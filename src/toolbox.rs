use crate::gui::{draw_button, draw_combo_item, field_proc, rounded, scale, wide};
use netbridge::{
    adapters::{self, Adapter},
    adb_manager::{self, Installation, Release, Version},
    config::Config,
    speed::{self, ResultRow},
    speed_view::{self, Preferences, SpeedUnit},
};
use std::{
    collections::BTreeMap,
    io, ptr,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
    UI::{
        Controls::*, Input::KeyboardAndMouse::EnableWindow, Shell::SetWindowSubclass,
        WindowsAndMessaging::*,
    },
};

const CHECK_ADB: u16 = 301;
const UPDATE_ADB: u16 = 109;
const MEASURE: u16 = 104;
const CANCEL: u16 = 302;
const REFRESH: u16 = 303;
const ADAPTER: u16 = 304;
const ADB_STATE: u16 = 305;
const ADB_PATH: u16 = 306;
const ADB_LATEST: u16 = 307;
const PROGRESS: u16 = 308;
const RESULTS: u16 = 309;
const UNIT: u16 = 310;
const CRITERIA: u16 = 311;
const BG: u32 = 0xf9f5f1;
const WHITE: u32 = 0xffffff;
const INK: u32 = 0x362517;
const MUTED: u32 = 0x806c5b;
const NAVY: u32 = 0x332011;
const BORDER: u32 = 0xede4dc;
const REFLOW: u32 = WM_APP + 7;

enum Event {
    Installed(Result<Installation, String>),
    Latest(Result<Release, String>),
    Updated(Result<Installation, String>),
    AdbProgress(String),
    Progress(String),
    Row(ResultRow),
    Done,
    Adapters(Result<Vec<Adapter>, String>),
}
struct State {
    hwnd: HWND,
    controls: BTreeMap<u16, HWND>,
    fonts: Vec<HFONT>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    adapters: Vec<Adapter>,
    installation: Option<Installation>,
    latest: Option<Release>,
    rows: Vec<ResultRow>,
    adb_busy: bool,
    speed_busy: bool,
    closing: bool,
    cancel: Arc<AtomicBool>,
    configured_adb: String,
    unit: SpeedUnit,
    preference_path: std::path::PathBuf,
}
impl State {
    fn new(config: Config) -> Self {
        let (tx, rx) = mpsc::channel();
        let preference_path = netbridge::config::data_dir().join("toolbox-preferences.json");
        let unit = Preferences::load(&preference_path).unwrap_or_default().unit;
        Self {
            hwnd: ptr::null_mut(),
            controls: BTreeMap::new(),
            fonts: Vec::new(),
            tx,
            rx,
            adapters: Vec::new(),
            installation: None,
            latest: None,
            rows: Vec::new(),
            adb_busy: true,
            speed_busy: false,
            closing: false,
            cancel: Arc::new(AtomicBool::new(false)),
            configured_adb: config.adb_path,
            unit,
            preference_path,
        }
    }
    fn control(&self, id: u16) -> HWND {
        *self.controls.get(&id).unwrap_or(&ptr::null_mut())
    }
    fn set(&self, id: u16, text: &str) {
        unsafe {
            SetWindowTextW(self.control(id), wide(text).as_ptr());
        }
    }
    unsafe fn font(&mut self, size: i32, weight: i32) -> HFONT {
        let font = CreateFontW(
            -scale(self.hwnd, size),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            wide("Segoe UI").as_ptr(),
        );
        self.fonts.push(font);
        font
    }
    unsafe fn add(&mut self, id: u16, class: &str, text: &str, style: u32, font: HFONT) {
        let control = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | style
                | if class == "STATIC" {
                    windows_sys::Win32::System::SystemServices::SS_NOPREFIX
                } else {
                    0
                },
            0,
            0,
            100,
            30,
            self.hwnd,
            id as usize as HMENU,
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font as usize, 0);
        self.controls.insert(id, control);
    }
    unsafe fn create(&mut self) {
        let title = self.font(28, 600);
        let heading = self.font(19, 600);
        let font = self.font(15, 400);
        let small = self.font(14, 400);
        for(id,text,f) in [
            (400,"ADB & tốc độ mạng",title),
            (401,"Công cụ portable · quản lý ADB và kiểm tra từng đường truyền",small),
            (402,"ADB tích hợp",heading),
            (ADB_STATE,"Đang chuẩn bị ADB đi kèm…",font),
            (ADB_PATH,"Lần đầu sẽ giải nén ADB; không cần cài Android SDK hoặc thêm PATH.",small),
            (ADB_LATEST,"Nguồn cập nhật: Google · chỉ phiên bản ổn định",small),
            (403,"Tốc độ Internet theo đường mạng",heading),
            (404,"Cloudflare · khoảng 12 MB/mạng · HTTP latency, download và upload một luồng",small),
            (PROGRESS,"Chọn một adapter hoặc tất cả rồi bấm Đo tốc độ.",font),
            (405,"Kết quả tham khảo, phụ thuộc máy chủ và lưu lượng hiện tại. Không thay đổi proxy hoặc route.\nBấm Dừng sẽ kết thúc sau yêu cầu đang chạy, tối đa khoảng 15 giây.",small),
            (406,"Đơn vị tốc độ",small),
            (407,"Đổi đơn vị không cần đo lại · đánh giá tham khảo",small),
        ] { self.add(id,"STATIC",text,0,f); }
        for (id, text) in [
            (CHECK_ADB, "Kiểm tra bản mới"),
            (UPDATE_ADB, "Cập nhật ADB"),
            (REFRESH, "Làm mới"),
            (MEASURE, "Đo tốc độ"),
            (CANCEL, "Dừng"),
            (CRITERIA, "Tiêu chí đánh giá"),
        ] {
            self.add(id, "BUTTON", text, WS_TABSTOP | BS_OWNERDRAW as u32, font);
        }
        self.add(
            ADAPTER,
            "COMBOBOX",
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32,
            font,
        );
        SendMessageW(
            self.control(ADAPTER),
            CB_SETITEMHEIGHT,
            usize::MAX,
            scale(self.hwnd, 28) as isize,
        );
        SendMessageW(
            self.control(ADAPTER),
            CB_SETITEMHEIGHT,
            0,
            scale(self.hwnd, 32) as isize,
        );
        SendMessageW(
            self.control(ADAPTER),
            CB_SETDROPPEDWIDTH,
            scale(self.hwnd, 580) as usize,
            0,
        );
        SetWindowSubclass(self.control(ADAPTER), Some(field_proc), 1, 0);
        self.add(
            UNIT,
            "COMBOBOX",
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32,
            font,
        );
        SendMessageW(
            self.control(UNIT),
            CB_SETITEMHEIGHT,
            usize::MAX,
            scale(self.hwnd, 28) as isize,
        );
        SendMessageW(
            self.control(UNIT),
            CB_SETITEMHEIGHT,
            0,
            scale(self.hwnd, 32) as isize,
        );
        for unit in SpeedUnit::ALL {
            SendMessageW(
                self.control(UNIT),
                CB_ADDSTRING,
                0,
                wide(unit.label()).as_ptr() as isize,
            );
        }
        SendMessageW(
            self.control(UNIT),
            CB_SETCURSEL,
            SpeedUnit::ALL
                .iter()
                .position(|unit| *unit == self.unit)
                .unwrap_or(0),
            0,
        );
        SetWindowSubclass(self.control(UNIT), Some(field_proc), 1, 0);
        self.add(
            RESULTS,
            "EDIT",
            "Chưa có kết quả đo.",
            WS_TABSTOP | WS_VSCROLL | (ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY) as u32,
            font,
        );
        SendMessageW(
            self.control(RESULTS),
            EM_SETMARGINS,
            (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
            (12 | (12 << 16)) as isize,
        );
        self.layout();
        self.refresh();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Event::Installed(
                adb_manager::ensure_bundled().map_err(|e| e.to_string()),
            ));
        });
        self.buttons();
        SetTimer(self.hwnd, 1, 150, None);
    }
    unsafe fn layout(&self) {
        let (w, h) = size(self.hwnd);
        for (id, x, y, width, height) in [
            (400, 24, 18, w - 48, 38),
            (401, 24, 58, w - 48, 22),
            (402, 44, 128, w - 88, 28),
            (ADB_STATE, 44, 166, w - 88, 24),
            (ADB_PATH, 44, 196, w - 88, 42),
            (ADB_LATEST, 44, 244, w - 88, 24),
            (CHECK_ADB, 44, 282, 190, 34),
            (UPDATE_ADB, 246, 282, 180, 34),
            (403, 44, 366, w - 88, 28),
            (404, 44, 400, w - 88, 22),
            (ADAPTER, 44, 434, w - 430, 280),
            (REFRESH, w - 374, 434, 100, 34),
            (MEASURE, w - 262, 434, 130, 34),
            (CANCEL, w - 120, 434, 76, 34),
            (PROGRESS, 44, 482, w - 88, 38),
            (406, 44, 534, 120, 22),
            (UNIT, 174, 526, 130, 220),
            (CRITERIA, 320, 526, 166, 34),
            (407, 502, 534, w - 546, 22),
            (RESULTS, 44, 574, w - 88, (h - 664).max(60)),
            (405, 44, h - 76, w - 88, 44),
        ] {
            MoveWindow(
                self.control(id),
                scale(self.hwnd, x),
                scale(self.hwnd, y),
                scale(self.hwnd, width),
                scale(self.hwnd, height),
                0,
            );
        }
        RedrawWindow(
            self.hwnd,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
        );
    }
    fn refresh(&self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Event::Adapters(
                adapters::enumerate().map_err(|e| e.to_string()),
            ));
        });
    }
    fn show_installation(&self) {
        if let Some(installation) = &self.installation {
            self.set(
                ADB_STATE,
                &format!("ADB {} · sẵn sàng dùng ngoại tuyến", installation.version),
            );
            let custom = !self.configured_adb.is_empty()
                && !self.configured_adb.eq_ignore_ascii_case("auto");
            let full_path = installation.path.display().to_string();
            let display_path = if full_path.chars().count() > 100 {
                format!(
                    "…{}",
                    full_path
                        .chars()
                        .rev()
                        .take(99)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<String>()
                )
            } else {
                full_path
            };
            self.set(ADB_PATH,&format!("{}\n{}",display_path,if custom {
                "Màn hình chính đang có đường dẫn tùy chỉnh; nhập auto tại ô ADB để dùng bản tích hợp."
            } else {"Chế độ auto dùng bản tích hợp. Không cần cài SDK hoặc cấu hình PATH."}));
        }
    }
    fn buttons(&self) {
        let idle = !self.adb_busy && !self.speed_busy;
        let update = self.latest.as_ref().is_some_and(|latest| {
            self.installation.as_ref().is_none_or(|installed| {
                Version::from_str(&latest.version).ok() > Version::from_str(&installed.version).ok()
            })
        });
        unsafe {
            EnableWindow(self.control(CHECK_ADB), idle as i32);
            EnableWindow(self.control(UPDATE_ADB), (idle && update) as i32);
            EnableWindow(
                self.control(MEASURE),
                (idle && !self.adapters.is_empty()) as i32,
            );
            EnableWindow(self.control(REFRESH), idle as i32);
            EnableWindow(self.control(ADAPTER), idle as i32);
            EnableWindow(
                self.control(CANCEL),
                (self.speed_busy && !self.cancel.load(Ordering::Relaxed)) as i32,
            );
        }
    }
    fn start_measurement(&mut self) {
        if self.speed_busy || self.adb_busy {
            return;
        }
        let index = unsafe { SendMessageW(self.control(ADAPTER), CB_GETCURSEL, 0, 0) };
        let targets = if index == 0 {
            self.adapters.clone()
        } else {
            self.adapters
                .get((index - 1) as usize)
                .cloned()
                .into_iter()
                .collect()
        };
        if targets.is_empty() {
            self.set(PROGRESS, "Không có adapter đang hoạt động.");
            return;
        }
        self.speed_busy = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.rows.clear();
        self.set(
            RESULTS,
            "Đang đo. Kết quả từng adapter sẽ xuất hiện tại đây.",
        );
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        thread::spawn(move || {
            for adapter in targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let row = speed::measure_adapter(&adapter, &cancel, &|message| {
                    let _ = tx.send(Event::Progress(message));
                });
                let _ = tx.send(Event::Row(row));
            }
            let _ = tx.send(Event::Done);
        });
        self.buttons();
    }
    fn action(&mut self, id: u16) {
        match id {
            UNIT => {
                let selected = unsafe { SendMessageW(self.control(UNIT), CB_GETCURSEL, 0, 0) };
                if let Some(unit) = SpeedUnit::ALL.get(selected as usize) {
                    self.unit = *unit;
                    if !self.rows.is_empty() {
                        self.render_results();
                    }
                    if let Err(error) = (Preferences { unit: *unit }).save(&self.preference_path) {
                        self.set(
                            PROGRESS,
                            &format!("Đã đổi đơn vị; chưa lưu được lựa chọn: {error}"),
                        );
                    }
                }
            }
            CRITERIA => unsafe {
                MessageBoxW(
                    self.hwnd,
                    wide(speed_view::CRITERIA).as_ptr(),
                    wide("Tiêu chí đánh giá mạng").as_ptr(),
                    MB_OK | MB_ICONINFORMATION,
                );
            },
            CHECK_ADB if !self.adb_busy && !self.speed_busy => {
                self.adb_busy = true;
                self.set(ADB_LATEST, "Đang kiểm tra kho Google…");
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let _ = tx.send(Event::Latest(
                        adb_manager::check_update().map_err(|e| e.to_string()),
                    ));
                });
            }
            UPDATE_ADB if !self.adb_busy && !self.speed_busy => {
                self.adb_busy = true;
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let result = adb_manager::update(&|message| {
                        let _ = tx.send(Event::AdbProgress(message));
                    })
                    .map_err(|e| e.to_string());
                    let _ = tx.send(Event::Updated(result));
                });
            }
            MEASURE => self.start_measurement(),
            CANCEL => {
                self.cancel.store(true, Ordering::Relaxed);
                self.set(
                    PROGRESS,
                    "Đang dừng; chờ yêu cầu hiện tại kết thúc (tối đa khoảng 15 giây)…",
                );
            }
            REFRESH if !self.adb_busy && !self.speed_busy => self.refresh(),
            _ => {}
        }
        self.buttons();
    }
    fn render_results(&self) {
        let text = self
            .rows
            .iter()
            .map(|row| speed_view::render_result(row, self.unit))
            .collect::<Vec<_>>()
            .join("\r\n");
        self.set(RESULTS, &text);
    }
    fn tick(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Installed(result) | Event::Updated(result) => {
                    self.adb_busy = false;
                    match result {
                        Ok(installed) => {
                            self.installation = Some(installed);
                            self.show_installation();
                            self.set(
                                ADB_LATEST,
                                "ADB đã sẵn sàng. Bấm Kiểm tra bản mới để đối chiếu với Google.",
                            );
                        }
                        Err(error) => self.set(ADB_STATE, &format!("ADB: {error}")),
                    }
                }
                Event::Latest(result) => {
                    self.adb_busy = false;
                    match result {
                        Ok(release) => {
                            let newer = self.installation.as_ref().is_none_or(|i| {
                                Version::from_str(&release.version).ok()
                                    > Version::from_str(&i.version).ok()
                            });
                            self.set(
                                ADB_LATEST,
                                &format!(
                                    "Google stable: {} · {}",
                                    release.version,
                                    if newer {
                                        "Có bản mới, có thể cập nhật."
                                    } else {
                                        "Đã là phiên bản mới nhất."
                                    }
                                ),
                            );
                            self.latest = Some(release);
                        }
                        Err(error) => {
                            self.set(ADB_LATEST, &format!("Không kiểm tra được: {error}"))
                        }
                    }
                }
                Event::AdbProgress(message) => self.set(ADB_LATEST, &message),
                Event::Progress(message) => {
                    if !self.cancel.load(Ordering::Relaxed) {
                        self.set(PROGRESS, &message)
                    }
                }
                Event::Row(row) => {
                    self.rows.push(row);
                    self.render_results();
                }
                Event::Done => {
                    self.speed_busy = false;
                    self.set(
                        PROGRESS,
                        if self.cancel.load(Ordering::Relaxed) {
                            "Đã dừng. Giữ lại các kết quả đã đo."
                        } else {
                            "Đã hoàn tất. Kết quả bên dưới thuộc lần đo gần nhất."
                        },
                    );
                }
                Event::Adapters(result) => match result {
                    Ok(list) => {
                        let selected =
                            unsafe { SendMessageW(self.control(ADAPTER), CB_GETCURSEL, 0, 0) };
                        let id = self
                            .adapters
                            .get(selected.saturating_sub(1) as usize)
                            .filter(|_| selected > 0)
                            .map(|a| a.id.clone());
                        let mut seen = std::collections::HashSet::new();
                        self.adapters = list
                            .into_iter()
                            .filter(|a| seen.insert(a.id.clone()))
                            .collect();
                        unsafe {
                            SendMessageW(self.control(ADAPTER), CB_RESETCONTENT, 0, 0);
                            SendMessageW(
                                self.control(ADAPTER),
                                CB_ADDSTRING,
                                0,
                                wide("Tất cả adapter đang hoạt động").as_ptr() as isize,
                            );
                            for adapter in &self.adapters {
                                SendMessageW(
                                    self.control(ADAPTER),
                                    CB_ADDSTRING,
                                    0,
                                    wide(&format!("{} · {}", adapter.name, adapter.ip)).as_ptr()
                                        as isize,
                                );
                            }
                            let index = id
                                .and_then(|id| self.adapters.iter().position(|a| a.id == id))
                                .map(|i| i + 1)
                                .unwrap_or(0);
                            SendMessageW(self.control(ADAPTER), CB_SETCURSEL, index, 0);
                        }
                    }
                    Err(error) => self.set(PROGRESS, &format!("Không đọc được adapter: {error}")),
                },
            }
        }
        self.buttons();
        if self.closing && !self.adb_busy {
            unsafe {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe fn size(hwnd: HWND) -> (i32, i32) {
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as i32;
    (rect.right * 96 / dpi, rect.bottom * 96 / dpi)
}
unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let (w, h) = size(hwnd);
    for (x, y, width, height, fill, border) in [
        (0, 0, w, 92, NAVY, NAVY),
        (24, 112, w - 48, 222, WHITE, BORDER),
        (24, 350, w - 48, h - 374, WHITE, BORDER),
    ] {
        rounded(
            dc,
            RECT {
                left: scale(hwnd, x),
                top: scale(hwnd, y),
                right: scale(hwnd, x + width),
                bottom: scale(hwnd, y + height),
            },
            fill,
            border,
            scale(hwnd, 14),
        );
    }
    EndPaint(hwnd, &ps);
}
thread_local! {static HANDLING:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        HANDLING.with(|v| v.set(false));
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let state = (*(l as *const CREATESTRUCTW)).lpCreateParams as *mut State;
        (*state).hwnd = hwnd;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
    }
    match message {
        WM_GETMINMAXINFO => {
            (*(l as *mut MINMAXINFO)).ptMinTrackSize = POINT {
                x: scale(hwnd, 920),
                y: scale(hwnd, 780),
            };
            return 0;
        }
        WM_PAINT => {
            paint(hwnd);
            return 0;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let id = GetDlgCtrlID(l as HWND) as u16;
            let bg = if matches!(id, 400 | 401) { NAVY } else { WHITE };
            let color = if id == 400 {
                WHITE
            } else if id == 401 {
                0xd8c6b2
            } else if matches!(id, 402 | 403 | ADB_STATE | PROGRESS | RESULTS) {
                INK
            } else {
                MUTED
            };
            SetBkColor(w as HDC, bg);
            SetTextColor(w as HDC, color);
            SetDCBrushColor(w as HDC, bg);
            return GetStockObject(DC_BRUSH) as isize;
        }
        WM_DRAWITEM => {
            let item = &*(l as *const DRAWITEMSTRUCT);
            if item.CtlType == ODT_BUTTON {
                draw_button(item);
                return 1;
            }
            if item.CtlType == ODT_COMBOBOX {
                draw_combo_item(item);
                return 1;
            }
            return DefWindowProcW(hwnd, message, w, l);
        }
        WM_MEASUREITEM => {
            let item = &mut *(l as *mut MEASUREITEMSTRUCT);
            if item.CtlType == ODT_COMBOBOX {
                item.itemHeight = scale(hwnd, 32) as u32;
                return 1;
            }
            return DefWindowProcW(hwnd, message, w, l);
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            PostQuitMessage(0);
            return 0;
        }
        WM_ACTIVATEAPP => {
            if w != 0 {
                PostMessageW(hwnd, REFLOW, 0, 0);
            }
            return DefWindowProcW(hwnd, message, w, l);
        }
        WM_COMMAND => {
            let id = (w & 0xffff) as u16;
            let notification = (w >> 16) & 0xffff;
            if notification != BN_CLICKED as usize
                && !(id == UNIT && notification == CBN_SELCHANGE as usize)
            {
                return 0;
            }
        }
        WM_CREATE | WM_SIZE | WM_TIMER | WM_CLOSE | REFLOW => {}
        _ => return DefWindowProcW(hwnd, message, w, l),
    }
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    if state.is_null() {
        return DefWindowProcW(hwnd, message, w, l);
    }
    if HANDLING.with(|v| v.replace(true)) {
        if message == WM_SIZE {
            PostMessageW(hwnd, REFLOW, 0, 0);
        }
        return 0;
    }
    let _guard = Guard;
    let state = &mut *state;
    match message {
        WM_CREATE => state.create(),
        WM_SIZE | REFLOW => state.layout(),
        WM_TIMER => state.tick(),
        WM_COMMAND => state.action((w & 0xffff) as u16),
        WM_CLOSE => {
            state.cancel.store(true, Ordering::Relaxed);
            if state.adb_busy {
                state.closing = true;
                state.set(ADB_LATEST, "Đang hoàn tất tác vụ ADB trước khi đóng…");
            } else {
                DestroyWindow(hwnd);
            }
        }
        _ => {}
    }
    0
}

pub fn run(config: Config) -> io::Result<()> {
    unsafe {
        let mutex = CreateMutexW(ptr::null(), 0, wide("Local\\NetBridge.Toolbox").as_ptr());
        if mutex.is_null() {
            return Err(io::Error::last_os_error());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let hwnd = FindWindowW(wide("NetBridgeToolbox").as_ptr(), ptr::null());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
            CloseHandle(mutex);
            return Ok(());
        }
        SetProcessDPIAware();
        let mut state = Box::new(State::new(config));
        let brush = CreateSolidBrush(BG);
        let class = wide("NetBridgeToolbox");
        let instance = GetModuleHandleW(ptr::null());
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: brush,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            CloseHandle(mutex);
            DeleteObject(brush as HGDIOBJ);
            return Err(io::Error::last_os_error());
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("NetBridge · ADB & tốc độ mạng").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            860,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            (&mut *state as *mut State).cast(),
        );
        if hwnd.is_null() {
            CloseHandle(mutex);
            return Err(io::Error::last_os_error());
        }
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for font in &state.fonts {
            DeleteObject(*font as HGDIOBJ);
        }
        UnregisterClassW(class.as_ptr(), instance);
        DeleteObject(brush as HGDIOBJ);
        CloseHandle(mutex);
    }
    Ok(())
}

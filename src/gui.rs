use netbridge::{
    adapters::{self, Adapter},
    config::{self, Config, Rule},
    devices::{self, Device},
    dial,
    policy::{self, Route},
    proxy::{self, Log, Server, SharedConfig},
    system_proxy,
};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    path::PathBuf,
    ptr,
    sync::{mpsc, Arc, Mutex, RwLock},
    thread,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, IsWindowEnabled};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        SystemServices::{SS_CENTER, SS_OWNERDRAW},
        Threading::CreateMutexW,
    },
    UI::{Controls::*, HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
};

const LAN: u16 = 101;
const USB: u16 = 102;
const REFRESH: u16 = 103;
const START: u16 = 104;
const STOP: u16 = 105;
const RULES: u16 = 106;
const PATTERN: u16 = 107;
const ROUTE: u16 = 108;
const ADD: u16 = 109;
const REMOVE: u16 = 110;
const DEVICES: u16 = 111;
const REFRESH_USB: u16 = 112;
const USB_ON: u16 = 113;
const USB_OFF: u16 = 114;
const ADB: u16 = 115;
const CHECK: u16 = 116;
const LOGS: u16 = 117;
const STATUS: u16 = 118;
const STATUS_DETAIL: u16 = 119;
const USB_HINT: u16 = 120;
const CHECK_SUMMARY: u16 = 121;
const PAUSE_LOG: u16 = 122;
const LAN_INFO: u16 = 123;
const USB_INFO: u16 = 124;
const TOOLBOX: u16 = 125;
const CHECK_RESULT: u16 = 240;
const CHECK_TARGETS: [(&str, u16, &str); 4] = [
    ("git.etc.vn", 443, "Git ETC"),
    ("10.3.6.5", 12443, "SRS"),
    ("www.onorca.dev", 443, "Orca"),
    ("103.122.82.124", 1883, "MQTT"),
];
const fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    r | (g << 8) | (b << 16)
}
const BACKGROUND: COLORREF = rgb(241, 245, 249);
const SURFACE: COLORREF = rgb(255, 255, 255);
const INK: COLORREF = rgb(23, 37, 54);
const MUTED: COLORREF = rgb(91, 108, 128);
const ACCENT: COLORREF = rgb(15, 118, 110);
const BORDER: COLORREF = rgb(220, 228, 237);
const SUCCESS: COLORREF = rgb(20, 119, 81);
const WARNING: COLORREF = rgb(180, 76, 28);
const NAVY: COLORREF = rgb(17, 32, 51);
const HEADER_TEXT: COLORREF = rgb(178, 198, 216);
const BLUE: COLORREF = rgb(42, 94, 184);
const BLUE_TINT: COLORREF = rgb(235, 242, 255);
const TEAL_TINT: COLORREF = rgb(230, 246, 242);
const LOG_BACKGROUND: COLORREF = rgb(22, 37, 55);
const LOG_TEXT: COLORREF = rgb(207, 223, 236);
const FIELD_BORDER: COLORREF = rgb(185, 201, 214);
const FIELD_FILL: COLORREF = rgb(249, 251, 253);
pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
enum Event {
    Adapters(Result<Vec<Adapter>, String>),
    Devices(Result<Vec<Device>, String>),
    Usb(Result<String, String>),
    CheckResult(usize, bool, String),
    CheckDone,
}

struct State {
    hwnd: HWND,
    controls: BTreeMap<u16, HWND>,
    fonts: Vec<HFONT>,
    brush: HBRUSH,
    config: SharedConfig,
    path: PathBuf,
    backup: PathBuf,
    adapters: Vec<Adapter>,
    devices: Vec<Device>,
    logs: Log,
    server: Option<Server>,
    proxy_enabled: bool,
    watchdog_started: bool,
    usb_busy: bool,
    check_busy: bool,
    check_results: [Option<bool>; 4],
    log_paused: bool,
    refresh_after_usb: u8,
    repaint_ticks: u8,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    last_log: String,
}
impl State {
    fn new(config: Config, path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            hwnd: ptr::null_mut(),
            controls: BTreeMap::new(),
            fonts: Vec::new(),
            brush: ptr::null_mut(),
            config: Arc::new(RwLock::new(config)),
            path,
            backup: config::data_dir().join("proxy-backup.json"),
            adapters: Vec::new(),
            devices: Vec::new(),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            server: None,
            proxy_enabled: false,
            watchdog_started: false,
            usb_busy: false,
            check_busy: false,
            check_results: [None; 4],
            log_paused: false,
            refresh_after_usb: 0,
            repaint_ticks: 3,
            tx,
            rx,
            last_log: String::new(),
        }
    }
    fn control(&self, id: u16) -> HWND {
        *self.controls.get(&id).unwrap_or(&ptr::null_mut())
    }
    fn text(&self, id: u16) -> String {
        unsafe {
            let h = self.control(id);
            let n = GetWindowTextLengthW(h).max(0) as usize;
            let mut v = vec![0; n + 1];
            let len = GetWindowTextW(h, v.as_mut_ptr(), v.len() as i32);
            String::from_utf16_lossy(&v[..len.max(0) as usize])
        }
    }
    fn set(&self, id: u16, text: &str) {
        unsafe {
            SetWindowTextW(self.control(id), wide(text).as_ptr());
        }
    }
    fn tone(&self, id: u16, color: COLORREF) {
        unsafe {
            SetWindowLongPtrW(self.control(id), GWLP_USERDATA, color as isize);
            InvalidateRect(self.control(id), ptr::null(), 1);
        }
    }
    fn index(&self, id: u16) -> Option<usize> {
        unsafe {
            let n = SendMessageW(self.control(id), CB_GETCURSEL, 0, 0);
            if n < 0 {
                None
            } else {
                Some(n as usize)
            }
        }
    }
    fn error(&self, error: impl std::fmt::Display) {
        proxy::log(&self.logs, error.to_string());
        show_message(self.hwnd, "NetBridge", &error.to_string(), MB_ICONWARNING);
    }
    fn capture(&self) -> io::Result<Config> {
        let mut cfg = self
            .config
            .read()
            .map_err(|_| io::Error::other("Configuration locked"))?
            .clone();
        if let Some(i) = self.index(LAN).and_then(|i| i.checked_sub(1)) {
            if let Some(a) = self.adapters.get(i) {
                cfg.lan_id = a.id.clone();
            }
        }
        if let Some(i) = self.index(USB).and_then(|i| i.checked_sub(1)) {
            if let Some(a) = self.adapters.get(i) {
                cfg.usb_id = a.id.clone();
            }
        }
        cfg.adb_path = self.text(ADB).trim().into();
        cfg.validate()?;
        Ok(cfg)
    }
    fn save(&mut self) -> io::Result<Config> {
        let cfg = self.capture()?;
        cfg.save(&self.path)?;
        *self
            .config
            .write()
            .map_err(|_| io::Error::other("Configuration locked"))? = cfg.clone();
        self.update_network_hint();
        Ok(cfg)
    }
    fn missing_network(&self) -> Option<&'static str> {
        let cfg = self.config.read().unwrap_or_else(|e| e.into_inner());
        let selected = |id| {
            self.index(id)
                .and_then(|i| i.checked_sub(1))
                .is_some_and(|i| i < self.adapters.len())
        };
        let lan = cfg.rules.iter().any(|r| r.route == Route::Lan) && !selected(LAN);
        let usb = cfg.rules.iter().any(|r| r.route == Route::Usb) && !selected(USB);
        match (lan, usb) {
            (true, true) => Some("Chọn card LAN và USB ở bước 02 để bắt đầu."),
            (true, false) => Some("Chọn card LAN ở bước 02 để bắt đầu."),
            (false, true) => Some("Kết nối Ethernet USB, rồi chọn card USB ở bước 02."),
            _ => None,
        }
    }
    fn update_network_hint(&self) {
        for (selector, label, color) in [(LAN, LAN_INFO, BLUE), (USB, USB_INFO, ACCENT)] {
            let adapter = self
                .index(selector)
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| self.adapters.get(i));
            self.set(
                label,
                if adapter.is_some() {
                    "Đã chọn adapter"
                } else {
                    "Chưa chọn card mạng"
                },
            );
            self.tone(label, if adapter.is_some() { color } else { MUTED });
        }
        if self.server.is_none() {
            self.set(
                STATUS_DETAIL,
                self.missing_network()
                    .unwrap_or("Đã chọn đủ mạng. Kiểm tra kết nối rồi bật phân luồng."),
            );
        }
    }
    fn refresh(&self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Event::Adapters(
                adapters::enumerate().map_err(|e| e.to_string()),
            ));
        });
    }
    fn refresh_usb(&self) {
        self.set(USB_HINT, "Đang tìm thiết bị Android qua USB…");
        let tx = self.tx.clone();
        let adb = self.text(ADB);
        thread::spawn(move || {
            let _ = tx.send(Event::Devices(
                devices::list(&adb).map_err(|e| e.to_string()),
            ));
        });
    }
    fn fill_networks(&mut self, list: Vec<Adapter>) {
        self.adapters = list;
        let mut cfg = self.config.write().unwrap_or_else(|e| e.into_inner());
        cfg.auto_select(&self.adapters);
        for (id, selected) in [(LAN, &cfg.lan_id), (USB, &cfg.usb_id)] {
            unsafe {
                SendMessageW(self.control(id), CB_RESETCONTENT, 0, 0);
                SendMessageW(
                    self.control(id),
                    CB_ADDSTRING,
                    0,
                    wide("Chọn card mạng").as_ptr() as isize,
                );
                let mut current = 0;
                for (i, a) in self.adapters.iter().enumerate() {
                    let label = format!("{} · {}", a.name, a.ip);
                    SendMessageW(
                        self.control(id),
                        CB_ADDSTRING,
                        0,
                        wide(&label).as_ptr() as isize,
                    );
                    if &a.id == selected {
                        current = i + 1
                    }
                }
                SendMessageW(self.control(id), CB_SETCURSEL, current, 0);
            }
        }
        drop(cfg);
        self.update_network_hint();
    }
    fn fill_rules(&self) {
        unsafe {
            SendMessageW(self.control(RULES), LB_RESETCONTENT, 0, 0);
            let cfg = self.config.read().unwrap_or_else(|e| e.into_inner());
            for r in &cfg.rules {
                SendMessageW(
                    self.control(RULES),
                    LB_ADDSTRING,
                    0,
                    wide(&format!("{}    →    {}", r.pattern, route_name(r.route))).as_ptr()
                        as isize,
                );
            }
        }
    }
    fn fill_devices(&mut self, devices: Vec<Device>) {
        let selected = self
            .index(DEVICES)
            .and_then(|i| self.devices.get(i))
            .map(|d| d.serial.clone());
        self.devices = devices.into_iter().filter(|d| d.usb).collect();
        unsafe {
            SendMessageW(self.control(DEVICES), CB_RESETCONTENT, 0, 0);
            for d in &self.devices {
                let state = match d.state.as_str() {
                    "device" => "Sẵn sàng",
                    "unauthorized" => "Chưa cấp quyền ADB",
                    "offline" => "Ngoại tuyến",
                    other => other,
                };
                SendMessageW(
                    self.control(DEVICES),
                    CB_ADDSTRING,
                    0,
                    wide(&format!("{} · {} · {}", d.model, d.serial, state)).as_ptr() as isize,
                );
            }
            if !self.devices.is_empty() {
                let index = self
                    .devices
                    .iter()
                    .position(|d| Some(&d.serial) == selected.as_ref())
                    .unwrap_or(0);
                SendMessageW(self.control(DEVICES), CB_SETCURSEL, index, 0);
            } else {
                SendMessageW(
                    self.control(DEVICES),
                    CB_ADDSTRING,
                    0,
                    wide("Chưa tìm thấy thiết bị USB").as_ptr() as isize,
                );
                SendMessageW(self.control(DEVICES), CB_SETCURSEL, 0, 0);
            }
        }
        self.update_device_hint();
    }
    fn update_device_hint(&self) {
        let text = if self.usb_busy {
            "Đang đổi chế độ USB… Chờ thiết bị kết nối lại."
        } else {
            match self.index(DEVICES).and_then(|i| self.devices.get(i)) {
                Some(d) if d.state == "device" => {
                    "Thiết bị sẵn sàng. Bật Wi-Fi trên Android trước khi kết nối."
                }
                Some(d) if d.state == "unauthorized" => {
                    "Mở khóa Android và chấp nhận yêu cầu USB debugging."
                }
                Some(_) => "Thiết bị chưa sẵn sàng. Kiểm tra cáp và tìm lại thiết bị.",
                None => "Cắm cáp USB, bật USB debugging rồi chọn Tìm thiết bị.",
            }
        };
        self.set(USB_HINT, text);
    }
    fn start(&mut self) -> io::Result<()> {
        if self.server.is_some() {
            return Ok(());
        }
        let cfg = self.save()?;
        for r in &cfg.rules {
            let id = match r.route {
                Route::Lan => &cfg.lan_id,
                Route::Usb => &cfg.usb_id,
                Route::System => continue,
            };
            if !self.adapters.iter().any(|a| &a.id == id) {
                return Err(io::Error::other(format!(
                    "Chưa chọn card {} đang kết nối",
                    route_name(r.route)
                )));
            }
        }
        let server = Server::start(self.config.clone(), self.logs.clone())?;
        if !self.watchdog_started {
            system_proxy::spawn_watchdog(&self.backup)?;
            self.watchdog_started = true;
        }
        if let Err(error) = system_proxy::enable(&self.backup, cfg.port) {
            // Keep the listener alive if Windows still points at it after a failed rollback.
            if system_proxy::read().map_or(true, |s| {
                s.server == format!("127.0.0.1:{}", cfg.port) && s.flags & 2 != 0
            }) {
                self.proxy_enabled = true;
                self.server = Some(server);
                self.set(STATUS, "Cần khôi phục proxy");
                self.set(
                    STATUS_DETAIL,
                    "Bấm Tắt / Khôi phục để thử phục hồi cấu hình.",
                );
                self.tone(STATUS, WARNING);
            }
            return Err(error);
        }
        self.proxy_enabled = true;
        self.server = Some(server);
        self.set(STATUS, "Đang phân luồng");
        self.set(
            STATUS_DETAIL,
            &format!("HTTP / SOCKS5 · 127.0.0.1:{}", cfg.port),
        );
        self.tone(STATUS, SUCCESS);
        proxy::log(
            &self.logs,
            "Đã bật proxy người dùng. Giữ cả LAN và USB. HTTPS được truyền nguyên vẹn.",
        );
        Ok(())
    }
    fn stop(&mut self) -> io::Result<()> {
        let mut restored = false;
        if self.proxy_enabled {
            restored = system_proxy::restore(&self.backup)?;
            self.proxy_enabled = false;
        }
        self.server.take();
        let message = if restored {
            "ĐÃ TẮT · Cấu hình proxy ban đầu đã được khôi phục"
        } else {
            "ĐÃ TẮT · Giữ cấu hình Windows hiện tại"
        };
        self.set(STATUS, "Phân luồng đang tắt");
        self.set(
            STATUS_DETAIL,
            if restored {
                "Đã khôi phục cấu hình proxy ban đầu."
            } else {
                "Đang dùng cấu hình mạng hiện tại của Windows."
            },
        );
        self.tone(STATUS, MUTED);
        proxy::log(&self.logs, message);
        Ok(())
    }
    fn add_rule(&mut self) -> io::Result<()> {
        let pattern = self.text(PATTERN).trim().to_ascii_lowercase();
        if !policy::valid_pattern(&pattern) {
            return Err(io::Error::other(
                "Nhập domain, IPv4 hoặc CIDR; ví dụ git.etc.vn hoặc 10.3.0.0/16",
            ));
        }
        let route = match self.index(ROUTE).unwrap_or(0) {
            0 => Route::Lan,
            1 => Route::Usb,
            _ => Route::System,
        };
        let mut cfg = self.capture()?;
        if let Some(r) = cfg.rules.iter_mut().find(|r| r.pattern == pattern) {
            r.route = route
        } else {
            cfg.rules.push(Rule { pattern, route })
        }
        cfg.save(&self.path)?;
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = cfg;
        self.fill_rules();
        self.set(PATTERN, "");
        self.update_network_hint();
        Ok(())
    }
    fn remove_rule(&mut self) -> io::Result<()> {
        let index = unsafe { SendMessageW(self.control(RULES), LB_GETCURSEL, 0, 0) };
        let mut cfg = self.capture()?;
        if index >= 0 && (index as usize) < cfg.rules.len() {
            cfg.rules.remove(index as usize);
            cfg.save(&self.path)?;
            *self.config.write().unwrap_or_else(|e| e.into_inner()) = cfg;
            self.fill_rules();
            self.update_network_hint();
        }
        Ok(())
    }
    fn tether(&mut self, on: bool) -> io::Result<()> {
        if self.usb_busy {
            return Ok(());
        }
        let d = self
            .index(DEVICES)
            .and_then(|i| self.devices.get(i))
            .ok_or_else(|| io::Error::other("Chọn thiết bị USB trước"))?
            .clone();
        let cfg = self.save()?;
        let tx = self.tx.clone();
        self.usb_busy = true;
        self.update_device_hint();
        proxy::log(
            &self.logs,
            format!(
                "{} USB Ethernet: {}",
                if on { "Đang bật" } else { "Đang ngắt" },
                d.serial
            ),
        );
        thread::spawn(move || {
            let result =
                devices::set_tether(&cfg.adb_path, &d.serial, on).map_err(|e| e.to_string());
            let _ = tx.send(Event::Usb(result));
        });
        Ok(())
    }
    fn check(&mut self) -> io::Result<()> {
        if self.check_busy {
            return Ok(());
        }
        let cfg = self.save()?;
        let logs = self.logs.clone();
        let tx = self.tx.clone();
        self.check_busy = true;
        self.check_results = [None; 4];
        self.set(CHECK, "Đang kiểm tra…");
        self.set(
            CHECK_SUMMARY,
            "Đang kiểm tra TCP · Kết quả sẽ hiện tại từng đích.",
        );
        for (i, (_, _, name)) in CHECK_TARGETS.iter().enumerate() {
            let (host, port, _) = CHECK_TARGETS[i];
            self.set(
                CHECK_RESULT + i as u16,
                &format!("{name} · Đang kiểm tra…\n{host}:{port}"),
            );
            self.tone(CHECK_RESULT + i as u16, MUTED);
        }
        unsafe {
            EnableWindow(self.control(CHECK), 0);
        }
        thread::spawn(move || {
            match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime.block_on(async {
                    for (i, (host, port, name)) in CHECK_TARGETS.into_iter().enumerate() {
                        let route = cfg.select(host);
                        match dial::connect(&cfg, host, port).await {
                            Ok((s, _)) => {
                                let _ = tx.send(Event::CheckResult(
                                    i,
                                    true,
                                    format!("{name} · ĐẠT · {}", route_name(route)),
                                ));
                                proxy::log(
                                    &logs,
                                    format!(
                                        "OK  {host}:{port} → {} ({})",
                                        route_name(route),
                                        s.local_addr()
                                            .map(|a| a.ip().to_string())
                                            .unwrap_or_default()
                                    ),
                                );
                            }
                            Err(e) => {
                                let _ = tx.send(Event::CheckResult(
                                    i,
                                    false,
                                    format!("{name} · LỖI · {}", route_name(route)),
                                ));
                                proxy::log(
                                    &logs,
                                    format!("LỖI  {host}:{port} → {}: {e}", route_name(route)),
                                );
                            }
                        }
                    }
                }),
                Err(e) => {
                    proxy::log(&logs, e.to_string());
                    for (i, (_, _, name)) in CHECK_TARGETS.iter().enumerate() {
                        let _ = tx.send(Event::CheckResult(
                            i,
                            false,
                            format!("{name} · Không thể kiểm tra"),
                        ));
                    }
                }
            }
            let _ = tx.send(Event::CheckDone);
        });
        Ok(())
    }
    fn tick(&mut self) {
        if self.repaint_ticks > 0 {
            self.repaint_ticks -= 1;
            unsafe {
                RedrawWindow(
                    self.hwnd,
                    ptr::null(),
                    ptr::null_mut(),
                    RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE | RDW_UPDATENOW,
                );
            }
        }
        if self.refresh_after_usb > 0 {
            self.refresh_after_usb -= 1;
            if self.refresh_after_usb == 0 {
                self.refresh();
            }
        }
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Adapters(Ok(v)) => self.fill_networks(v),
                Event::Adapters(Err(e)) => proxy::log(&self.logs, e),
                Event::Devices(Err(e)) => {
                    self.set(
                        USB_HINT,
                        "Không tìm được thiết bị. Kiểm tra đường dẫn adb.exe và nhật ký.",
                    );
                    proxy::log(&self.logs, e);
                }
                Event::Devices(Ok(v)) => self.fill_devices(v),
                Event::Usb(result) => {
                    self.usb_busy = false;
                    self.update_device_hint();
                    self.refresh_after_usb = 8;
                    match result {
                        Ok(v) => proxy::log(&self.logs, v),
                        Err(e) => self.error(e),
                    }
                    self.refresh();
                    self.refresh_usb();
                }
                Event::CheckResult(i, ok, text) => {
                    self.check_results[i] = Some(ok);
                    let (host, port, _) = CHECK_TARGETS[i];
                    self.set(CHECK_RESULT + i as u16, &format!("{text}\n{host}:{port}"));
                    self.tone(CHECK_RESULT + i as u16, if ok { SUCCESS } else { WARNING });
                }
                Event::CheckDone => {
                    self.check_busy = false;
                    let passed = self
                        .check_results
                        .iter()
                        .filter(|r| **r == Some(true))
                        .count();
                    self.set(CHECK, "Kiểm tra lại");
                    self.set(
                        CHECK_SUMMARY,
                        &format!("{passed}/4 đích kết nối TCP · Chưa kiểm tra chứng chỉ TLS."),
                    );
                    unsafe {
                        EnableWindow(self.control(CHECK), 1);
                    }
                }
            }
        }
        let log = self
            .logs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\r\n");
        if !self.log_paused && log != self.last_log {
            self.set(LOGS, &log);
            unsafe {
                SendMessageW(
                    self.control(LOGS),
                    EM_SETSEL,
                    log.encode_utf16().count(),
                    log.encode_utf16().count() as isize,
                );
                SendMessageW(self.control(LOGS), EM_SCROLLCARET, 0, 0);
            }
            self.last_log = log;
        }
        unsafe {
            let device_ready = !self.usb_busy
                && self
                    .index(DEVICES)
                    .and_then(|i| self.devices.get(i))
                    .is_some_and(|d| d.state == "device");
            EnableWindow(self.control(USB_ON), device_ready as i32);
            EnableWindow(self.control(USB_OFF), device_ready as i32);
            EnableWindow(self.control(DEVICES), (!self.usb_busy) as i32);
            EnableWindow(self.control(REFRESH_USB), (!self.usb_busy) as i32);
            EnableWindow(
                self.control(REMOVE),
                (SendMessageW(self.control(RULES), LB_GETCURSEL, 0, 0) >= 0) as i32,
            );
            EnableWindow(
                self.control(START),
                (self.server.is_none() && self.missing_network().is_none()) as i32,
            );
            EnableWindow(self.control(STOP), self.server.is_some() as i32);
        }
    }
}
fn route_name(route: Route) -> &'static str {
    match route {
        Route::Lan => "LAN",
        Route::Usb => "USB",
        Route::System => "Theo Windows",
    }
}

// Painting uses only HWND/geometry, never borrows State during a Win32 callback.
pub(crate) unsafe fn scale(hwnd: HWND, value: i32) -> i32 {
    value * GetDpiForWindow(hwnd).max(96) as i32 / 96
}
// Tighten vertical spacing on compact windows without shrinking the text.
unsafe fn vertical(hwnd: HWND, value: i32) -> i32 {
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let height = rect.bottom * 96 / GetDpiForWindow(hwnd).max(96) as i32;
    scale(hwnd, value * height.min(760) / 760)
}
unsafe fn geometry(hwnd: HWND) -> (i32, i32, i32, i32, i32) {
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let dpi = GetDpiForWindow(hwnd).max(96) as i32;
    let w = rect.right * 96 / dpi;
    let h = (rect.bottom * 96 / dpi).max(760);
    let left = (w - 68) * 44 / 100;
    let right = 44 + left;
    (w, h, left, right, w - right - 24)
}
pub(crate) unsafe fn rounded(dc: HDC, rect: RECT, fill: COLORREF, border: COLORREF, radius: i32) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(dc, brush as HGDIOBJ);
    let old_pen = SelectObject(dc, pen as HGDIOBJ);
    RoundRect(
        dc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius,
        radius,
    );
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen as HGDIOBJ);
    DeleteObject(brush as HGDIOBJ);
}
unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let (w, h, left, right, right_width) = geometry(hwnd);
    let previous = SelectObject(
        dc,
        SendMessageW(GetDlgItem(hwnd, 201), WM_GETFONT, 0, 0) as HGDIOBJ,
    );
    rounded(dc, dip_rect(hwnd, 0, 0, w, 88), NAVY, NAVY, 0);
    // A compact bridge mark, drawn with GDI so the portable binary needs no assets.
    rounded(
        dc,
        dip_rect(hwnd, 24, 22, 44, 44),
        ACCENT,
        ACCENT,
        scale(hwnd, 12),
    );
    rounded(
        dc,
        dip_rect(hwnd, 34, 34, 10, 10),
        SURFACE,
        SURFACE,
        scale(hwnd, 3),
    );
    rounded(
        dc,
        dip_rect(hwnd, 48, 46, 10, 10),
        SURFACE,
        SURFACE,
        scale(hwnd, 3),
    );
    rounded(dc, dip_rect(hwnd, 39, 38, 14, 3), SURFACE, SURFACE, 0);
    rounded(dc, dip_rect(hwnd, 50, 39, 3, 12), SURFACE, SURFACE, 0);
    rounded(
        dc,
        dip_rect(hwnd, w - 228, 28, 204, 32),
        rgb(32, 52, 74),
        rgb(48, 69, 91),
        scale(hwnd, 16),
    );
    for (x, y, width, height) in [
        (24, 104, w - 48, 76),
        (24, 196, left, 254),
        (24, 466, left, 160),
        (right, 196, right_width, 190),
        (right, 402, right_width, 224),
    ] {
        rounded(
            dc,
            dip_rect(hwnd, x, y + 2, width, height),
            rgb(229, 235, 242),
            rgb(229, 235, 242),
            scale(hwnd, 16),
        );
        rounded(
            dc,
            dip_rect(hwnd, x, y, width, height),
            SURFACE,
            BORDER,
            scale(hwnd, 16),
        );
    }
    rounded(
        dc,
        dip_rect(hwnd, 24, 120, 4, 44),
        ACCENT,
        ACCENT,
        scale(hwnd, 3),
    );
    for (x, y, label, fill, color) in [
        (44, 210, "01", TEAL_TINT, ACCENT),
        (44, 480, "02", BLUE_TINT, BLUE),
        (right + 20, 210, "03", BLUE_TINT, BLUE),
    ] {
        let rect = dip_rect(hwnd, x, y, 28, 28);
        rounded(dc, rect, fill, fill, scale(hwnd, 8));
        draw_text(dc, label, rect, color, DT_CENTER);
    }
    rounded(
        dc,
        dip_rect(hwnd, 24, 670, w - 48, (h - 712).max(40)),
        LOG_BACKGROUND,
        LOG_BACKGROUND,
        scale(hwnd, 14),
    );
    SelectObject(dc, previous);
    EndPaint(hwnd, &ps);
}

unsafe fn dip_rect(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) -> RECT {
    RECT {
        left: scale(hwnd, x),
        top: vertical(hwnd, y),
        right: scale(hwnd, x + width),
        bottom: vertical(hwnd, y + height),
    }
}

pub(crate) unsafe fn draw_text(dc: HDC, text: &str, mut rect: RECT, color: COLORREF, align: u32) {
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let text = wide(text);
    DrawTextW(
        dc,
        text.as_ptr(),
        (text.len() - 1) as i32,
        &mut rect,
        align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

unsafe fn window_text(hwnd: HWND) -> String {
    let mut text = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
    let count = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
    String::from_utf16_lossy(&text[..count.max(0) as usize])
}

unsafe fn draw_result(item: &DRAWITEMSTRUCT) {
    let hwnd = item.hwndItem;
    let tone = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as COLORREF;
    let fill = if tone == SUCCESS {
        TEAL_TINT
    } else if tone == WARNING {
        rgb(255, 244, 235)
    } else {
        rgb(246, 248, 251)
    };
    rounded(item.hDC, item.rcItem, fill, fill, scale(hwnd, 10));
    let old_font = SelectObject(item.hDC, SendMessageW(hwnd, WM_GETFONT, 0, 0) as HGDIOBJ);
    let text = window_text(hwnd);
    let (summary, host) = text.split_once('\n').unwrap_or((&text, ""));
    let (name, status) = summary.split_once(" · ").unwrap_or((summary, ""));
    let mut rect = item.rcItem;
    rect.left += scale(hwnd, 12);
    rect.right -= scale(hwnd, 12);
    rect.top += scale(hwnd, 4);
    rect.bottom = rect.top + scale(hwnd, 20);
    let mut title = rect;
    title.right = title.left + scale(hwnd, 58);
    draw_text(item.hDC, name, title, INK, DT_LEFT);
    rect.left += scale(hwnd, 62);
    draw_text(item.hDC, status, rect, tone, DT_RIGHT);
    rect.left = item.rcItem.left + scale(hwnd, 12);
    rect.top += scale(hwnd, 20);
    rect.bottom += scale(hwnd, 20);
    draw_text(item.hDC, host, rect, MUTED, DT_LEFT);
    SelectObject(item.hDC, old_font);
}

unsafe fn draw_rule(item: &DRAWITEMSTRUCT) {
    if item.itemID == u32::MAX {
        return;
    }
    let hwnd = item.hwndItem;
    let length = SendMessageW(hwnd, LB_GETTEXTLEN, item.itemID as usize, 0);
    if length < 0 {
        return;
    }
    let mut text = vec![0u16; length as usize + 1];
    SendMessageW(
        hwnd,
        LB_GETTEXT,
        item.itemID as usize,
        text.as_mut_ptr() as isize,
    );
    let text = String::from_utf16_lossy(&text[..length as usize]);
    let (pattern, route) = text.rsplit_once('→').unwrap_or((&text, ""));
    let route = route.trim();
    let selected = item.itemState & ODS_SELECTED != 0;
    let fill = if selected {
        BLUE_TINT
    } else if item.itemID % 2 == 0 {
        rgb(248, 250, 252)
    } else {
        SURFACE
    };
    rounded(item.hDC, item.rcItem, fill, fill, 0);
    let old_font = SelectObject(item.hDC, SendMessageW(hwnd, WM_GETFONT, 0, 0) as HGDIOBJ);
    let mut label = item.rcItem;
    label.left += scale(hwnd, 10);
    label.right -= scale(hwnd, 122);
    draw_text(item.hDC, pattern.trim(), label, INK, DT_LEFT);
    let mut badge = item.rcItem;
    badge.left = badge.right - scale(hwnd, 112);
    badge.right -= scale(hwnd, 8);
    badge.top += scale(hwnd, 2);
    badge.bottom -= scale(hwnd, 2);
    let (color, tint) = match route {
        "LAN" => (BLUE, BLUE_TINT),
        "USB" => (ACCENT, TEAL_TINT),
        _ => (MUTED, BACKGROUND),
    };
    rounded(item.hDC, badge, tint, tint, scale(hwnd, 8));
    draw_text(item.hDC, route, badge, color, DT_CENTER);
    if item.itemState & ODS_FOCUS != 0 {
        let mut focus = item.rcItem;
        InflateRect(&mut focus, -1, -1);
        DrawFocusRect(item.hDC, &focus);
    }
    SelectObject(item.hDC, old_font);
}
unsafe fn combo_text(hwnd: HWND, index: usize) -> String {
    let length = SendMessageW(hwnd, CB_GETLBTEXTLEN, index, 0);
    if length < 0 {
        return String::new();
    }
    let mut text = vec![0u16; length as usize + 1];
    SendMessageW(hwnd, CB_GETLBTEXT, index, text.as_mut_ptr() as isize);
    String::from_utf16_lossy(&text[..length as usize])
}

unsafe fn field_frame(hwnd: HWND, dc: HDC, rect: RECT, focused: bool) {
    SetDCBrushColor(dc, SURFACE);
    FillRect(dc, &rect, GetStockObject(DC_BRUSH) as HBRUSH);
    let enabled = IsWindowEnabled(hwnd) != 0;
    let fill = if enabled { FIELD_FILL } else { BACKGROUND };
    let border = if focused && enabled {
        ACCENT
    } else if enabled {
        FIELD_BORDER
    } else {
        BORDER
    };
    rounded(dc, rect, fill, border, scale(hwnd, 12));
    if focused && enabled {
        let mut inner = rect;
        InflateRect(&mut inner, -scale(hwnd, 1), -scale(hwnd, 1));
        rounded(dc, inner, fill, border, scale(hwnd, 10));
    }
}

unsafe fn draw_combo(hwnd: HWND, dc: HDC) {
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let open = SendMessageW(hwnd, CB_GETDROPPEDSTATE, 0, 0) != 0;
    field_frame(hwnd, dc, rect, GetFocus() == hwnd || open);
    let old_font = SelectObject(dc, SendMessageW(hwnd, WM_GETFONT, 0, 0) as HGDIOBJ);
    let selected = SendMessageW(hwnd, CB_GETCURSEL, 0, 0);
    let text = if selected >= 0 {
        combo_text(hwnd, selected as usize)
    } else {
        String::new()
    };
    let mut label = rect;
    label.left += scale(hwnd, 12);
    label.right -= scale(hwnd, 32);
    draw_text(
        dc,
        &text,
        label,
        if IsWindowEnabled(hwnd) != 0 {
            INK
        } else {
            MUTED
        },
        DT_LEFT,
    );
    let x = rect.right - scale(hwnd, 18);
    let y = rect.bottom / 2;
    let direction = if open { -1 } else { 1 };
    let points = [
        POINT {
            x: x - scale(hwnd, 4),
            y: y - direction * scale(hwnd, 2),
        },
        POINT {
            x,
            y: y + direction * scale(hwnd, 2),
        },
        POINT {
            x: x + scale(hwnd, 4),
            y: y - direction * scale(hwnd, 2),
        },
    ];
    let pen = CreatePen(PS_SOLID, scale(hwnd, 2), MUTED);
    let old_pen = SelectObject(dc, pen as HGDIOBJ);
    Polyline(dc, points.as_ptr(), points.len() as i32);
    SelectObject(dc, old_pen);
    DeleteObject(pen as HGDIOBJ);
    SelectObject(dc, old_font);
}

pub(crate) unsafe fn draw_combo_item(item: &DRAWITEMSTRUCT) {
    // The closed field has its own renderer; Windows still owns popup and keyboard behavior.
    if item.itemState & ODS_COMBOBOXEDIT != 0 {
        return;
    }
    let selected = item.itemState & ODS_SELECTED != 0;
    let disabled = item.itemState & ODS_DISABLED != 0;
    SetDCBrushColor(item.hDC, if selected { TEAL_TINT } else { SURFACE });
    FillRect(item.hDC, &item.rcItem, GetStockObject(DC_BRUSH) as HBRUSH);
    if item.itemID == u32::MAX {
        return;
    }
    let font = SelectObject(
        item.hDC,
        SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HGDIOBJ,
    );
    let mut label = item.rcItem;
    label.left += scale(item.hwndItem, 12);
    label.right -= scale(item.hwndItem, 30);
    draw_text(
        item.hDC,
        &combo_text(item.hwndItem, item.itemID as usize),
        label,
        if disabled {
            MUTED
        } else if selected {
            ACCENT
        } else {
            INK
        },
        DT_LEFT,
    );
    if SendMessageW(item.hwndItem, CB_GETCURSEL, 0, 0) == item.itemID as isize {
        let mut mark = item.rcItem;
        mark.left = mark.right - scale(item.hwndItem, 28);
        draw_text(item.hDC, "✓", mark, ACCENT, DT_CENTER);
    }
    SelectObject(item.hDC, font);
}

unsafe fn draw_input_frame(hwnd: HWND, dc: HDC) {
    let saved = SaveDC(dc);
    let mut window: RECT = std::mem::zeroed();
    let mut client: RECT = std::mem::zeroed();
    let mut origin = POINT { x: 0, y: 0 };
    GetWindowRect(hwnd, &mut window);
    GetClientRect(hwnd, &mut client);
    ClientToScreen(hwnd, &mut origin);
    OffsetRect(&mut client, origin.x - window.left, origin.y - window.top);
    ExcludeClipRect(dc, client.left, client.top, client.right, client.bottom);
    OffsetRect(&mut window, -window.left, -window.top);
    field_frame(hwnd, dc, window, GetFocus() == hwnd);
    RestoreDC(dc, saved);
}

// Subclasses retain native selection, caret, clipboard, IME and keyboard navigation.
// They never access State: control messages can re-enter during a State operation.
pub(crate) unsafe extern "system" fn field_proc(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    input: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(field_proc), id);
        return DefSubclassProc(hwnd, message, w, l);
    }
    if input == 0 {
        match message {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                draw_combo(hwnd, dc);
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_PRINT | WM_PRINTCLIENT => {
                draw_combo(hwnd, w as HDC);
                return 0;
            }
            WM_ERASEBKGND => return 1,
            _ => {}
        }
    } else {
        match message {
            WM_NCCALCSIZE => {
                let rect = if w != 0 {
                    &mut (*(l as *mut NCCALCSIZE_PARAMS)).rgrc[0]
                } else {
                    &mut *(l as *mut RECT)
                };
                let dc = GetDC(hwnd);
                let font = SelectObject(dc, SendMessageW(hwnd, WM_GETFONT, 0, 0) as HGDIOBJ);
                let mut metrics: TEXTMETRICW = std::mem::zeroed();
                GetTextMetricsW(dc, &mut metrics);
                SelectObject(dc, font);
                ReleaseDC(hwnd, dc);
                let text_height = metrics.tmHeight + scale(hwnd, 2);
                let padding = ((rect.bottom - rect.top - text_height) / 2).max(1);
                InflateRect(rect, -scale(hwnd, 12), -padding);
                return 0;
            }
            WM_NCPAINT => {
                let dc = GetWindowDC(hwnd);
                draw_input_frame(hwnd, dc);
                ReleaseDC(hwnd, dc);
                return 0;
            }
            WM_PRINT => {
                let result = DefSubclassProc(hwnd, message, w, l);
                if l & PRF_NONCLIENT as isize != 0 {
                    draw_input_frame(hwnd, w as HDC);
                }
                return result;
            }
            _ => {}
        }
    }
    let result = DefSubclassProc(hwnd, message, w, l);
    if matches!(
        message,
        WM_SETFOCUS
            | WM_KILLFOCUS
            | WM_ENABLE
            | WM_KEYDOWN
            | WM_KEYUP
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | CB_SETCURSEL
            | CB_SHOWDROPDOWN
            | CB_RESETCONTENT
            | CB_ADDSTRING
    ) {
        RedrawWindow(
            hwnd,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_FRAME,
        );
    }
    result
}

pub(crate) unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    let primary = item.CtlID == START as u32 || item.CtlID == ADD as u32;
    let disabled = item.itemState & ODS_DISABLED != 0;
    let pressed = item.itemState & ODS_SELECTED != 0;
    let fill = if disabled {
        rgb(236, 241, 246)
    } else if primary {
        if pressed {
            rgb(11, 91, 85)
        } else {
            ACCENT
        }
    } else if item.CtlID == CHECK as u32 {
        if pressed {
            rgb(214, 228, 251)
        } else {
            BLUE_TINT
        }
    } else if pressed {
        rgb(225, 234, 249)
    } else {
        SURFACE
    };
    rounded(
        item.hDC,
        item.rcItem,
        fill,
        if primary && !disabled { fill } else { BORDER },
        scale(item.hwndItem, 10),
    );
    let color = if disabled {
        rgb(115, 131, 150)
    } else if primary {
        SURFACE
    } else if item.CtlID == CHECK as u32 {
        BLUE
    } else {
        INK
    };
    SetBkMode(item.hDC, TRANSPARENT as i32);
    SetTextColor(item.hDC, color);
    let font = SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HGDIOBJ;
    let previous = SelectObject(item.hDC, font);
    let mut text = [0u16; 128];
    let count = GetWindowTextW(item.hwndItem, text.as_mut_ptr(), text.len() as i32);
    let mut rect = item.rcItem;
    DrawTextW(
        item.hDC,
        text.as_ptr(),
        count,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(item.hDC, previous);
    if item.itemState & ODS_FOCUS != 0 {
        InflateRect(&mut rect, -4, -4);
        DrawFocusRect(item.hDC, &rect);
    }
}

impl State {
    unsafe fn add(&mut self, id: u16, class: &str, text: &str, style: u32, font: HFONT) {
        let hwnd = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            100,
            30,
            self.hwnd,
            id as usize as HMENU,
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        );
        SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
        self.controls.insert(id, hwnd);
    }
    unsafe fn create(&mut self) {
        let font = self.make_font(16, 400, "Segoe UI");
        let title = self.make_font(30, 600, "Segoe UI");
        let small = self.make_font(14, 400, "Segoe UI");
        let heading = self.make_font(17, 600, "Segoe UI");
        let mono = self.make_font(13, 400, "Consolas");
        for (id, text, style_font) in [
            (200, "NetBridge", title),
            (201, "Kết nối đúng mạng. Làm việc liền mạch.", small),
            (209, "WINDOWS   /   LAN + USB", small),
            (STATUS, "Phân luồng đang tắt", heading),
            (
                STATUS_DETAIL,
                "Chọn mạng bên dưới, sau đó bật phân luồng.",
                small,
            ),
            (205, "Thiết bị USB", heading),
            (210, "Chia sẻ Wi-Fi từ Android sang máy tính.", small),
            (USB_HINT, "Đang tìm thiết bị Android qua USB…", small),
            (206, "ADB · auto = dùng bản tích hợp", small),
            (211, "Đường mạng", heading),
            (202, "LAN · Hệ thống công ty", small),
            (203, "USB · Wi-Fi từ Android", small),
            (LAN_INFO, "Chưa chọn card mạng", small),
            (USB_INFO, "Chưa chọn card mạng", small),
            (212, "Kiểm tra kết nối", heading),
            (
                CHECK_SUMMARY,
                "Kiểm tra TCP của 4 đích theo quy tắc hiện tại.",
                small,
            ),
            (204, "Quy tắc phân luồng", heading),
            (207, "Nhật ký kết nối", heading),
            (213, "Giờ UTC · Chi tiết lỗi xuất hiện tại đây", small),
            (214, "Domain / IPv4 / CIDR", small),
            (215, "Đi qua mạng", small),
            (
                208,
                "Chỉ ứng dụng hỗ trợ proxy · IPv4 · HTTPS giữ nguyên kiểm tra TLS",
                small,
            ),
        ] {
            self.add(
                id,
                "STATIC",
                text,
                if id == 209 { SS_CENTER } else { 0 },
                style_font,
            );
            self.tone(
                id,
                if id == 200 {
                    SURFACE
                } else if matches!(id, 201 | 209) {
                    HEADER_TEXT
                } else if matches!(id, 204 | 205 | 207 | 211 | 212) {
                    INK
                } else {
                    MUTED
                },
            );
        }
        for (id, label) in [
            (START, "Bật phân luồng"),
            (STOP, "Tắt / Khôi phục"),
            (REFRESH, "Làm mới"),
            (REFRESH_USB, "Tìm thiết bị"),
            (USB_ON, "Kết nối Ethernet"),
            (USB_OFF, "Ngắt Ethernet"),
            (CHECK, "Kiểm tra 4 đích"),
            (ADD, "Lưu"),
            (REMOVE, "Bỏ"),
            (PAUSE_LOG, "Tạm dừng log"),
            (TOOLBOX, "ADB & tốc độ mạng"),
        ] {
            self.add(id, "BUTTON", label, WS_TABSTOP | BS_OWNERDRAW as u32, font);
        }
        for id in [DEVICES, LAN, USB, ROUTE] {
            self.add(
                id,
                "COMBOBOX",
                "",
                WS_TABSTOP
                    | WS_VSCROLL
                    | (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32,
                small,
            );
            SendMessageW(
                self.control(id),
                CB_SETITEMHEIGHT,
                usize::MAX,
                scale(self.hwnd, 28) as isize,
            );
            SendMessageW(
                self.control(id),
                CB_SETDROPPEDWIDTH,
                scale(self.hwnd, if id == ROUTE { 190 } else { 380 }) as usize,
                0,
            );
            SendMessageW(
                self.control(id),
                CB_SETITEMHEIGHT,
                0,
                scale(self.hwnd, 32) as isize,
            );
            SetWindowSubclass(self.control(id), Some(field_proc), 1, 0);
        }
        for (id, style) in [
            (ADB, ES_AUTOHSCROLL),
            (PATTERN, ES_AUTOHSCROLL),
            (LOGS, ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY),
        ] {
            self.add(
                id,
                "EDIT",
                "",
                WS_TABSTOP | style as u32 | if id == LOGS { WS_VSCROLL } else { 0 },
                if id == LOGS { mono } else { small },
            );
            SendMessageW(
                self.control(id),
                EM_SETMARGINS,
                (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
                if id == LOGS {
                    (scale(self.hwnd, 8) | (scale(self.hwnd, 8) << 16)) as isize
                } else {
                    0
                },
            );
            if id != LOGS {
                SetWindowSubclass(self.control(id), Some(field_proc), 1, 1);
                SetWindowPos(
                    self.control(id),
                    ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        }
        SendMessageW(
            self.control(PATTERN),
            EM_SETCUEBANNER,
            0,
            wide("Domain, *.domain hoặc IPv4/CIDR").as_ptr() as isize,
        );
        self.tone(LOGS, LOG_TEXT);
        self.add(
            RULES,
            "LISTBOX",
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT) as u32,
            small,
        );
        SendMessageW(
            self.control(RULES),
            LB_SETITEMHEIGHT,
            0,
            scale(self.hwnd, 24) as isize,
        );
        for label in ["LAN", "USB", "Theo Windows"] {
            SendMessageW(
                self.control(ROUTE),
                CB_ADDSTRING,
                0,
                wide(label).as_ptr() as isize,
            );
        }
        SendMessageW(self.control(ROUTE), CB_SETCURSEL, 0, 0);
        for (i, (host, port, name)) in CHECK_TARGETS.iter().enumerate() {
            self.add(
                CHECK_RESULT + i as u16,
                "STATIC",
                &format!("{name} · Chưa kiểm tra\n{host}:{port}"),
                SS_OWNERDRAW,
                small,
            );
            self.tone(CHECK_RESULT + i as u16, MUTED);
        }
        // Keep keyboard navigation in the same order as the visible workflow.
        let order = [
            TOOLBOX,
            START,
            STOP,
            DEVICES,
            REFRESH_USB,
            USB_ON,
            USB_OFF,
            ADB,
            REFRESH,
            LAN,
            USB,
            CHECK,
            RULES,
            PATTERN,
            ROUTE,
            ADD,
            REMOVE,
            PAUSE_LOG,
            LOGS,
        ];
        let mut previous = HWND_TOP;
        for id in order {
            let control = self.control(id);
            SetWindowPos(
                control,
                previous,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            previous = control;
        }
        EnableWindow(self.control(STOP), 0);
        EnableWindow(self.control(USB_ON), 0);
        EnableWindow(self.control(USB_OFF), 0);
        EnableWindow(self.control(REMOVE), 0);
        self.set(
            ADB,
            &self
                .config
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .adb_path,
        );
        self.fill_rules();
        self.refresh();
        self.refresh_usb();
        SetTimer(self.hwnd, 1, 400, None);
        proxy::log(
            &self.logs,
            "Sẵn sàng. Chọn LAN và USB → kiểm tra kết nối → bật phân luồng.",
        );
        self.layout();
    }
    unsafe fn make_font(&mut self, size: i32, weight: i32, name: &str) -> HFONT {
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
            wide(name).as_ptr(),
        );
        self.fonts.push(font);
        font
    }
    unsafe fn layout(&self) {
        let (w, h, left, right, right_width) = geometry(self.hwnd);
        let half = (left - 52) / 2;
        let result_width = (right_width - 52) / 2;
        let entries = [
            (200, 84, 16, 300, 38),
            (201, 84, 56, w - 340, 22),
            (209, w - 222, 36, 192, 20),
            (TOOLBOX, w - 450, 28, 206, 32),
            (STATUS, 48, 116, w - 436, 26),
            (STATUS_DETAIL, 48, 146, w - 436, 22),
            (START, w - 374, 122, 160, 40),
            (STOP, w - 202, 122, 158, 40),
            (205, 84, 212, left - 80, 26),
            (210, 44, 244, left - 40, 22),
            (DEVICES, 44, 272, left - 182, 260),
            (REFRESH_USB, left - 126, 272, 130, 32),
            (USB_ON, 44, 314, 170, 34),
            (USB_OFF, 224, 314, 154, 34),
            (USB_HINT, 44, 358, left - 40, 32),
            (206, 44, 390, left - 40, 20),
            (ADB, 44, 410, left - 40, 34),
            (211, 84, 482, left - 190, 26),
            (REFRESH, left - 96, 480, 100, 30),
            (202, 44, 520, half, 20),
            (203, 56 + half, 520, half, 20),
            (LAN, 44, 544, half, 240),
            (USB, 56 + half, 544, half, 240),
            (LAN_INFO, 44, 582, half, 34),
            (USB_INFO, 56 + half, 582, half, 34),
            (212, right + 60, 212, right_width - 226, 26),
            (CHECK, right + right_width - 158, 210, 138, 32),
            (CHECK_SUMMARY, right + 20, 244, right_width - 40, 22),
            (CHECK_RESULT, right + 20, 274, result_width, 46),
            (
                CHECK_RESULT + 1,
                right + 32 + result_width,
                274,
                result_width,
                46,
            ),
            (CHECK_RESULT + 2, right + 20, 328, result_width, 46),
            (
                CHECK_RESULT + 3,
                right + 32 + result_width,
                328,
                result_width,
                46,
            ),
            (204, right + 20, 416, right_width - 40, 26),
            (RULES, right + 20, 450, right_width - 40, 96),
            (214, right + 20, 554, right_width - 300, 20),
            (215, right + right_width - 268, 554, 100, 20),
            (PATTERN, right + 20, 576, right_width - 300, 34),
            (ROUTE, right + right_width - 268, 576, 100, 180),
            (ADD, right + right_width - 158, 576, 74, 32),
            (REMOVE, right + right_width - 76, 576, 56, 32),
            (207, 24, 642, 220, 24),
            (213, w - 510, 648, 336, 20),
            (PAUSE_LOG, w - 160, 640, 136, 30),
            (LOGS, 36, 682, w - 72, (h - 736).max(24)),
            (208, 24, h - 28, w - 48, 22),
        ];
        for (id, x, y, width, height) in entries {
            if matches!(id, DEVICES | LAN | USB | ROUTE) {
                SendMessageW(
                    self.control(id),
                    CB_SETITEMHEIGHT,
                    usize::MAX,
                    (vertical(self.hwnd, 34) - scale(self.hwnd, 6)).max(scale(self.hwnd, 22))
                        as isize,
                );
            }
            MoveWindow(
                self.control(id),
                scale(self.hwnd, x),
                vertical(self.hwnd, y),
                scale(self.hwnd, width.max(30)),
                vertical(self.hwnd, height),
                0,
            );
        }
        SendMessageW(
            self.control(RULES),
            LB_SETITEMHEIGHT,
            0,
            (vertical(self.hwnd, 96) / 4) as isize,
        );
        RedrawWindow(
            self.hwnd,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE,
        );
    }
    fn action(&mut self, id: u16, notification: u16) {
        let result = match id {
            TOOLBOX => (|| {
                self.save()?;
                let mut command = std::process::Command::new(std::env::current_exe()?);
                command.arg("--tools").arg("--config").arg(&self.path);
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
                command.spawn()?;
                Ok(())
            })(),
            START => self.start(),
            STOP => self.stop(),
            REFRESH => {
                self.refresh();
                Ok(())
            }
            REFRESH_USB => {
                self.refresh_usb();
                Ok(())
            }
            ADD => self.add_rule(),
            REMOVE => self.remove_rule(),
            USB_ON => self.tether(true),
            USB_OFF => self.tether(false),
            CHECK => self.check(),
            PAUSE_LOG => {
                self.log_paused = !self.log_paused;
                self.set(
                    PAUSE_LOG,
                    if self.log_paused {
                        "Tiếp tục log"
                    } else {
                        "Tạm dừng log"
                    },
                );
                self.set(
                    213,
                    if self.log_paused {
                        "Đã dừng cuộn · Kết nối vẫn hoạt động"
                    } else {
                        "Giờ UTC · Chi tiết lỗi xuất hiện tại đây"
                    },
                );
                Ok(())
            }
            LAN | USB if notification == CBN_SELCHANGE as u16 => self.save().map(|_| ()),
            RULES if notification == LBN_SELCHANGE as u16 => {
                let i = unsafe { SendMessageW(self.control(RULES), LB_GETCURSEL, 0, 0) };
                let cfg = self.config.read().unwrap_or_else(|e| e.into_inner());
                if let Some(rule) = cfg.rules.get(i as usize) {
                    self.set(PATTERN, &rule.pattern);
                    let index = match rule.route {
                        Route::Lan => 0,
                        Route::Usb => 1,
                        Route::System => 2,
                    };
                    unsafe {
                        SendMessageW(self.control(ROUTE), CB_SETCURSEL, index, 0);
                    }
                }
                Ok(())
            }
            DEVICES if notification == CBN_SELCHANGE as u16 => {
                self.update_device_hint();
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.error(e)
        }
    }
}

thread_local! { static HANDLING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
struct HandlerGuard;
impl Drop for HandlerGuard {
    fn drop(&mut self) {
        HANDLING.with(|value| value.set(false));
    }
}
const REFLOW: u32 = WM_APP + 1;

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        let state = create.lpCreateParams as *mut State;
        (*state).hwnd = hwnd;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
    }
    // Painting and edit notifications can synchronously re-enter this callback. Do not
    // create a second mutable State reference while another handler is calling Win32.
    match message {
        WM_GETMINMAXINFO => {
            (*(l as *mut MINMAXINFO)).ptMinTrackSize = POINT {
                x: scale(hwnd, 1000),
                y: scale(hwnd, 740),
            };
            return 0;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let child = l as HWND;
            let id = GetDlgCtrlID(child) as u16;
            let background = if matches!(id, 200 | 201) {
                NAVY
            } else if id == 209 {
                rgb(32, 52, 74)
            } else if matches!(id, ADB | PATTERN) {
                if IsWindowEnabled(child) != 0 {
                    FIELD_FILL
                } else {
                    BACKGROUND
                }
            } else if id == LOGS {
                LOG_BACKGROUND
            } else if matches!(id, 207 | 208 | 213) {
                BACKGROUND
            } else {
                SURFACE
            };
            let tone = GetWindowLongPtrW(child, GWLP_USERDATA) as COLORREF;
            SetBkColor(w as HDC, background);
            SetTextColor(w as HDC, if tone == 0 { INK } else { tone });
            SetDCBrushColor(w as HDC, background);
            return GetStockObject(DC_BRUSH) as isize;
        }
        WM_PAINT => {
            paint(hwnd);
            return 0;
        }
        WM_DRAWITEM => {
            let item = &*(l as *const DRAWITEMSTRUCT);
            if item.CtlType == ODT_BUTTON {
                draw_button(item);
                return 1;
            }
            if item.CtlType == ODT_STATIC
                && (CHECK_RESULT as u32..CHECK_RESULT as u32 + 4).contains(&item.CtlID)
            {
                draw_result(item);
                return 1;
            }
            if item.CtlType == ODT_LISTBOX && item.CtlID == RULES as u32 {
                draw_rule(item);
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
            if item.CtlType == ODT_LISTBOX && item.CtlID == RULES as u32 {
                item.itemHeight = scale(hwnd, 24) as u32;
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
            let notification = ((w >> 16) & 0xffff) as u16;
            let button = matches!(
                id,
                START
                    | STOP
                    | REFRESH
                    | REFRESH_USB
                    | ADD
                    | REMOVE
                    | USB_ON
                    | USB_OFF
                    | CHECK
                    | PAUSE_LOG
                    | TOOLBOX
            ) && notification == BN_CLICKED as u16;
            let selection = matches!(id, LAN | USB | DEVICES)
                && notification == CBN_SELCHANGE as u16
                || id == RULES && notification == LBN_SELCHANGE as u16;
            if !button && !selection {
                return 0;
            }
        }
        WM_CREATE | WM_SIZE | WM_TIMER | WM_CLOSE | REFLOW => {}
        _ => return DefWindowProcW(hwnd, message, w, l),
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, message, w, l);
    }
    if HANDLING.with(|value| value.replace(true)) {
        if message == WM_SIZE {
            PostMessageW(hwnd, REFLOW, 0, 0);
        }
        return 0;
    }
    let _guard = HandlerGuard;
    let state = &mut *ptr;
    match message {
        WM_CREATE => {
            state.create();
            0
        }
        WM_SIZE | REFLOW => {
            state.repaint_ticks = 2;
            state.layout();
            0
        }
        WM_TIMER => {
            state.tick();
            0
        }
        WM_COMMAND => {
            state.action((w & 0xffff) as u16, ((w >> 16) & 0xffff) as u16);
            0
        }
        WM_CLOSE => {
            match state.stop() {
                Ok(()) => {
                    DestroyWindow(hwnd);
                }
                Err(e) => state.error(e),
            }
            0
        }
        _ => DefWindowProcW(hwnd, message, w, l),
    }
}

fn show_message(parent: HWND, title: &str, message: &str, style: u32) {
    unsafe {
        MessageBoxW(
            parent,
            wide(message).as_ptr(),
            wide(title).as_ptr(),
            MB_OK | style,
        );
    }
}
pub fn show_error(message: &str) {
    show_message(ptr::null_mut(), "NetBridge", message, MB_ICONERROR);
}

pub fn run(config: Config, path: PathBuf) -> io::Result<()> {
    unsafe {
        let mutex = CreateMutexW(ptr::null(), 0, wide("Local\\NetBridge.GUI").as_ptr());
        if mutex.is_null() {
            return Err(io::Error::last_os_error());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(mutex);
            show_message(
                ptr::null_mut(),
                "NetBridge",
                "NetBridge đang mở. Kiểm tra cửa sổ hiện có.",
                MB_ICONINFORMATION,
            );
            return Ok(());
        }
        let mut state = Box::new(State::new(config, path));
        match system_proxy::recover(&state.backup) {
            Ok(true) => proxy::log(&state.logs, "Đã khôi phục proxy từ phiên trước."),
            Err(e) => proxy::log(&state.logs, format!("Khôi phục trước: {e}")),
            _ => {}
        }
        SetProcessDPIAware();
        let class = wide("NetBridgeMainWindow");
        let instance = GetModuleHandleW(ptr::null());
        state.brush = CreateSolidBrush(BACKGROUND);
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: state.brush,
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            CloseHandle(mutex);
            return Err(io::Error::last_os_error());
        }
        let mut work: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
        let width = 1160.min((work.right - work.left - 32).max(1000));
        let height = 840.min((work.bottom - work.top - 32).max(740));
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("NetBridge · LAN + USB").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            work.left + ((work.right - work.left - width) / 2).max(0),
            work.top + ((work.bottom - work.top - height) / 2).max(0),
            width,
            height,
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
        RedrawWindow(
            hwnd,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE | RDW_UPDATENOW,
        );
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
        CloseHandle(mutex);
    }
    Ok(())
}

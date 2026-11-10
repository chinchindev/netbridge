use netbridge::{
    adapters::{self, Adapter},
    config::Config,
    policy::Route,
    proxy::{self, Log, Server, SharedConfig},
    system_proxy,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub kind: String,
    pub owned: bool,
    pub label: String,
    pub detail: String,
    pub endpoint: String,
}
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub kind: String,
    pub phase: String,
    pub active: bool,
    pub cancelling: bool,
    pub error: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetResult {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub route: Route,
    pub ok: bool,
    pub detail: String,
    pub checked_at: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedResult {
    pub adapter: String,
    pub source_ip: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub latency_ms: Option<f64>,
    pub grade: String,
    pub explanation: String,
    pub errors: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub mode: String,
    pub version: String,
    pub config: Config,
    pub config_revision: String,
    pub config_changed: bool,
    pub exit_allowed: bool,
    pub adapters: Vec<Adapter>,
    pub session: SessionView,
    pub job: Job,
    pub targets: Vec<TargetResult>,
    pub speeds: Vec<SpeedResult>,
    pub logs: Vec<String>,
    pub unit: netbridge::speed_view::SpeedUnit,
    pub adb_latest: Option<netbridge::adb_manager::Release>,
}
pub struct Session {
    pub preview: bool,
    pub config: SharedConfig,
    pub path: PathBuf,
    pub backup: PathBuf,
    pub logs: Log,
    server: Option<Server>,
    proxy_enabled: bool,
    watchdog_started: bool,
    disk_revision: String,
}
fn profile_revision(path: &std::path::Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() <= 128 * 1024 => Ok(format!("{:x}", Sha256::digest(bytes))),
        Ok(_) => Err("Cấu hình lớn hơn giới hạn 128 KB.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("absent".into()),
        Err(e) => Err(e.to_string()),
    }
}
fn read_profile(path: &std::path::Path) -> Result<(Config, String), String> {
    let revision = profile_revision(path)?;
    let cfg = Config::load(path).map_err(|e| e.to_string())?;
    if profile_revision(path)? != revision {
        return Err("Cấu hình đang thay đổi. Thử tải lại.".into());
    }
    Ok((cfg, revision))
}
impl Session {
    pub fn revision(&self) -> String {
        self.disk_revision.clone()
    }
    pub fn save_expected(&mut self, cfg: Config, expected: &str) -> Result<(), String> {
        self.ensure_live()?;
        if expected != self.disk_revision {
            return Err("Bản nháp dựa trên cấu hình cũ. Tải lại cấu hình trước khi lưu.".into());
        }
        self.save(cfg)
    }
    pub fn changed_on_disk(&self) -> bool {
        !self.preview && profile_revision(&self.path).map_or(true, |rev| rev != self.disk_revision)
    }
    pub fn reload(&mut self) -> Result<(), String> {
        self.ensure_live()?;
        if self.owned() {
            return Err("Tắt phiên trước khi tải lại cấu hình được sửa từ bên ngoài.".into());
        }
        let (mut cfg, revision) = read_profile(&self.path)?;
        cfg.auto_select(&adapters::enumerate().unwrap_or_default());
        *self.config.write().map_err(|_| "Config lock")? = cfg;
        self.disk_revision = revision;
        Ok(())
    }
    pub fn new(preview: bool, path: PathBuf) -> Result<Self, String> {
        let (mut config, disk_revision) = if preview {
            (Config::default(), "preview".into())
        } else {
            read_profile(&path)?
        };
        config.auto_select(&adapters::enumerate().unwrap_or_default());
        Ok(Self {
            preview,
            config: Arc::new(RwLock::new(config)),
            path,
            backup: netbridge::config::data_dir().join("proxy-backup.json"),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            server: None,
            proxy_enabled: false,
            watchdog_started: false,
            disk_revision,
        })
    }
    pub fn ensure_live(&self) -> Result<(), String> {
        if self.preview {
            Err("Chế độ xem thử không được thay đổi cấu hình hoặc kết nối mạng.".into())
        } else {
            Ok(())
        }
    }
    pub fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn view(&self) -> SessionView {
        let cfg = self.config();
        if self.preview {
            return SessionView {
                kind: "inactive".into(),
                owned: false,
                label: "Chế độ xem thử".into(),
                detail: "Không thay đổi proxy Windows hoặc thiết bị.".into(),
                endpoint: format!("127.0.0.1:{}", cfg.port),
            };
        }
        let current = system_proxy::read().ok();
        let expected = format!("127.0.0.1:{}", cfg.port);
        let applied = current.as_ref().is_some_and(|v| {
            v.flags == 3
                && v.server == expected
                && v.bypass == "localhost;127.0.0.1;[::1]"
                && v.pac_url.is_empty()
        });
        let (kind, label, detail) = if self.proxy_enabled && self.server.is_some() && applied {
            let adapters = adapters::enumerate().unwrap_or_default();
            let missing = cfg.rules.iter().any(|r| match r.route {
                Route::Lan => !adapters.iter().any(|a| a.id == cfg.lan_id),
                Route::Usb => !adapters.iter().any(|a| a.id == cfg.usb_id),
                Route::System => false,
            });
            if missing {
                (
                    "degraded",
                    "Thiếu một đường mạng",
                    "Phiên vẫn chạy; các đích qua mạng bị mất sẽ báo lỗi, không tự đổi đường.",
                )
            } else {
                ("active","Đang phân luồng","Proxy Windows đã được xác minh. Đóng cửa sổ sẽ giữ phiên chạy ở khay hệ thống.")
            }
        } else if self.proxy_enabled && self.server.is_some() {
            ("conflict","Proxy đã thay đổi ngoài NetBridge","Phiên vẫn giữ listener. Tắt phiên để bỏ quyền sở hữu; không ghi đè cấu hình bên ngoài.")
        } else if current
            .as_ref()
            .is_some_and(|settings| !can_release_listener(settings, cfg.port))
        {
            (
                "external",
                "Một phiên khác đang phân luồng",
                "Không chiếm phiên hiện tại. Dùng cửa sổ đang giữ phiên hoặc tắt phiên đó trước.",
            )
        } else if current.is_none() {
            (
                "unknown",
                "Chưa đọc được trạng thái Windows",
                "Chưa thể xác minh proxy. Kiểm tra chẩn đoán trước khi thay đổi.",
            )
        } else {
            (
                "inactive",
                "Đang dùng mạng Windows",
                "Bật phân luồng để chọn đường mạng theo từng đích.",
            )
        };
        SessionView {
            kind: kind.into(),
            owned: self.proxy_enabled,
            label: label.into(),
            detail: detail.into(),
            endpoint: expected,
        }
    }
    pub fn save(&mut self, cfg: Config) -> Result<(), String> {
        self.ensure_live()?;
        cfg.validate().map_err(|e| e.to_string())?;
        if self.changed_on_disk() {
            return Err(
                "File cấu hình đã thay đổi ngoài ứng dụng. Tải cấu hình mới trước khi lưu.".into(),
            );
        }
        if serde_json::to_vec_pretty(&cfg)
            .map_err(|e| e.to_string())?
            .len()
            > 128 * 1024
        {
            return Err("Cấu hình lớn hơn giới hạn 128 KB.".into());
        }
        if matches!(self.view().kind.as_str(), "external" | "unknown") {
            return Err("Profile đang thuộc phiên khác; không ghi đè cấu hình.".into());
        }
        if self.server.is_some() && cfg.port != self.config().port {
            return Err("Tắt phiên trước khi thay đổi cổng proxy.".into());
        }
        cfg.save(&self.path).map_err(|e| e.to_string())?;
        self.disk_revision = profile_revision(&self.path)?;
        *self
            .config
            .write()
            .map_err(|_| "Không khóa được cấu hình")? = cfg;
        proxy::log(
            &self.logs,
            "Đã lưu cấu hình. Quy tắc áp dụng cho kết nối mới.",
        );
        Ok(())
    }
    pub fn start(&mut self) -> Result<(), String> {
        self.ensure_live()?;
        if self.changed_on_disk() {
            return Err("Cấu hình đã thay đổi ngoài ứng dụng. Tải lại trước khi bật phiên.".into());
        }
        if self.server.is_some() {
            return Err("Phiên đã chạy; không tạo phiên trùng.".into());
        }
        let cfg = self.config();
        cfg.validate().map_err(|e| e.to_string())?;
        let available = adapters::enumerate().map_err(|e| e.to_string())?;
        for rule in &cfg.rules {
            let id = match rule.route {
                Route::Lan => &cfg.lan_id,
                Route::Usb => &cfg.usb_id,
                Route::System => continue,
            };
            if !available.iter().any(|a| &a.id == id) {
                return Err(format!("Chưa có adapter hoạt động cho {:?}.", rule.route));
            }
        }
        let server =
            Server::start(self.config.clone(), self.logs.clone()).map_err(|e| e.to_string())?;
        if !self.watchdog_started {
            system_proxy::spawn_watchdog(&self.backup).map_err(|e| e.to_string())?;
            self.watchdog_started = true;
        }
        if let Err(error) = system_proxy::enable(&self.backup, cfg.port) {
            if system_proxy::read().map_or(true, |s| {
                s.flags & 2 != 0 && s.server == format!("127.0.0.1:{}", cfg.port)
            }) {
                self.server = Some(server);
                self.proxy_enabled = true;
            }
            return Err(error.to_string());
        }
        self.server = Some(server);
        self.proxy_enabled = true;
        proxy::log(
            &self.logs,
            "Đã bật phân luồng với quyền sở hữu phiên đã xác minh.",
        );
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.ensure_live()?;
        if self.proxy_enabled {
            system_proxy::restore(&self.backup).map_err(|e| e.to_string())?;
            let current = system_proxy::read().map_err(|e| {
                format!("Chưa xác minh được proxy; giữ listener để bảo toàn kết nối: {e}")
            })?;
            if !can_release_listener(&current, self.config().port) {
                return Err("Windows vẫn trỏ tới listener NetBridge sau phục hồi. Giữ phiên để tránh mất kết nối; kiểm tra cấu hình proxy Windows.".into());
            }
            self.proxy_enabled = false;
        }
        self.server.take();
        proxy::log(
            &self.logs,
            "Đã tắt phiên. Windows dùng cấu hình proxy được bảo toàn.",
        );
        Ok(())
    }
    pub fn owned(&self) -> bool {
        self.proxy_enabled
    }
}
pub struct AppState {
    pub session: Mutex<Session>,
    pub job: Mutex<Job>,
    pub targets: Mutex<Vec<TargetResult>>,
    pub speeds: Mutex<Vec<SpeedResult>>,
    pub cancel: Mutex<Arc<AtomicBool>>,
    pub serial: AtomicU64,
    pub quit_allowed: AtomicBool,
    shutting_down: AtomicBool,
    pub adb_latest: Mutex<Option<netbridge::adb_manager::Release>>,
}
fn can_release_listener(settings: &system_proxy::Settings, port: u16) -> bool {
    if settings.flags & 2 == 0 {
        return true;
    }
    let endpoints = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    !settings.server.split(';').any(|entry| {
        let target = entry
            .split_once('=')
            .map_or(entry, |(_, value)| value)
            .trim();
        endpoints
            .iter()
            .any(|endpoint| target.eq_ignore_ascii_case(endpoint))
    })
}
impl AppState {
    pub fn begin_shutdown(&self) -> Result<(), String> {
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active && matches!(job.kind.as_str(), "adb" | "usb" | "app-update") {
            return Err(
                "Chờ tác vụ ADB/USB hoặc cập nhật ứng dụng hoàn tất trước khi thoát.".into(),
            );
        }
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return Err("Ứng dụng đang thoát.".into());
        }
        self.cancel
            .lock()
            .map_err(|_| "Cancel lock")?
            .store(true, Ordering::Relaxed);
        Ok(())
    }
    pub fn abort_shutdown(&self) {
        self.shutting_down.store(false, Ordering::SeqCst);
    }
    pub fn prepare_update_install(&self, id: u64) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| "Session lock")?;
        session.ensure_live()?;
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if !job.active || job.id != id || job.kind != "app-update" {
            return Err("Tác vụ cập nhật không còn hợp lệ.".into());
        }
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return Err("Ứng dụng đang thoát.".into());
        }
        if let Err(error) = session.stop() {
            self.abort_shutdown();
            return Err(error);
        }
        Ok(())
    }
    pub fn save_profile(&self, cfg: Config, revision: &str) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| "Session lock")?;
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active || self.shutting_down.load(Ordering::SeqCst) {
            return Err("Chờ tác vụ hiện tại kết thúc trước khi lưu cấu hình.".into());
        }
        session.save_expected(cfg, revision)?;
        self.targets.lock().map_err(|_| "Target lock")?.clear();
        Ok(())
    }
    pub fn reload_profile(&self) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| "Session lock")?;
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active || self.shutting_down.load(Ordering::SeqCst) {
            return Err("Chờ tác vụ hiện tại kết thúc trước khi tải cấu hình.".into());
        }
        session.reload()?;
        self.targets.lock().map_err(|_| "Target lock")?.clear();
        Ok(())
    }
    pub fn start_session(&self) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| "Session lock")?;
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active || self.shutting_down.load(Ordering::SeqCst) {
            return Err("Chờ tác vụ hiện tại kết thúc trước khi bật phiên.".into());
        }
        session.start()
    }
    pub fn can_exit(&self) -> Result<(), String> {
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active && matches!(job.kind.as_str(), "adb" | "usb" | "app-update") {
            return Err(
                "Chờ tác vụ ADB/USB hoặc cập nhật ứng dụng hoàn tất trước khi thoát.".into(),
            );
        }
        Ok(())
    }
    pub fn hide_on_close(&self) -> bool {
        self.session
            .try_lock()
            .map_or(true, |session| session.owned())
            || self.job.try_lock().map_or(true, |job| job.active)
    }
    pub fn new(session: Session) -> Self {
        Self {
            session: Mutex::new(session),
            job: Mutex::new(Job::default()),
            targets: Mutex::new(vec![]),
            speeds: Mutex::new(vec![]),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            serial: AtomicU64::new(0),
            quit_allowed: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
            adb_latest: Mutex::new(None),
        }
    }
    pub fn begin_job(&self, kind: &str) -> Result<(u64, Arc<AtomicBool>), String> {
        self.session
            .lock()
            .map_err(|_| "Session lock")?
            .ensure_live()?;
        let mut job = self.job.lock().map_err(|_| "Job lock")?;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("Ứng dụng đang thoát; không nhận tác vụ mới.".into());
        }
        if job.active {
            return Err("Có tác vụ đang chạy. Chờ hoàn tất hoặc bấm Dừng.".into());
        }
        let id = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().map_err(|_| "Cancel lock")? = cancel.clone();
        *job = Job {
            id,
            kind: kind.into(),
            phase: "Đang chuẩn bị…".into(),
            active: true,
            cancelling: false,
            error: None,
        };
        Ok((id, cancel))
    }
    pub fn progress(&self, id: u64, phase: String) {
        if let Ok(mut job) = self.job.lock() {
            if job.id == id && !job.cancelling {
                job.phase = phase;
            }
        }
    }
    pub fn finish(&self, id: u64, result: Result<(), String>) {
        if let Ok(mut job) = self.job.lock() {
            if job.id == id {
                job.active = false;
                job.error = result.err();
                job.phase = if job.cancelling {
                    "Đã dừng."
                } else if job.error.is_some() {
                    "Tác vụ chưa hoàn tất."
                } else {
                    "Đã hoàn tất."
                }
                .into();
            }
        }
    }
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let session = self.session.lock().map_err(|_| "Session lock")?;
        let unit = netbridge::speed_view::Preferences::load(
            &netbridge::config::data_dir().join("toolbox-preferences.json"),
        )
        .unwrap_or_default()
        .unit;
        let logs = session
            .logs
            .lock()
            .map_err(|_| "Log lock")?
            .iter()
            .cloned()
            .collect();
        let exit_allowed = self.can_exit().is_ok();
        Ok(Snapshot {
            mode: if session.preview { "preview" } else { "live" }.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            config: session.config(),
            config_revision: session.revision(),
            config_changed: session.changed_on_disk(),
            exit_allowed,
            adapters: adapters::enumerate().unwrap_or_default(),
            session: session.view(),
            job: self.job.lock().map_err(|_| "Job lock")?.clone(),
            targets: self.targets.lock().map_err(|_| "Target lock")?.clone(),
            speeds: self.speeds.lock().map_err(|_| "Speed lock")?.clone(),
            logs,
            unit,
            adb_latest: self.adb_latest.lock().map_err(|_| "ADB lock")?.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/qa-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("config.json")
    }
    #[test]
    fn preview_rejects_all_session_mutations_before_any_write() {
        let path = temp();
        let mut session = Session::new(true, path.clone()).unwrap();
        assert!(session.start().is_err());
        assert!(session.stop().is_err());
        assert!(session.save(Config::default()).is_err());
        assert!(!path.exists());
        let state = AppState::new(session);
        assert!(state.begin_job("speed").is_err());
    }
    #[test]
    fn invalid_draft_does_not_change_the_saved_profile() {
        let path = temp();
        let original = Config::default();
        original.save(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut session = Session::new(false, path.clone()).unwrap();
        let mut draft = original;
        draft.rules[0].pattern = "not a valid host".into();
        assert!(session.save(draft).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn stale_draft_revision_cannot_overwrite_a_saved_profile() {
        let path = temp();
        Config::default().save(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut session = Session::new(false, path.clone()).unwrap();
        let mut draft = session.config();
        draft.rules[0].pattern = "changed.invalid".into();
        assert!(session.save_expected(draft, "stale-revision").is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn externally_changed_profile_is_preserved_when_saving_a_draft() {
        let path = temp();
        Config::default().save(&path).unwrap();
        let mut session = Session::new(false, path.clone()).unwrap();
        let revision = session.revision();
        let draft = session.config();
        let mut external = Config::default();
        external.rules[0].pattern = "external.invalid".into();
        external.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(session.save_expected(draft, &revision).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn saved_profile_gets_a_new_revision() {
        let path = temp();
        let mut session = Session::new(false, path).unwrap();
        let before = session.revision();
        let mut draft = session.config();
        draft.rules[0].pattern = "saved.invalid".into();
        session.save_expected(draft, &before).unwrap();
        assert_ne!(session.revision(), before);
    }
    #[test]
    fn listener_is_retained_while_any_proxy_entry_still_references_it() {
        for server in [
            "127.0.0.1:17890",
            "http=127.0.0.1:17890;https=other:443",
            "localhost:17890",
        ] {
            let settings = system_proxy::Settings {
                flags: 3,
                server: server.into(),
                bypass: "changed".into(),
                pac_url: String::new(),
            };
            assert!(!can_release_listener(&settings, 17890));
        }
        let settings = system_proxy::Settings {
            flags: 1,
            server: "127.0.0.1:17890".into(),
            bypass: String::new(),
            pac_url: String::new(),
        };
        assert!(can_release_listener(&settings, 17890));
    }
    #[test]
    fn stale_job_progress_cannot_overwrite_a_newer_job() {
        let state = AppState::new(Session::new(true, temp()).unwrap());
        *state.job.lock().unwrap() = Job {
            id: 2,
            kind: "speed".into(),
            phase: "new".into(),
            active: true,
            ..Job::default()
        };
        state.progress(1, "stale".into());
        state.finish(1, Ok(()));
        let job = state.job.lock().unwrap();
        assert_eq!(job.phase, "new");
        assert!(job.active);
    }
    #[test]
    fn adb_update_prevents_process_exit_until_the_transaction_finishes() {
        let state = AppState::new(Session::new(true, temp()).unwrap());
        *state.job.lock().unwrap() = Job {
            id: 1,
            kind: "adb".into(),
            active: true,
            ..Job::default()
        };
        assert!(state.can_exit().is_err());
        assert!(state.hide_on_close());
        state.job.lock().unwrap().active = false;
        assert!(state.can_exit().is_ok());
    }
    #[test]
    fn measurement_can_be_cancelled_on_exit_but_closing_its_window_hides_it() {
        let state = AppState::new(Session::new(true, temp()).unwrap());
        *state.job.lock().unwrap() = Job {
            id: 1,
            kind: "speed".into(),
            active: true,
            ..Job::default()
        };
        assert!(state.can_exit().is_ok());
        assert!(state.hide_on_close());
    }
    #[test]
    fn exit_gate_prevents_a_new_job_after_a_cancellable_job_finishes() {
        let state = AppState::new(Session::new(false, temp()).unwrap());
        let (id, cancel) = state.begin_job("speed").unwrap();
        state.begin_shutdown().unwrap();
        state.finish(id, Ok(()));
        assert!(state.begin_job("adb").is_err());
        assert!(cancel.load(Ordering::Relaxed));
    }
    #[test]
    fn application_update_blocks_exit_and_other_jobs() {
        let state = AppState::new(Session::new(false, temp()).unwrap());
        let (id, _) = state.begin_job("app-update").unwrap();
        assert!(state.can_exit().is_err());
        assert!(state.begin_shutdown().is_err());
        assert!(state.begin_job("adb").is_err());
        assert!(state.hide_on_close());
        state.finish(id, Ok(()));
        assert!(state.can_exit().is_ok());
    }
    #[test]
    fn stale_update_cannot_prepare_installation() {
        let state = AppState::new(Session::new(false, temp()).unwrap());
        let (id, _) = state.begin_job("app-update").unwrap();
        assert!(state.prepare_update_install(id + 1).is_err());
        assert!(!state.shutting_down.load(Ordering::SeqCst));
    }
    #[test]
    fn failed_proxy_restore_does_not_allow_update_installation() {
        let path = temp();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let backup = path.with_file_name("invalid-backup.json");
        std::fs::write(&backup, "invalid json").unwrap();
        let mut session = Session::new(false, path).unwrap();
        session.backup = backup;
        session.proxy_enabled = true;
        let state = AppState::new(session);
        let (id, _) = state.begin_job("app-update").unwrap();
        assert!(state.prepare_update_install(id).is_err());
        assert!(state.session.lock().unwrap().owned());
        assert!(!state.shutting_down.load(Ordering::SeqCst));
    }
    #[test]
    fn preview_cannot_prepare_update_installation() {
        let state = AppState::new(Session::new(true, temp()).unwrap());
        assert!(state.begin_job("app-update").is_err());
        assert!(state.prepare_update_install(1).is_err());
    }
}

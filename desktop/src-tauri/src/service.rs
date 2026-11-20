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
    pub windows: netbridge::diagnostics::Probe,
    pub selected: netbridge::diagnostics::Probe,
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
    pub config_error: Option<String>,
    pub recovery_available: bool,
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
    pub(crate) traffic: netbridge::traffic::Traffic,
    server: Option<Server>,
    proxy_enabled: bool,
    watchdog_started: bool,
    disk_revision: String,
    proxy_reader: fn() -> std::io::Result<system_proxy::Settings>,
    config_error: Option<String>,
}
fn profile_revision(path: &std::path::Path) -> Result<String, String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() > Config::MAX_BYTES as u64 => Ok(format!(
            "oversized:{}:{:?}",
            metadata.len(),
            metadata.modified().ok()
        )),
        Ok(_) => netbridge::storage::read_limited(path, Config::MAX_BYTES)
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
            .map_err(|e| e.to_string()),
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
    pub fn needs_recovery(&self) -> bool {
        self.config_error.is_some()
    }
    pub fn usb_preflight(&self, revision: &str, available: &[Adapter]) -> Result<Config, String> {
        self.ensure_ready()?;
        if self.owned() || self.server.is_some() {
            return Err("Phiên đang chạy. Dùng kết nối USB riêng để khôi phục đường mạng.".into());
        }
        if revision != self.disk_revision || self.changed_on_disk() {
            return Err("Lưu hoặc tải lại cấu hình trước khi kết nối.".into());
        }
        if matches!(self.view().kind.as_str(), "external" | "unknown") {
            return Err(
                "Một phiên khác đang giữ proxy hoặc chưa đọc được trạng thái Windows.".into(),
            );
        }
        // Suggested adapter IDs in the UI are not saved selections.
        let (config, read_revision) = read_profile(&self.path)?;
        if read_revision != revision {
            return Err("Cấu hình đã thay đổi. Tải lại trước khi kết nối.".into());
        }
        netbridge::connection_flow::validate_saved_profile(&config, available)?;
        Ok(config)
    }
    pub fn preferences_path(&self) -> PathBuf {
        self.path.with_file_name("toolbox-preferences.json")
    }
    pub fn settings_backup(&self) -> Result<netbridge::settings_backup::SettingsBackup, String> {
        self.ensure_ready()?;
        if self.changed_on_disk() {
            return Err("Cấu hình đã thay đổi ngoài ứng dụng. Tải lại trước khi sao lưu.".into());
        }
        let preferences = netbridge::speed_view::Preferences::load(&self.preferences_path())
            .map_err(|e| e.to_string())?;
        Ok(netbridge::settings_backup::SettingsBackup::new(
            self.config(),
            preferences,
        ))
    }
    pub fn restore_settings(
        &mut self,
        backup: &netbridge::settings_backup::SettingsBackup,
        expected: &str,
    ) -> Result<PathBuf, String> {
        self.ensure_live()?;
        backup.validate().map_err(|e| e.to_string())?;
        if self.owned() || self.server.is_some() {
            return Err("Tắt phân luồng trước khi khôi phục setting.".into());
        }
        if expected != self.disk_revision || self.changed_on_disk() {
            return Err("Cấu hình đã thay đổi. Chọn lại file trước khi khôi phục.".into());
        }
        if matches!(self.view().kind.as_str(), "external" | "unknown") {
            return Err("Chưa thể xác minh quyền sở hữu cấu hình. Không khôi phục.".into());
        }
        let revision = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec_pretty(&backup.config).map_err(|e| e.to_string())?)
        );
        let recovery =
            netbridge::settings_backup::restore_files(&self.path, &self.preferences_path(), backup)
                .map_err(|e| e.to_string())?;
        *self.config.write().map_err(|_| "Config lock")? = backup.config.clone();
        self.disk_revision = revision;
        self.config_error = None;
        Ok(recovery)
    }
    pub fn recover_config(&mut self, expected: &str) -> Result<(), String> {
        self.ensure_live()?;
        if self.owned() || self.server.is_some() {
            return Err("Tắt phân luồng trước khi phục hồi cấu hình.".into());
        }
        if self.config_error.is_none() {
            return Err("Cấu hình hiện tại không cần phục hồi.".into());
        }
        if expected != self.disk_revision || self.changed_on_disk() {
            return Err("File đã thay đổi. Tải lại trạng thái trước khi phục hồi.".into());
        }
        if matches!(self.view().kind.as_str(), "external" | "unknown") {
            return Err("Chưa thể xác minh quyền sở hữu phiên.".into());
        }
        let (config, _) = Config::recover_last_good(&self.path).map_err(|e| e.to_string())?;
        *self.config.write().map_err(|_| "Config lock")? = config;
        self.disk_revision = profile_revision(&self.path)?;
        self.config_error = None;
        Ok(())
    }
    pub fn ensure_ready(&self) -> Result<(), String> {
        self.ensure_live()?;
        if self.config_error.is_some() {
            return Err("Cần phục hồi cấu hình trước khi thực hiện tác vụ mạng.".into());
        }
        Ok(())
    }
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
        let (mut cfg, revision) = match read_profile(&self.path) {
            Ok(value) => value,
            Err(error) => {
                self.config_error = Some(error.clone());
                self.disk_revision =
                    profile_revision(&self.path).unwrap_or_else(|_| "unreadable".into());
                return Err(error);
            }
        };
        cfg.auto_select(&adapters::enumerate().unwrap_or_default());
        *self.config.write().map_err(|_| "Config lock")? = cfg;
        self.disk_revision = revision;
        self.config_error = None;
        Ok(())
    }
    pub fn new(preview: bool, path: PathBuf) -> Result<Self, String> {
        let (mut config, disk_revision, config_error) = if preview {
            (Config::default(), "preview".into(), None)
        } else {
            match read_profile(&path) {
                Ok((config, revision)) => (config, revision, None),
                Err(error) => (
                    Config::default(),
                    profile_revision(&path).unwrap_or_else(|_| "unreadable".into()),
                    Some(error),
                ),
            }
        };
        config.auto_select(&adapters::enumerate().unwrap_or_default());
        Ok(Self {
            preview,
            config: Arc::new(RwLock::new(config)),
            path,
            backup: netbridge::config::data_dir().join("proxy-backup.json"),
            traffic: Default::default(),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            server: None,
            proxy_enabled: false,
            watchdog_started: false,
            disk_revision,
            proxy_reader: system_proxy::read,
            config_error,
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
        let current = (self.proxy_reader)().ok();
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
                "Bật proxy để áp dụng quy tắc cho các ứng dụng dùng proxy Windows.",
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
        self.ensure_ready()?;
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
        self.ensure_ready()?;
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
        let server = Server::start_monitored(self.config.clone(), self.logs.clone())
            .map_err(|e| e.to_string())?;
        self.traffic = server.traffic.clone();
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
    pub(crate) fn listener_port(&self) -> Option<u16> {
        self.server.as_ref().map(|server| server.port)
    }
    pub fn owned(&self) -> bool {
        self.proxy_enabled
    }
}
pub struct AppState {
    logs: Log,
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
    pub fn start_usb_job(&self, id: u64, revision: &str, adapter: &Adapter) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| "Session lock")?;
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if !job.active
            || job.id != id
            || job.kind != "usb"
            || self.shutting_down.load(Ordering::SeqCst)
        {
            return Err("Tác vụ kết nối không còn hợp lệ.".into());
        }
        let available = adapters::enumerate().map_err(|e| e.to_string())?;
        let mut config = session.usb_preflight(revision, &available)?;
        if !available.iter().any(|a| {
            a.id == adapter.id && a.ip == adapter.ip && a.is_usb_network() && a.id != config.lan_id
        }) {
            return Err("Card USB hoặc IP nguồn vừa thay đổi. Chọn lại card.".into());
        }
        if config.usb_id != adapter.id {
            config.usb_id = adapter.id.clone();
            session.save(config)?;
        }
        session.start()
    }
    pub fn recover_config(&self, revision: &str) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| "Session lock")?;
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active || self.shutting_down.load(Ordering::SeqCst) {
            return Err("Chờ tác vụ hiện tại kết thúc trước khi phục hồi.".into());
        }
        session.recover_config(revision)?;
        self.targets.lock().map_err(|_| "Target lock")?.clear();
        Ok(())
    }
    pub fn restore_settings(
        &self,
        backup: &netbridge::settings_backup::SettingsBackup,
        revision: &str,
    ) -> Result<PathBuf, String> {
        let mut session = self.session.lock().map_err(|_| "Session lock")?;
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active || self.shutting_down.load(Ordering::SeqCst) {
            return Err("Chờ tác vụ hiện tại hoàn tất trước khi khôi phục.".into());
        }
        let recovery = session.restore_settings(backup, revision)?;
        self.targets.lock().map_err(|_| "Target lock")?.clear();
        self.speeds.lock().map_err(|_| "Speed lock")?.clear();
        Ok(recovery)
    }
    pub fn begin_shutdown(&self) -> Result<(), String> {
        let job = self.job.lock().map_err(|_| "Job lock")?;
        if job.active && matches!(job.kind.as_str(), "adb" | "usb" | "app-update" | "settings") {
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
        if job.active && matches!(job.kind.as_str(), "adb" | "usb" | "app-update" | "settings") {
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
            logs: session.logs.clone(),
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
        {
            let session = self.session.lock().map_err(|_| "Session lock")?;
            if kind == "settings" {
                session.ensure_live()?;
            } else {
                session.ensure_ready()?;
            }
        }
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
        proxy::log(&self.logs, format!("#{id} {kind} · Bắt đầu"));
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
        let summary;
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
                summary = format!(
                    "#{id} {} · {}",
                    job.kind,
                    job.error.as_ref().unwrap_or(&job.phase)
                );
            } else {
                return;
            }
        } else {
            return;
        }
        proxy::log(&self.logs, summary);
    }
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let session = self.session.lock().map_err(|_| "Session lock")?;
        let unit = netbridge::speed_view::Preferences::load(&session.preferences_path())
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
            config_error: session.config_error.clone(),
            recovery_available: session.config_error.is_some()
                && Config::last_good(&session.path).is_ok(),
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
    fn isolated_session(preview: bool, path: PathBuf) -> Result<Session, String> {
        let mut session = Session::new(preview, path)?;
        session.proxy_reader = || {
            Ok(system_proxy::Settings {
                flags: 1,
                server: String::new(),
                bypass: String::new(),
                pac_url: String::new(),
            })
        };
        Ok(session)
    }

    fn temp() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/qa-tests")
            .join(netbridge::storage::stamp())
            .join("config.json")
    }
    #[test]
    fn preview_rejects_all_session_mutations_before_any_write() {
        let path = temp();
        let mut session = isolated_session(true, path.clone()).unwrap();
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
        let mut session = isolated_session(false, path.clone()).unwrap();
        let mut draft = original;
        draft.rules.push(netbridge::config::Rule {
            pattern: "not a valid host".into(),
            route: Route::Lan,
        });
        assert!(session.save(draft).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn stale_draft_revision_cannot_overwrite_a_saved_profile() {
        let path = temp();
        Config::default().save(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut session = isolated_session(false, path.clone()).unwrap();
        let mut draft = session.config();
        draft.rules.push(netbridge::config::Rule {
            pattern: "changed.invalid".into(),
            route: Route::Lan,
        });
        assert!(session.save_expected(draft, "stale-revision").is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn externally_changed_profile_is_preserved_when_saving_a_draft() {
        let path = temp();
        Config::default().save(&path).unwrap();
        let mut session = isolated_session(false, path.clone()).unwrap();
        let revision = session.revision();
        let draft = session.config();
        let mut external = Config::default();
        external.rules.push(netbridge::config::Rule {
            pattern: "external.invalid".into(),
            route: Route::Lan,
        });
        external.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(session.save_expected(draft, &revision).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn saved_profile_gets_a_new_revision() {
        let path = temp();
        let mut session = isolated_session(false, path).unwrap();
        let before = session.revision();
        let mut draft = session.config();
        draft.rules.push(netbridge::config::Rule {
            pattern: "saved.invalid".into(),
            route: Route::Lan,
        });
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
        let state = AppState::new(isolated_session(true, temp()).unwrap());
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
        let state = AppState::new(isolated_session(true, temp()).unwrap());
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
        let state = AppState::new(isolated_session(true, temp()).unwrap());
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
        let state = AppState::new(isolated_session(false, temp()).unwrap());
        let (id, cancel) = state.begin_job("speed").unwrap();
        state.begin_shutdown().unwrap();
        state.finish(id, Ok(()));
        assert!(state.begin_job("adb").is_err());
        assert!(cancel.load(Ordering::Relaxed));
    }
    #[test]
    fn application_update_blocks_exit_and_other_jobs() {
        let state = AppState::new(isolated_session(false, temp()).unwrap());
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
        let state = AppState::new(isolated_session(false, temp()).unwrap());
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
        let mut session = isolated_session(false, path).unwrap();
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
        let state = AppState::new(isolated_session(true, temp()).unwrap());
        assert!(state.begin_job("app-update").is_err());
        assert!(state.prepare_update_install(1).is_err());
    }
    #[test]
    fn settings_restore_rejects_preview_owned_session_busy_job_and_stale_revision() {
        let backup = netbridge::settings_backup::SettingsBackup::new(
            Config::default(),
            netbridge::speed_view::Preferences::default(),
        );
        let mut preview = isolated_session(true, temp()).unwrap();
        assert!(preview.restore_settings(&backup, "preview").is_err());
        assert!(preview.settings_backup().is_err());
        let mut live = isolated_session(false, temp()).unwrap();
        let revision = live.revision();
        live.proxy_enabled = true;
        assert!(live.restore_settings(&backup, &revision).is_err());
        live.proxy_enabled = false;
        assert!(live.restore_settings(&backup, "stale").is_err());
        let state = AppState::new(live);
        let (id, _) = state.begin_job("speed").unwrap();
        assert!(state.restore_settings(&backup, &revision).is_err());
        state.finish(id, Ok(()));
    }
    #[test]
    fn settings_restore_updates_revision_unit_and_preserves_recovery() {
        let path = temp();
        let mut session = isolated_session(false, path).unwrap();
        let revision = session.revision();
        let config = Config {
            port: 18001,
            ..Config::default()
        };
        let backup = netbridge::settings_backup::SettingsBackup::new(
            config,
            netbridge::speed_view::Preferences {
                unit: netbridge::speed_view::SpeedUnit::Megabytes,
            },
        );
        let recovery = session.restore_settings(&backup, &revision).unwrap();
        assert!(recovery.is_dir());
        assert_ne!(session.revision(), revision);
        assert_eq!(session.config().port, 18001);
        assert_eq!(
            session.settings_backup().unwrap().preferences.unit,
            netbridge::speed_view::SpeedUnit::Megabytes
        );
    }
    #[test]
    fn another_proxy_owner_blocks_settings_restore_without_writing() {
        let path = temp();
        Config::default().save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut session = isolated_session(false, path.clone()).unwrap();
        session.proxy_reader = || {
            Ok(system_proxy::Settings {
                flags: 3,
                server: "127.0.0.1:17890".into(),
                bypass: String::new(),
                pac_url: String::new(),
            })
        };
        let backup = netbridge::settings_backup::SettingsBackup::new(
            Config::default(),
            netbridge::speed_view::Preferences::default(),
        );
        assert!(session
            .restore_settings(&backup, &session.revision())
            .is_err());
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
    #[test]
    fn corrupt_config_opens_recovery_state_and_blocks_network_jobs() {
        let path = temp();
        Config::default().save(&path).unwrap();
        Config::default().save(&path).unwrap();
        std::fs::write(&path, b"{partial").unwrap();
        let mut session = isolated_session(false, path.clone()).unwrap();
        assert!(session.config_error.is_some());
        assert!(session.start().is_err());
        assert!(session.settings_backup().is_err());
        let revision = session.revision();
        assert!(session.recover_config("stale").is_err());
        session.recover_config(&revision).unwrap();
        assert!(session.config_error.is_none());
        assert_eq!(Config::load(&path).unwrap().port, 17890);
    }
    #[test]
    fn recovery_can_import_valid_backup_when_no_last_good_exists() {
        let path = temp();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"broken").unwrap();
        let session = isolated_session(false, path).unwrap();
        let revision = session.revision();
        let state = AppState::new(session);
        assert!(state.begin_job("usb").is_err());
        assert!(state.recover_config(&revision).is_err());
        let backup = netbridge::settings_backup::SettingsBackup::new(
            Config::default(),
            netbridge::speed_view::Preferences::default(),
        );
        state.restore_settings(&backup, &revision).unwrap();
        assert!(state.session.lock().unwrap().config_error.is_none());
    }
    #[test]
    fn combined_preflight_uses_saved_ids_not_ui_suggestions() {
        let path = temp();
        let cfg = Config {
            lan_id: "saved-lan".into(),
            rules: vec![netbridge::config::Rule {
                pattern: "portal.example.test".into(),
                route: Route::Lan,
            }],
            ..Config::default()
        };
        cfg.save(&path).unwrap();
        let session = isolated_session(false, path).unwrap();
        session.config.write().unwrap().usb_id = "suggested-usb".into();
        let adapter = Adapter {
            id: "saved-lan".into(),
            name: "LAN".into(),
            description: "Ethernet".into(),
            index: 1,
            ip: "192.0.2.1".parse().unwrap(),
            dns: vec![],
        gateway: None,
            if_type: 6,
        };
        let saved = session
            .usb_preflight(&session.revision(), &[adapter])
            .unwrap();
        assert!(saved.usb_id.is_empty());
    }
    #[test]
    fn externally_corrected_file_can_exit_recovery_by_reload() {
        let path = temp();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{broken").unwrap();
        let mut session = isolated_session(false, path.clone()).unwrap();
        let old = session.revision();
        std::fs::write(&path, serde_json::to_vec(&Config::default()).unwrap()).unwrap();
        assert!(session.changed_on_disk());
        assert!(session.recover_config(&old).is_err());
        session.reload().unwrap();
        assert!(!session.needs_recovery());
    }
}

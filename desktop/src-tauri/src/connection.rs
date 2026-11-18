use crate::service::AppState;
use netbridge::{
    adapters::{self, Adapter},
    config::Config,
    connection_flow::{self, ConnectionActions},
    devices, system_proxy,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};

struct UsbConnection {
    app: AppHandle,
    state: Arc<AppState>,
    id: u64,
    serial: String,
    revision: String,
    config: Option<Config>,
    adapter: Option<Adapter>,
    original_proxy: Option<system_proxy::Settings>,
    original_usb: Option<String>,
}
impl UsbConnection {
    fn phase(&self, text: &str) {
        self.state.progress(self.id, text.into());
        let _ = self.app.emit("netbridge://state", self.id);
    }
    fn config(&self) -> &Config {
        self.config.as_ref().expect("preflight completed")
    }
}
impl ConnectionActions for UsbConnection {
    fn preflight(&mut self) -> Result<(), String> {
        let available = adapters::enumerate().map_err(|e| e.to_string())?;
        self.config = Some(
            self.state
                .session
                .lock()
                .map_err(|_| "Session lock")?
                .usb_preflight(&self.revision, &available)?,
        );
        self.original_proxy = Some(system_proxy::read().map_err(|e| e.to_string())?);
        Ok(())
    }
    fn usb_enabled(&mut self) -> Result<bool, String> {
        let functions = devices::usb_functions(&self.config().adb_path, &self.serial)
            .map_err(|e| e.to_string())?;
        let enabled = devices::usb_network_enabled(&functions);
        if !enabled && !devices::can_restore_usb_functions(&functions) {
            return Err("Chưa xác định được chế độ USB có thể phục hồi. Dùng kết nối USB riêng để thiết lập thiết bị.".into());
        }
        self.original_usb = Some(functions);
        Ok(enabled)
    }
    fn enable_usb(&mut self) -> Result<(), String> {
        self.phase("Đang kết nối Ethernet USB…");
        devices::set_tether(&self.config().adb_path, &self.serial, true)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn wait_for_adapter(&mut self) -> Result<(), String> {
        self.phase("Đang chờ adapter USB nhận địa chỉ IPv4…");
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(available) = adapters::enumerate() {
                if let Some(adapter) = connection_flow::usb_candidate(self.config(), &available)? {
                    self.adapter = Some(adapter);
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err("Chưa thấy adapter USB đã chọn nhận IPv4. Có thể dùng Chỉ kết nối USB, chọn lại card và lưu cấu hình.".into());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    fn start_routing(&mut self) -> Result<(), String> {
        self.phase("USB đã sẵn sàng. Đang bật phân luồng…");
        self.state.start_usb_job(
            self.id,
            &self.revision,
            self.adapter.as_ref().ok_or("Adapter USB chưa sẵn sàng")?,
        )
    }
    fn must_keep_usb(&self) -> bool {
        if self
            .state
            .session
            .lock()
            .map_or(true, |session| session.owned())
        {
            return true;
        }
        system_proxy::read().map_or(true, |current| {
            self.original_proxy.as_ref() != Some(&current)
        })
    }
    fn undo_usb(&mut self) -> Result<(), String> {
        self.phase("Đang phục hồi chế độ USB trước thao tác…");
        devices::restore_usb_functions(
            &self.config().adb_path,
            &self.serial,
            self.original_usb
                .as_deref()
                .ok_or("Không có trạng thái USB ban đầu")?,
        )
        .map_err(|e| e.to_string())
    }
}
#[tauri::command]
pub fn connect_usb_and_route(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    serial: String,
    expected_revision: String,
) -> Result<u64, String> {
    let state = state.inner().clone();
    let (id, _) = state.begin_job("usb")?;
    let _ = app.emit("netbridge://state", id);
    tauri::async_runtime::spawn_blocking(move || {
        let mut actions = UsbConnection {
            app: app.clone(),
            state: state.clone(),
            id,
            serial,
            revision: expected_revision,
            config: None,
            adapter: None,
            original_proxy: None,
            original_usb: None,
        };
        let result = connection_flow::connect_and_route(&mut actions);
        state.finish(id, result);
        let _ = app.emit("netbridge://state", id);
    });
    Ok(id)
}

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
    confirmed_adapter: Option<(String, std::net::Ipv4Addr)>,
    automatic: bool,
    config: Option<Config>,
    adapter: Option<Adapter>,
    original_proxy: Option<system_proxy::Settings>,
    original_usb: Option<String>,
}
impl UsbConnection {
    fn verify_card(
        &self,
        ids: &netbridge::usb_identity::UsbIdentity,
        adapter: &Adapter,
    ) -> Result<(), String> {
        let owner = ids.serial(&adapter.id);
        if owner.is_some_and(|s| s.eq_ignore_ascii_case(&self.serial)) {
            return Ok(());
        }
        if self.automatic || self.confirmed_adapter.is_none() {
            return Err(
                "Chưa xác minh được card của điện thoại. Làm mới thiết bị rồi chọn lại.".into(),
            );
        }
        if let Some(owner) = owner {
            if devices::list(&self.config().adb_path)
                .map_err(|e| e.to_string())?
                .iter()
                .any(|d| d.usb && d.serial.eq_ignore_ascii_case(owner))
            {
                return Err(
                    "Card USB đang thuộc điện thoại khác. Làm mới danh sách thiết bị.".into(),
                );
            }
        }
        Ok(())
    }
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
        let candidates: Vec<_> = available
            .iter()
            .filter(|a| a.is_usb_network() && a.id != self.config().lan_id)
            .collect();
        let ids = netbridge::usb_identity::read();
        if let Some((id, ip)) = &self.confirmed_adapter {
            let card = candidates
                .iter()
                .find(|a| &a.id == id && &a.ip == ip)
                .ok_or("Card USB đã thay đổi. Chọn lại card trước khi bật.")?;
            self.verify_card(&ids, card)?;
            self.config
                .as_mut()
                .expect("profile loaded")
                .usb_id
                .clone_from(id);
        } else {
            self.config.as_mut().expect("profile loaded").usb_id.clear();
        }
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
                let ids = netbridge::usb_identity::read();
                let candidate = if self.confirmed_adapter.is_some() {
                    connection_flow::usb_candidate(self.config(), &available)?
                } else {
                    let mut matches = available.iter().filter(|card| {
                        card.is_usb_network()
                            && card.id != self.config().lan_id
                            && ids
                                .serial(&card.id)
                                .is_some_and(|s| s.eq_ignore_ascii_case(&self.serial))
                    });
                    let first = matches.next().cloned();
                    if matches.next().is_some() {
                        return Err(
                            "Điện thoại có nhiều card USB. Làm mới rồi chọn card cần dùng.".into(),
                        );
                    }
                    first
                };
                if let Some(adapter) = candidate {
                    self.verify_card(&ids, &adapter)?;
                    if self
                        .confirmed_adapter
                        .as_ref()
                        .is_some_and(|(_, ip)| adapter.ip != *ip)
                    {
                        return Err("IP của card USB đã thay đổi. Chọn lại card.".into());
                    }
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
        self.verify_card(
            &netbridge::usb_identity::read(),
            self.adapter.as_ref().ok_or("Adapter USB chưa sẵn sàng")?,
        )?;
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
    confirmed_adapter: Option<(String, std::net::Ipv4Addr)>,
    automatic: Option<bool>,
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
            confirmed_adapter,
            automatic: automatic.unwrap_or(false),
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

use crate::{adapters::Adapter, config::Config, policy::Route};

pub trait ConnectionActions {
    fn preflight(&mut self) -> Result<(), String>;
    fn usb_enabled(&mut self) -> Result<bool, String>;
    fn enable_usb(&mut self) -> Result<(), String>;
    fn wait_for_adapter(&mut self) -> Result<(), String>;
    fn start_routing(&mut self) -> Result<(), String>;
    fn must_keep_usb(&self) -> bool;
    fn undo_usb(&mut self) -> Result<(), String>;
}
pub fn connect_and_route(actions: &mut impl ConnectionActions) -> Result<(), String> {
    actions.preflight()?;
    let existing = actions.usb_enabled()?;
    let outcome = (|| {
        if !existing {
            actions.enable_usb()?;
        }
        actions.wait_for_adapter()?;
        actions.start_routing()
    })();
    match outcome {
        Ok(()) => Ok(()),
        Err(error) if existing || actions.must_keep_usb() => Err(format!(
            "{error} USB được giữ nguyên để bảo toàn kết nối hiện có."
        )),
        Err(error) => match actions.undo_usb() {
            Ok(()) => Err(format!("{error} Đã hoàn tác Ethernet USB vừa bật.")),
            Err(rollback) => Err(format!(
                "{error} Chưa hoàn tác được USB: {rollback}. Kiểm tra kết nối trước khi thử lại."
            )),
        },
    }
}
pub fn validate_saved_profile(config: &Config, available: &[Adapter]) -> Result<(), String> {
    config.validate().map_err(|e| e.to_string())?;
    if config.rules.is_empty() {
        return Err("Thêm và lưu quy tắc trước khi kết nối USB kèm phân luồng.".into());
    }
    if !config.lan_id.is_empty() && config.lan_id == config.usb_id {
        return Err("Chọn hai adapter LAN và USB khác nhau cho thao tác kết nối này.".into());
    }
    if config.rules.iter().any(|rule| rule.route == Route::Lan)
        && !available.iter().any(|adapter| adapter.id == config.lan_id)
    {
        return Err("Adapter LAN đã chọn chưa sẵn sàng.".into());
    }
    Ok(())
}
pub fn usb_candidate(config: &Config, available: &[Adapter]) -> Result<Option<Adapter>, String> {
    if !config.usb_id.is_empty() {
        return Ok(available
            .iter()
            .find(|adapter| adapter.id == config.usb_id)
            .cloned());
    }
    let candidates: Vec<_> = available
        .iter()
        .filter(|adapter| adapter.id != config.lan_id && adapter.is_usb_network())
        .collect();
    match candidates.as_slice() {
        [] => Ok(None),
        [adapter] => Ok(Some((*adapter).clone())),
        _ => Err("Có nhiều adapter USB. Chọn và lưu đúng adapter trước khi bật phân luồng.".into()),
    }
}

#[tauri::command]
pub async fn network_monitor() -> Result<netbridge::network_monitor::Snapshot, String> {
    tauri::async_runtime::spawn_blocking(netbridge::network_monitor::snapshot)
        .await
        .map_err(|e| e.to_string())
}

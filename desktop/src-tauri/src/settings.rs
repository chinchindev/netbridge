use crate::service::AppState;
use netbridge::settings_backup::{SettingsBackup, MAX_BACKUP_BYTES};
use serde::Serialize;
use std::{io::Read, sync::Arc};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn recover_config(
    state: State<'_, Arc<AppState>>,
    expected_revision: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.recover_config(&expected_revision))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    backup: SettingsBackup,
    revision: String,
    warnings: Vec<String>,
}
#[tauri::command]
pub async fn export_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    let (id, _) = state.begin_job("settings")?;
    let worker = state.clone();
    let handle = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let backup = worker
            .session
            .lock()
            .map_err(|_| "Session lock")?
            .settings_backup()?;
        let Some(file) = handle
            .dialog()
            .file()
            .add_filter("NetBridge settings", &["json"])
            .set_file_name("NetBridge-settings.json")
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        {
            let session = worker.session.lock().map_err(|_| "Session lock")?;
            let selected = path.canonicalize().unwrap_or_else(|_| path.clone());
            for protected in [&session.path, &session.preferences_path(), &session.backup] {
                if selected
                    == protected
                        .canonicalize()
                        .unwrap_or_else(|_| protected.clone())
                {
                    return Err(
                        "Chọn file sao lưu riêng, không ghi đè file cấu hình đang sử dụng.".into(),
                    );
                }
            }
        }
        netbridge::settings_backup::write_backup_file(&path, &backup).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|value| value);
    state.finish(id, outcome.as_ref().map(|_| ()).map_err(Clone::clone));
    let _ = app.emit("netbridge://state", id);
    outcome
}
#[tauri::command]
pub async fn import_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<ImportPreview>, String> {
    let state = state.inner().clone();
    let (id, _) = state.begin_job("settings")?;
    let worker = state.clone();
    let handle = app.clone();
    let outcome=tauri::async_runtime::spawn_blocking(move || {
        let Some(file)=handle.dialog().file().add_filter("NetBridge settings", &["json"]).blocking_pick_file() else { return Ok(None) };
        let path=file.into_path().map_err(|e|e.to_string())?;
        let mut bytes=Vec::new();std::fs::File::open(path).map_err(|e|e.to_string())?.take(MAX_BACKUP_BYTES as u64+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        let backup=SettingsBackup::parse(&bytes).map_err(|e|e.to_string())?;
        let adapters=netbridge::adapters::enumerate().map_err(|e|e.to_string())?;
        let mut warnings=Vec::new();
        for (label,id) in [("LAN",&backup.config.lan_id),("USB",&backup.config.usb_id)] {
            if !id.is_empty()&&!adapters.iter().any(|adapter| &adapter.id==id){warnings.push(format!("Adapter {label} trong file chưa có trên máy này; hãy chọn lại sau khi khôi phục."));}
        }
        if backup.config.adb_path!="auto" {warnings.push("File có đường dẫn ADB tùy chỉnh. Kiểm tra đường dẫn trước khi dùng thiết bị.".into());}
        let revision=worker.session.lock().map_err(|_|"Session lock")?.revision();
        Ok(Some(ImportPreview{backup,revision,warnings}))
    }).await.map_err(|e|e.to_string()).and_then(|value|value);
    state.finish(id, outcome.as_ref().map(|_| ()).map_err(Clone::clone));
    let _ = app.emit("netbridge://state", id);
    outcome
}
#[tauri::command]
pub async fn restore_settings(
    state: State<'_, Arc<AppState>>,
    backup: SettingsBackup,
    expected_revision: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .restore_settings(&backup, &expected_revision)
            .map(|path| path.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

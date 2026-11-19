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
            let recovery = session.path.with_extension("json.bak");
            let directory = session.path.parent().ok_or("Thiếu thư mục cấu hình")?;
            if reserved_export(&selected, directory) {
                return Err("Chọn nơi lưu khác ngoài các file phục hồi của NetBridge.".into());
            }
            for protected in [
                &session.path,
                &session.preferences_path(),
                &session.backup,
                &recovery,
            ] {
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

fn reserved_export(selected: &std::path::Path, directory: &std::path::Path) -> bool {
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_owned());
    let parent = selected.parent().and_then(|p| p.canonicalize().ok());
    let Some(parent) = parent else { return false };
    // Windows paths are case-insensitive, including new destinations in an existing directory.
    let parent = parent.to_string_lossy().to_lowercase();
    let directory = directory.to_string_lossy().to_lowercase();
    let Some(name) = selected.file_name() else {
        return false;
    };
    let name = name.to_string_lossy().to_lowercase();
    let sub = std::path::Path::new(&directory).join("settings-backups");
    std::path::Path::new(&parent).starts_with(sub)
        || (parent == directory
            && (matches!(
                name.as_str(),
                "config.json.bak"
                    | "config.json"
                    | "toolbox-preferences.json"
                    | "proxy-backup.json"
                    | "watchdog-result.json"
            ) || (name.starts_with("proxy-backup.") && name.ends_with(".bak"))))
}

#[tauri::command]
pub fn diagnostic_details(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let session = state.session.lock().map_err(|_| "Session lock")?;
    let parent = session
        .path
        .parent()
        .ok_or("Thiếu thư mục cấu hình")?
        .to_owned();
    let traffic = session.traffic.snapshot();
    drop(session);
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut latest = None;
    for entry in std::fs::read_dir(&parent).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("proxy-backup.") && name.ends_with(".bak") {
            if let Ok(meta) = entry.metadata() {
                count += 1;
                bytes += meta.len();
                latest = latest.max(
                    meta.modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|t| t.as_secs()),
                );
            }
        }
    }
    let watchdog = netbridge::storage::read_limited(&parent.join("watchdog-result.json"), 4096)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    Ok(
        serde_json::json!({"traffic":traffic,"backups":{"count":count,"bytes":bytes,"latest":latest},"watchdog":watchdog}),
    )
}

#[cfg(test)]
mod tests {
    use super::reserved_export;
    #[test]
    fn protects_last_good_and_rotated_backups_at_new_destinations() {
        let directory = std::env::current_dir().unwrap();
        for name in [
            "CONFIG.JSON.BAK",
            "proxy-backup.012.bak",
            "watchdog-result.json",
        ] {
            assert!(reserved_export(&directory.join(name), &directory));
        }
        assert!(!reserved_export(
            &directory.join("my-settings.json"),
            &directory
        ));
    }
}

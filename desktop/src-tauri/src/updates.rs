use crate::service::AppState;
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    pub status: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}
#[derive(Default)]
pub struct UpdateStore {
    pub view: Mutex<UpdateView>,
    pending: Mutex<Option<Update>>,
}
fn signal(app: &AppHandle, id: u64) {
    let _ = app.emit("netbridge://state", id);
}
fn fail(store: &UpdateStore, error: String) {
    if let Ok(mut view) = store.view.lock() {
        view.status = "error".into();
        view.error = Some(error);
    }
}

#[tauri::command]
pub fn get_app_update(store: State<'_, Arc<UpdateStore>>) -> Result<UpdateView, String> {
    store
        .view
        .lock()
        .map(|view| view.clone())
        .map_err(|_| "Update lock".into())
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    store: State<'_, Arc<UpdateStore>>,
) -> Result<(), String> {
    let (id, _) = state.begin_job("app-update")?;
    let result: Result<(), String> = async {
        *store.pending.lock().map_err(|_| "Update lock")? = None;
        *store.view.lock().map_err(|_| "Update lock")? = UpdateView {
            status: "checking".into(),
            ..Default::default()
        };
        state.progress(id, "Đang kiểm tra phiên bản NetBridge…".into());
        signal(&app, id);
        let candidate = app
            .updater_builder()
            .timeout(Duration::from_secs(25))
            .build()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(update) = candidate.as_ref() {
            if update.download_url.scheme() != "https" {
                return Err("Địa chỉ tải cập nhật phải dùng HTTPS.".into());
            }
        }
        *store.view.lock().map_err(|_| "Update lock")? = UpdateView {
            status: if candidate.is_some() {
                "available"
            } else {
                "current"
            }
            .into(),
            version: candidate.as_ref().map(|update| update.version.clone()),
            notes: candidate.as_ref().and_then(|update| update.body.clone()),
            ..Default::default()
        };
        *store.pending.lock().map_err(|_| "Update lock")? = candidate;
        Ok(())
    }
    .await;
    if let Err(ref error) = result {
        fail(&store, error.clone());
    }
    state.finish(id, result.clone());
    signal(&app, id);
    result
}

#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    store: State<'_, Arc<UpdateStore>>,
    expected_version: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    let store = store.inner().clone();
    let (id, _) = state.begin_job("app-update")?;
    let result: Result<(), String> = async {
        let mut update = store
            .pending
            .lock()
            .map_err(|_| "Update lock")?
            .clone()
            .ok_or("Kiểm tra phiên bản mới trước khi cập nhật.")?;
        if update.version != expected_version {
            return Err("Phiên bản đã thay đổi. Kiểm tra lại trước khi cài.".into());
        }
        update.timeout = Some(Duration::from_secs(600));
        {
            let mut view = store.view.lock().map_err(|_| "Update lock")?;
            view.status = "downloading".into();
            view.error = None;
            view.downloaded = 0;
        }
        state.progress(id, "Đang tải và xác minh bản cập nhật…".into());
        signal(&app, id);
        let mut downloaded = 0u64;
        let mut last = Instant::now();
        let bytes = update
            .download(
                |chunk, total| {
                    downloaded += chunk as u64;
                    if let Ok(mut view) = store.view.lock() {
                        view.downloaded = downloaded;
                        view.total = total;
                    }
                    if last.elapsed() >= Duration::from_millis(200) {
                        signal(&app, id);
                        last = Instant::now();
                    }
                },
                || {},
            )
            .await
            .map_err(|e| e.to_string())?;
        // Download verifies the signature before any proxy state is changed.
        state.prepare_update_install(id)?;
        store.view.lock().map_err(|_| "Update lock")?.status = "installing".into();
        state.progress(id, "Đã xác minh. Đang mở bộ cài NetBridge…".into());
        signal(&app, id);
        tauri::async_runtime::spawn_blocking(move || {
            update.install(bytes).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(())
    }
    .await;
    if let Err(ref error) = result {
        state.abort_shutdown();
        fail(&store, error.clone());
    }
    state.finish(id, result.clone());
    signal(&app, id);
    result
}

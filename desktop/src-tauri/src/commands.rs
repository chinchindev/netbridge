use crate::service::{AppState, Snapshot, SpeedResult, TargetResult};
use netbridge::{
    adb_manager,
    config::Config,
    devices::Device,
    speed,
    speed_view::{self, Preferences, SpeedUnit},
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter, State};
fn signal(app: &AppHandle, id: u64) {
    let _ = app.emit("netbridge://state", id);
}
fn launch<F>(app: AppHandle, state: Arc<AppState>, kind: &str, work: F) -> Result<u64, String>
where
    F: FnOnce(&AppHandle, &AppState, u64, Arc<AtomicBool>) -> Result<(), String> + Send + 'static,
{
    let (id, cancel) = state.begin_job(kind)?;
    signal(&app, id);
    tauri::async_runtime::spawn_blocking(move || {
        let result = work(&app, &state, id, cancel);
        state.finish(id, result);
        signal(&app, id);
    });
    Ok(id)
}
#[tauri::command]
pub async fn get_snapshot(state: State<'_, Arc<AppState>>) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.snapshot())
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn save_config(
    state: State<'_, Arc<AppState>>,
    config: Config,
    expected_revision: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.save_profile(config, &expected_revision))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn reload_config(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.reload_profile())
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn start_routing(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.start_session())
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn stop_routing(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .session
            .lock()
            .map_err(|_| "Session lock".to_string())?
            .stop()
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn try_rule(config: Config, host: String) -> Result<serde_json::Value, String> {
    config.validate().map_err(|e| e.to_string())?;
    let host = host.trim().to_ascii_lowercase();
    if !netbridge::policy::valid_host(&host) {
        return Err("Nhập hostname hoặc IPv4 hợp lệ.".into());
    }
    Ok(
        serde_json::json!({"route":config.select(&host),"ruleIndex":config.rules.iter().position(|r|netbridge::policy::matches(&r.pattern,&host))}),
    )
}
#[tauri::command]
pub async fn get_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<Device>, String> {
    let path = {
        let session = state.session.lock().map_err(|_| "Session lock")?;
        if session.preview {
            return Ok(vec![]);
        }
        session.config().adb_path
    };
    tauri::async_runtime::spawn_blocking(move || {
        netbridge::devices::list(&path)
            .map(|items| items.into_iter().filter(|d| d.usb).collect())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbInfo {
    version: String,
    path: String,
    bundled: bool,
}
#[tauri::command]
pub async fn get_adb(state: State<'_, Arc<AppState>>) -> Result<AdbInfo, String> {
    if state.session.lock().map_err(|_| "Session lock")?.preview {
        return Ok(AdbInfo {
            version: adb_manager::bundled_release()
                .map_err(|e| e.to_string())?
                .version,
            path: "Chế độ xem thử".into(),
            bundled: true,
        });
    }
    tauri::async_runtime::spawn_blocking(move || {
        adb_manager::ensure_bundled()
            .map(|i| AdbInfo {
                version: i.version,
                path: i.path.display().to_string(),
                bundled: true,
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn tether(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    serial: String,
    connect: bool,
) -> Result<u64, String> {
    launch(
        app,
        state.inner().clone(),
        "usb",
        move |app, state, id, _| {
            let cfg = state.session.lock().map_err(|_| "Session lock")?.config();
            state.progress(
                id,
                if connect {
                    "Đang kết nối Ethernet USB…"
                } else {
                    "Đang ngắt Ethernet USB…"
                }
                .into(),
            );
            signal(app, id);
            let message = netbridge::devices::set_tether(&cfg.adb_path, &serial, connect)
                .map_err(|e| e.to_string())?;
            state.progress(id, message);
            Ok(())
        },
    )
}
#[tauri::command]
pub fn adb_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    install: bool,
) -> Result<u64, String> {
    launch(
        app,
        state.inner().clone(),
        "adb",
        move |app, state, id, _| {
            if install {
                let installed = adb_manager::update(&|message| {
                    state.progress(id, message);
                    signal(app, id);
                })
                .map_err(|e| e.to_string())?;
                let logs = state
                    .session
                    .lock()
                    .map_err(|_| "Session lock")?
                    .logs
                    .clone();
                netbridge::proxy::log(&logs, format!("ADB {} đã sẵn sàng.", installed.version));
            } else {
                state.progress(id, "Đang kiểm tra Google stable…".into());
                signal(app, id);
                let release = adb_manager::check_update().map_err(|e| e.to_string())?;
                *state.adb_latest.lock().map_err(|_| "ADB lock")? = Some(release.clone());
                let logs = state
                    .session
                    .lock()
                    .map_err(|_| "Session lock")?
                    .logs
                    .clone();
                netbridge::proxy::log(
                    &logs,
                    format!("Bản ADB ổn định mới nhất: {}.", release.version),
                );
                state.progress(id, format!("Google stable: {}", release.version));
                signal(app, id);
            }
            Ok(())
        },
    )
}
#[tauri::command]
pub fn check_targets(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<u64, String> {
    launch(
        app,
        state.inner().clone(),
        "check",
        move |app, state, id, cancel| {
            let cfg = state.session.lock().map_err(|_| "Session lock")?.config();
            state.targets.lock().map_err(|_| "Target lock")?.clear();
            let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            for (name, host, port) in [
                ("Git ETC", "git.etc.vn", 443),
                ("SRS", "10.3.6.5", 12443),
                ("Orca", "www.onorca.dev", 443),
                ("MQTT", "103.122.82.124", 1883),
            ] {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                state.progress(id, format!("Kiểm tra {name}…"));
                signal(app, id);
                let result = runtime.block_on(netbridge::dial::connect(&cfg, host, port));
                let (ok, detail) = match result {
                    Ok((socket, _)) => (
                        true,
                        format!(
                            "TCP đạt · nguồn {}",
                            socket.local_addr().map_err(|e| e.to_string())?
                        ),
                    ),
                    Err(error) => (false, error.to_string()),
                };
                let checked_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string();
                state
                    .targets
                    .lock()
                    .map_err(|_| "Target lock")?
                    .push(TargetResult {
                        name: name.into(),
                        host: host.into(),
                        port,
                        route: cfg.select(host),
                        ok,
                        detail,
                        checked_at,
                    });
                signal(app, id);
            }
            Ok(())
        },
    )
}
#[tauri::command]
pub fn measure_network(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    adapter_id: String,
) -> Result<u64, String> {
    launch(
        app,
        state.inner().clone(),
        "speed",
        move |app, state, id, cancel| {
            let targets: Vec<_> = netbridge::adapters::enumerate()
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|a| adapter_id == "all" || a.id == adapter_id)
                .collect();
            if targets.is_empty() {
                return Err("Adapter đã mất kết nối. Làm mới danh sách rồi thử lại.".into());
            }
            state.speeds.lock().map_err(|_| "Speed lock")?.clear();
            for adapter in targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let row = speed::measure_adapter(&adapter, &cancel, &|phase| {
                    state.progress(id, phase);
                    signal(app, id);
                });
                let rating = speed_view::assess(&row);
                state
                    .speeds
                    .lock()
                    .map_err(|_| "Speed lock")?
                    .push(SpeedResult {
                        adapter: row.adapter,
                        source_ip: row.source_ip.to_string(),
                        download_mbps: row.download_mbps,
                        upload_mbps: row.upload_mbps,
                        latency_ms: row.latency_ms,
                        grade: rating.grade.label().into(),
                        explanation: rating.explanation,
                        errors: row.errors,
                    });
                signal(app, id);
            }
            Ok(())
        },
    )
}
#[tauri::command]
pub fn cancel_job(state: State<'_, Arc<AppState>>, job_id: u64) -> Result<(), String> {
    state
        .session
        .lock()
        .map_err(|_| "Session lock")?
        .ensure_live()?;
    let mut job = state.job.lock().map_err(|_| "Job lock")?;
    if job.id != job_id || !job.active {
        return Err("Tác vụ này đã kết thúc.".into());
    }
    state
        .cancel
        .lock()
        .map_err(|_| "Cancel lock")?
        .store(true, Ordering::Relaxed);
    job.cancelling = true;
    job.phase = "Đang dừng, chờ yêu cầu hiện tại hoàn tất…".into();
    Ok(())
}
#[tauri::command]
pub fn set_unit(state: State<'_, Arc<AppState>>, unit: SpeedUnit) -> Result<(), String> {
    state
        .session
        .lock()
        .map_err(|_| "Session lock")?
        .ensure_live()?;
    Preferences { unit }
        .save(&netbridge::config::data_dir().join("toolbox-preferences.json"))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn exit_app(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state = state.inner().clone();
    state.begin_shutdown()?;
    if !state.session.lock().map_err(|_| "Session lock")?.preview {
        let owned = state.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            owned
                .session
                .lock()
                .map_err(|_| "Session lock".to_string())?
                .stop()
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|value| value);
        if let Err(error) = outcome {
            state.abort_shutdown();
            return Err(error);
        }
    }
    state.quit_allowed.store(true, Ordering::Relaxed);
    app.exit(0);
    Ok(())
}

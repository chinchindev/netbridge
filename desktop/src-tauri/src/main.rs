#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod commands;
mod service;
mod updates;
use service::{AppState, Session};
use std::sync::{atomic::Ordering, Arc};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--watchdog") {
        if let (Some(pid), Some(path)) = (args.get(1).and_then(|v| v.parse().ok()), args.get(2)) {
            let _ = netbridge::system_proxy::watchdog_for(
                pid,
                std::path::Path::new(path),
                args.get(3).and_then(|v| v.parse().ok()),
            );
        }
        return;
    }
    let preview = args.iter().any(|arg| arg == "--preview");
    let session = match Session::new(preview, netbridge::config::data_dir().join("config.json")) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };
    if !preview {
        if let Err(error) = netbridge::system_proxy::recover(&session.backup) {
            netbridge::proxy::log(&session.logs, error.to_string());
        }
    }
    let state = Arc::new(AppState::new(session));
    let mut context = tauri::generate_context!();
    if preview {
        context.config_mut().identifier.push_str(".preview");
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(state.clone())
        .manage(Arc::new(updates::UpdateStore::default()))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::save_config,
            commands::reload_config,
            commands::start_routing,
            commands::stop_routing,
            commands::try_rule,
            commands::get_devices,
            commands::get_adb,
            commands::tether,
            commands::adb_update,
            commands::check_targets,
            commands::measure_network,
            commands::cancel_job,
            commands::set_unit,
            commands::exit_app,
            updates::get_app_update,
            updates::check_app_update,
            updates::install_app_update
        ])
        .setup(|app| {
            let open = MenuItem::with_id(app, "open", "Mở NetBridge", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "Tắt phân luồng", true, None::<&str>)?;
            let exit =
                MenuItem::with_id(app, "exit", "Thoát và khôi phục proxy", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &stop, &exit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("App icon").clone())
                .tooltip("NetBridge · Quản lý kết nối")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "open" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                        return;
                    }
                    let state = app.state::<Arc<AppState>>().inner().clone();
                    let app = app.clone();
                    let exit = event.id.as_ref() == "exit";
                    tauri::async_runtime::spawn_blocking(move || {
                        if exit {
                            if let Err(error) = state.begin_shutdown() {
                                let _ = app.emit("netbridge://error", error);
                                if let Some(window) = app.get_webview_window("main") {
                                    let _ = window.show();
                                }
                                return;
                            }
                        }
                        let mut session = state.session.lock().unwrap_or_else(|e| e.into_inner());
                        let result = if session.preview {
                            Ok(())
                        } else {
                            session.stop()
                        };
                        drop(session);
                        if let Err(error) = result {
                            if exit {
                                state.abort_shutdown();
                            }
                            let _ = app.emit("netbridge://error", error);
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                            }
                        } else if exit {
                            state.quit_allowed.store(true, Ordering::Relaxed);
                            app.exit(0);
                        } else {
                            let _ = app.emit("netbridge://state", 0u64);
                        }
                    });
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<Arc<AppState>>().hide_on_close() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(context)
        .expect("Could not start NetBridge");
    app.run(move |app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !state.quit_allowed.load(Ordering::Relaxed) && state.hide_on_close() {
                api.prevent_exit();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
        }
    });
}

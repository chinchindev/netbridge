fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_snapshot",
            "save_config",
            "reload_config",
            "start_routing",
            "stop_routing",
            "try_rule",
            "get_devices",
            "get_adb",
            "tether",
            "adb_update",
            "check_targets",
            "measure_network",
            "cancel_job",
            "set_unit",
            "exit_app",
            "get_app_update",
            "check_app_update",
            "install_app_update",
        ]),
    ))
    .expect("Tauri build configuration");
}

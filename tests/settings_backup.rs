use netbridge::{
    config::{CheckTarget, Config, Rule},
    policy::Route,
    settings_backup::{restore_files, SettingsBackup},
    speed_view::{Preferences, SpeedUnit},
};
use std::{fs, path::PathBuf};
fn temp() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/backup-tests")
        .join(netbridge::storage::stamp());
    fs::create_dir_all(&p).unwrap();
    p
}
fn sample() -> SettingsBackup {
    let config = Config {
        rules: vec![Rule {
            pattern: "portal.example.test".into(),
            route: Route::Lan,
        }],
        check_targets: vec![CheckTarget {
            name: "Portal".into(),
            host: "portal.example.test".into(),
            port: 443,
        }],
        ..Config::default()
    };
    SettingsBackup::new(
        config,
        Preferences {
            unit: SpeedUnit::Megabytes,
        },
    )
}
#[test]
fn backup_roundtrip_contains_only_settings() {
    let b = sample();
    let text = b.to_json().unwrap();
    let parsed = SettingsBackup::parse(text.as_bytes()).unwrap();
    assert_eq!(parsed.config.rules[0].pattern, "portal.example.test");
    assert_eq!(parsed.preferences.unit, SpeedUnit::Megabytes);
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json.get("proxy-backup").is_none());
    assert!(json.get("logs").is_none());
}
#[test]
fn invalid_schema_size_unit_and_target_are_rejected() {
    let good: serde_json::Value = serde_json::from_str(&sample().to_json().unwrap()).unwrap();
    for (field, value) in [
        ("version", serde_json::json!(99)),
        ("format", serde_json::json!("foreign")),
        ("proxyBackup", serde_json::json!({})),
    ] {
        let mut bad = good.clone();
        bad[field] = value;
        assert!(SettingsBackup::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut bad = good.clone();
    bad["preferences"]["unit"] = serde_json::json!("invalid");
    assert!(SettingsBackup::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut bad = good;
    bad["config"]["check_targets"][0]["host"] = serde_json::json!("https://example.test/path");
    assert!(SettingsBackup::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    assert!(SettingsBackup::parse(&vec![b' '; 512 * 1024 + 1]).is_err());
}
#[test]
fn legacy_config_keeps_user_rules_and_has_no_new_targets() {
    let old = serde_json::json!({"port":17890,"lan_id":"","usb_id":"","adb_path":"auto","rules":[{"pattern":"private.example.test","route":"lan"}]});
    let config: Config = serde_json::from_value(old).unwrap();
    assert_eq!(config.rules[0].pattern, "private.example.test");
    assert!(config.check_targets.is_empty());
}
#[test]
fn duplicate_check_targets_are_rejected_even_with_case_changes() {
    let mut backup = sample();
    let mut duplicate = backup.config.check_targets[0].clone();
    duplicate.host = duplicate.host.to_ascii_uppercase();
    backup.config.check_targets.push(duplicate);
    assert!(backup.validate().is_err());
}
#[test]
fn restore_preserves_recovery_copy_and_restores_both_files() {
    let p = temp();
    let cfg = p.join("config.json");
    let prefs = p.join("toolbox-preferences.json");
    Config::default().save(&cfg).unwrap();
    Preferences::default().save(&prefs).unwrap();
    let before = fs::read(&cfg).unwrap();
    let recovery = restore_files(&cfg, &prefs, &sample()).unwrap();
    assert_eq!(fs::read(recovery.join("config.json")).unwrap(), before);
    assert_eq!(Config::load(&cfg).unwrap().rules.len(), 1);
    assert_eq!(
        Preferences::load(&prefs).unwrap().unit,
        SpeedUnit::Megabytes
    );
}
#[test]
fn invalid_import_does_not_touch_saved_files() {
    let p = temp();
    let cfg = p.join("config.json");
    let prefs = p.join("toolbox-preferences.json");
    Config::default().save(&cfg).unwrap();
    Preferences::default().save(&prefs).unwrap();
    let before = fs::read(&cfg).unwrap();
    let mut bad = sample();
    bad.config.port = 0;
    assert!(restore_files(&cfg, &prefs, &bad).is_err());
    assert_eq!(fs::read(&cfg).unwrap(), before);
}
#[cfg(windows)]
#[test]
fn config_write_failure_rolls_back_preferences() {
    let p = temp();
    let cfg = p.join("config.json");
    let prefs = p.join("toolbox-preferences.json");
    Config::default().save(&cfg).unwrap();
    Preferences::default().save(&prefs).unwrap();
    let before = fs::read(&prefs).unwrap();
    let mut permissions = fs::metadata(&cfg).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&cfg, permissions).unwrap();
    assert!(restore_files(&cfg, &prefs, &sample()).is_err());
    assert_eq!(fs::read(&prefs).unwrap(), before);
}

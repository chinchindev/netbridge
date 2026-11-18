use netbridge::config::Config;
use std::{fs, path::PathBuf};
fn temp() -> PathBuf {
    temp_at(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    )
}
fn temp_at(nanos: u128) -> PathBuf {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/recovery-tests")
        .join(format!("{}-{nanos}-{serial}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir.join("config.json")
}
#[cfg(windows)]
#[test]
fn readonly_fixture_cannot_contaminate_another_test_in_the_same_clock_tick() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let readonly = temp_at(tick);
    Config::default().save(&readonly).unwrap();
    let mut permissions = fs::metadata(&readonly).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&readonly, permissions).unwrap();
    let independent = temp_at(tick);
    Config::default().save(&independent).unwrap();
    assert_ne!(readonly, independent);
    assert!(fs::metadata(readonly).unwrap().permissions().readonly());
}
#[test]
fn saving_over_corrupt_config_does_not_destroy_last_good_backup() {
    let path = temp();
    let cfg = Config::default();
    cfg.save(&path).unwrap();
    cfg.save(&path).unwrap();
    let backup = fs::read(path.with_extension("json.bak")).unwrap();
    fs::write(&path, b"{partial").unwrap();
    assert!(cfg.save(&path).is_err());
    assert_eq!(fs::read(path.with_extension("json.bak")).unwrap(), backup);
    assert_eq!(fs::read(path).unwrap(), b"{partial");
}
#[test]
fn invalid_config_is_rejected_before_any_write() {
    let path = temp();
    let mut cfg = Config::default();
    cfg.save(&path).unwrap();
    let original = fs::read(&path).unwrap();
    cfg.rules = (0..200)
        .map(|_| netbridge::config::Rule {
            pattern: format!("{}.test", "a".repeat(1000)),
            route: netbridge::policy::Route::System,
        })
        .collect();
    assert!(cfg.save(&path).is_err());
    assert_eq!(fs::read(path).unwrap(), original);
}
#[test]
fn recovering_last_good_keeps_damaged_file_and_backup() {
    let path = temp();
    let cfg = Config::default();
    cfg.save(&path).unwrap();
    cfg.save(&path).unwrap();
    fs::write(&path, b"{unfinished").unwrap();
    let before = fs::read(path.with_extension("json.bak")).unwrap();
    let (restored, preserved) = Config::recover_last_good(&path).unwrap();
    assert_eq!(restored.port, 17890);
    assert_eq!(fs::read(preserved.unwrap()).unwrap(), b"{unfinished");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(path.with_extension("json.bak")).unwrap(), before);
}
#[test]
fn oversized_config_load_is_bounded() {
    let path = temp();
    fs::write(&path, vec![b' '; Config::MAX_BYTES + 1]).unwrap();
    assert!(Config::load(&path).is_err());
}
#[cfg(windows)]
#[test]
fn failed_replace_never_truncates_existing_config() {
    let path = temp();
    Config::default().save(&path).unwrap();
    let before = fs::read(&path).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    assert!(Config {
        port: 18001,
        ..Config::default()
    }
    .save(&path)
    .is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

use netbridge::{
    adb_manager::{self, Release, Version},
    speed,
};
use std::{io::Write, path::PathBuf, str::FromStr, time::Duration};

fn test_dir(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/qa-tests")
        .join(format!("{name}-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn package(version: &str, channel: &str, host: &str, url: &str) -> String {
    let p: Vec<_> = version.split('.').collect();
    format!(
        r#"<remotePackage path="platform-tools"><revision><major>{}</major><minor>{}</minor><micro>{}</micro></revision><channelRef ref="{channel}"/><archives><archive><host-os>{host}</host-os><complete><size>100</size><checksum type="sha1">0123456789abcdef0123456789abcdef01234567</checksum><url>{url}</url></complete></archive></archives></remotePackage>"#,
        p[0], p[1], p[2]
    )
}

#[test]
fn updater_selects_highest_stable_windows_package_not_preview_or_linux() {
    let xml = format!(
        "<repository>{}{}{}{}</repository>",
        package(
            "38.0.0",
            "channel-2",
            "windows",
            "platform-tools_r38.0.0-win.zip"
        ),
        package(
            "37.0.1",
            "channel-0",
            "windows",
            "platform-tools_r37.0.1-win.zip"
        ),
        package(
            "37.0.2",
            "channel-0",
            "linux",
            "platform-tools_r37.0.2-linux.zip"
        ),
        package(
            "36.0.9",
            "channel-0",
            "windows",
            "platform-tools_r36.0.9-win.zip"
        )
    );
    let release = adb_manager::parse_repository(&xml).unwrap();
    assert_eq!(release.version, "37.0.1");
    assert_eq!(
        release.url,
        "https://dl.google.com/android/repository/platform-tools_r37.0.1-win.zip"
    );
}

#[test]
fn updater_rejects_external_urls_and_missing_stable_channel() {
    for xml in [
        package(
            "37.0.1",
            "channel-0",
            "windows",
            "https://evil.invalid/adb.zip",
        ),
        package(
            "38.0.0",
            "channel-2",
            "windows",
            "platform-tools_r38.0.0-win.zip",
        ),
    ] {
        assert!(adb_manager::parse_repository(&format!("<repository>{xml}</repository>")).is_err());
    }
}

#[test]
fn versions_compare_numerically_and_reject_preview_labels() {
    assert!(Version::from_str("37.10.0").unwrap() > Version::from_str("37.2.0").unwrap());
    for text in ["", "37.1", "../37.0.1", "37.0.2-rc1"] {
        assert!(Version::from_str(text).is_err());
    }
}

#[test]
fn archive_checksum_and_size_are_required_before_installation() {
    let data = b"abc";
    let release = Release {
        version: "37.0.1".into(),
        url: "https://dl.google.com/android/repository/platform-tools_r37.0.1-win.zip".into(),
        sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
        size: 3,
    };
    assert!(adb_manager::verify_archive(&release, data).is_ok());
    assert!(adb_manager::verify_archive(&release, b"abd").is_err());
    assert!(adb_manager::verify_archive(&release, b"abcd").is_err());
}

#[test]
fn failed_update_preserves_the_existing_active_pointer() {
    let root = test_dir("failed-update");
    std::fs::write(root.join("current.json"), b"original pointer").unwrap();
    let mut release = adb_manager::bundled_release().unwrap();
    release.sha1 = "0".repeat(40);
    assert!(adb_manager::install_release_at(&release, b"invalid", &root).is_err());
    assert_eq!(
        std::fs::read(root.join("current.json")).unwrap(),
        b"original pointer"
    );
}

#[test]
fn installer_rejects_traversal_missing_files_and_duplicate_adb() {
    for names in [
        vec!["../adb.exe"],
        vec!["platform-tools/adb.exe"],
        vec!["platform-tools/adb.exe", "platform-tools/abx.exe"],
    ] {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in names {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(b"MZ").unwrap();
        }
        let mut bytes = archive.finish().unwrap().into_inner();
        let needle = b"platform-tools/abx.exe";
        for i in 0..bytes.len().saturating_sub(needle.len()) {
            if &bytes[i..i + needle.len()] == needle {
                bytes[i + needle.len() - 5] = b'd';
            }
        }
        assert!(adb_manager::extract_adb(&bytes, &test_dir("bad-zip")).is_err());
    }
}

#[test]
fn bundled_adb_installs_offline_and_has_a_real_working_version() {
    let root = test_dir("offline-adb");
    let installed = adb_manager::install_release_at(
        &adb_manager::bundled_release().unwrap(),
        adb_manager::bundled_bytes(),
        &root,
    )
    .unwrap();
    assert!(installed.path.is_file());
    let version =
        netbridge::devices::command(installed.path.to_str().unwrap(), &["version"]).unwrap();
    assert!(version.contains("Android Debug Bridge version"));
    assert!(version.contains("37.0.1"));
    assert!(installed
        .path
        .parent()
        .unwrap()
        .join("AdbWinUsbApi.dll")
        .is_file());
}

#[test]
fn transfer_rates_and_http_latency_are_not_fabricated_for_empty_samples() {
    assert_eq!(speed::mbps(1_000_000, Duration::from_secs(1)), Some(8.0));
    assert_eq!(speed::mbps(1, Duration::ZERO), None);
    assert_eq!(speed::median_latency(&[]), None);
    assert_eq!(
        speed::median_latency(&[
            Duration::from_millis(90),
            Duration::from_millis(10),
            Duration::from_millis(20)
        ]),
        Some(20.0)
    );
}

#[test]
fn new_configs_use_embedded_adb_and_migrate_the_previous_sdk_default() {
    let mut config = netbridge::config::Config::default();
    assert_eq!(config.adb_path, "auto");
    let folder = test_dir("legacy-config");
    config.adb_path = format!(
        "{}\\Android\\Sdk\\platform-tools\\adb.exe",
        std::env::var("LOCALAPPDATA").unwrap()
    );
    config.save(&folder.join("config.json")).unwrap();
    assert_eq!(
        netbridge::config::Config::load(&folder.join("config.json"))
            .unwrap()
            .adb_path,
        "auto"
    );
    config.adb_path = "D:\\custom-tools\\adb.exe".into();
    config.save(&folder.join("custom.json")).unwrap();
    assert_eq!(
        netbridge::config::Config::load(&folder.join("custom.json"))
            .unwrap()
            .adb_path,
        "D:\\custom-tools\\adb.exe"
    );
}

#[test]
fn replacing_the_active_adb_keeps_the_previous_files_and_pointer_backup() {
    let root = test_dir("retained-adb");
    let release = adb_manager::bundled_release().unwrap();
    let first =
        adb_manager::install_release_at(&release, adb_manager::bundled_bytes(), &root).unwrap();
    let previous = std::fs::read(root.join("current.json")).unwrap();
    let second =
        adb_manager::install_release_at(&release, adb_manager::bundled_bytes(), &root).unwrap();
    assert_ne!(first.path, second.path);
    assert!(first.path.is_file());
    assert!(second.path.is_file());
    assert_ne!(std::fs::read(root.join("current.json")).unwrap(), previous);
    assert!(std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .any(
            |entry| entry.path().extension().is_some_and(|ext| ext == "bak")
                && std::fs::read(entry.path()).unwrap() == previous
        ));
}

#[test]
fn an_explicit_existing_custom_adb_path_is_preserved() {
    let file = test_dir("custom-adb").join("adb.exe");
    std::fs::write(&file, b"custom path fixture").unwrap();
    assert_eq!(adb_manager::resolve(file.to_str().unwrap()).unwrap(), file);
}

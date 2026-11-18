use netbridge::{
    speed::ResultRow,
    speed_view::{self, Grade, Preferences, SpeedUnit},
};
use std::net::Ipv4Addr;
fn row(down: Option<f64>, up: Option<f64>, latency: Option<f64>) -> ResultRow {
    ResultRow {
        adapter_id: "a".into(),
        adapter: "LAN".into(),
        source_ip: Ipv4Addr::LOCALHOST,
        download_mbps: down,
        upload_mbps: up,
        latency_ms: latency,
        errors: vec![],
    }
}
#[test]
fn rates_convert_bits_bytes_and_binary_bytes_without_changing_the_measurement() {
    for (unit, expected) in [
        (SpeedUnit::Mbps, 100.0),
        (SpeedUnit::Megabytes, 12.5),
        (SpeedUnit::Mebibytes, 11.920928955078125),
        (SpeedUnit::Kbps, 100000.0),
        (SpeedUnit::Gbps, 0.1),
    ] {
        assert!((unit.convert(100.0).unwrap() - expected).abs() < 1e-10);
    }
}
#[test]
fn invalid_rates_are_not_displayed_as_real_measurements() {
    for unit in SpeedUnit::ALL {
        for value in [f64::NAN, f64::INFINITY, -1.0] {
            assert_eq!(unit.convert(value), None);
        }
        assert_eq!(unit.convert(0.0), Some(0.0));
    }
}
#[test]
fn overall_grade_uses_the_weakest_metric_at_documented_boundaries() {
    for (down, up, latency, grade) in [
        (100.0, 20.0, 60.0, Grade::Excellent),
        (99.99, 20.0, 60.0, Grade::Good),
        (25.0, 5.0, 120.0, Grade::Good),
        (100.0, 4.99, 60.0, Grade::Fair),
        (10.0, 1.0, 200.0, Grade::Fair),
        (10.0, 1.0, 200.01, Grade::Poor),
        (9.99, 20.0, 20.0, Grade::Poor),
        (1000.0, 0.0, 0.0, Grade::Poor),
    ] {
        assert_eq!(
            speed_view::assess(&row(Some(down), Some(up), Some(latency))).grade,
            grade
        );
    }
}
#[test]
fn missing_or_failed_samples_do_not_receive_a_good_network_grade() {
    assert_eq!(
        speed_view::assess(&row(None, None, None)).grade,
        Grade::Unavailable
    );
    for sample in [
        row(Some(100.0), None, Some(10.0)),
        row(Some(f64::NAN), Some(100.0), Some(10.0)),
        row(Some(100.0), Some(100.0), Some(-1.0)),
    ] {
        assert_eq!(speed_view::assess(&sample).grade, Grade::Incomplete);
    }
    let mut failed = row(Some(1000.0), Some(1000.0), Some(10.0));
    failed.errors.push("Latency sample failed".into());
    assert_eq!(speed_view::assess(&failed).grade, Grade::Incomplete);
}
#[test]
fn the_assessment_explains_which_metric_limits_the_network() {
    let assessment = speed_view::assess(&row(Some(100.0), Some(0.5), Some(30.0)));
    assert_eq!(assessment.grade, Grade::Poor);
    assert!(assessment.explanation.contains("upload"));
}
#[test]
fn preferences_roundtrip_without_touching_the_routing_config() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/qa-tests")
        .join(format!(
            "preferences-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("config.json"), b"routing settings").unwrap();
    let path = root.join("toolbox-preferences.json");
    assert_eq!(Preferences::load(&path).unwrap().unit, SpeedUnit::Mbps);
    Preferences {
        unit: SpeedUnit::Megabytes,
    }
    .save(&path)
    .unwrap();
    assert_eq!(Preferences::load(&path).unwrap().unit, SpeedUnit::Megabytes);
    assert_eq!(
        std::fs::read(root.join("config.json")).unwrap(),
        b"routing settings"
    );
}

use netbridge::policy::{authority, matches};

#[test]
fn fresh_profile_contains_no_preconfigured_business_rules() {
    assert!(netbridge::config::Config::default().rules.is_empty());
}

#[test]
fn routes_exact_hosts_without_suffix_confusion() {
    assert!(matches("portal.example.test", "PORTAL.EXAMPLE.TEST."));
    assert!(!matches("portal.example.test", "evilportal.example.test"));
    assert!(!matches("portal.example.test", "portal.example.test.attacker.test"));
    assert!(matches("*.example.org", "www.example.org"));
    assert!(matches("*.example.org", "example.org"));
    assert!(!matches("*.example.org", "notexample.org"));
}

#[test]
fn matches_ipv4_cidr_and_validates_prefixes() {
    assert!(matches("192.0.2.0/24", "192.0.2.20"));
    assert!(!matches("192.0.2.0/24", "198.51.100.20"));
    assert!(!matches("192.0.2.0/99", "192.0.2.20"));
    assert!(matches("192.0.2.20", "192.0.2.20"));
}

#[test]
fn authority_keeps_nonstandard_https_ports() {
    assert_eq!(
        authority("192.0.2.20:12443").unwrap(),
        ("192.0.2.20".into(), 12443)
    );
    assert_eq!(
        authority("portal.example.test:443").unwrap(),
        ("portal.example.test".into(), 443)
    );
    for bad in [
        "portal.example.test",
        "a:0",
        "a:65536",
        "a:443\r\nX: z",
        "user@a:443",
        "a/path:443",
        "[::1]:443",
    ] {
        assert!(authority(bad).is_err(), "{bad}");
    }
}

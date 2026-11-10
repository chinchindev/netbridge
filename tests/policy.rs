use netbridge::policy::{authority, matches};

#[test]
fn routes_exact_hosts_without_suffix_confusion() {
    assert!(matches("git.etc.vn", "GIT.ETC.VN."));
    assert!(!matches("git.etc.vn", "evilgit.etc.vn"));
    assert!(!matches("git.etc.vn", "git.etc.vn.attacker.test"));
    assert!(matches("*.onorca.dev", "www.onorca.dev"));
    assert!(matches("*.onorca.dev", "onorca.dev"));
    assert!(!matches("*.onorca.dev", "notonorca.dev"));
}

#[test]
fn matches_ipv4_cidr_and_validates_prefixes() {
    assert!(matches("10.3.0.0/16", "10.3.6.5"));
    assert!(!matches("10.3.0.0/16", "10.4.6.5"));
    assert!(!matches("10.3.0.0/99", "10.3.6.5"));
    assert!(matches("10.3.6.5", "10.3.6.5"));
}

#[test]
fn authority_keeps_nonstandard_https_ports() {
    assert_eq!(
        authority("10.3.6.5:12443").unwrap(),
        ("10.3.6.5".into(), 12443)
    );
    assert_eq!(
        authority("git.etc.vn:443").unwrap(),
        ("git.etc.vn".into(), 443)
    );
    for bad in [
        "git.etc.vn",
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

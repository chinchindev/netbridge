use netbridge::{
    config::{Config, Rule},
    diagnostics,
    policy::Route,
};
#[test]
fn direct_and_policy_results_have_independent_outcomes() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let cfg = Config {
            rules: vec![Rule {
                pattern: "127.0.0.1".into(),
                route: Route::Usb,
            }],
            usb_id: "missing-adapter".into(),
            ..Config::default()
        };
        let (windows, policy) = diagnostics::compare(&cfg, "127.0.0.1", port).await;
        assert!(windows.ok);
        assert_eq!(windows.source_ip.as_deref(), Some("127.0.0.1"));
        assert!(!policy.ok);
        assert!(policy.detail.contains("no fallback"));
    });
}
#[cfg(windows)]
#[test]
fn predicted_windows_loopback_route_is_read_only_and_ipv4() {
    let route = diagnostics::windows_route("127.0.0.1".parse().unwrap()).unwrap();
    assert!(route.source.is_loopback());
    assert!(route.interface_index > 0);
}

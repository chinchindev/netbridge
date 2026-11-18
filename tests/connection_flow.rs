use netbridge::{
    adapters::Adapter,
    config::Config,
    connection_flow::{connect_and_route, usb_candidate, ConnectionActions},
};
struct Fake {
    calls: Vec<&'static str>,
    existing: bool,
    keep: bool,
    fail: &'static str,
}
impl Fake {
    fn step(&mut self, name: &'static str) -> Result<(), String> {
        self.calls.push(name);
        if self.fail == name {
            Err(name.into())
        } else {
            Ok(())
        }
    }
}
impl ConnectionActions for Fake {
    fn preflight(&mut self) -> Result<(), String> {
        self.step("preflight")
    }
    fn usb_enabled(&mut self) -> Result<bool, String> {
        self.step("state")?;
        Ok(self.existing)
    }
    fn enable_usb(&mut self) -> Result<(), String> {
        self.step("enable")
    }
    fn wait_for_adapter(&mut self) -> Result<(), String> {
        self.step("wait")
    }
    fn start_routing(&mut self) -> Result<(), String> {
        self.step("start")
    }
    fn must_keep_usb(&self) -> bool {
        self.keep
    }
    fn undo_usb(&mut self) -> Result<(), String> {
        self.step("undo")
    }
}
fn fake(fail: &'static str) -> Fake {
    Fake {
        calls: vec![],
        existing: false,
        keep: false,
        fail,
    }
}
#[test]
fn success_waits_for_adapter_before_proxy() {
    let mut f = fake("");
    connect_and_route(&mut f).unwrap();
    assert_eq!(
        f.calls,
        vec!["preflight", "state", "enable", "wait", "start"]
    );
}
#[test]
fn invalid_profile_never_touches_usb() {
    let mut f = fake("preflight");
    assert!(connect_and_route(&mut f).is_err());
    assert_eq!(f.calls, vec!["preflight"]);
}
#[test]
fn failed_enable_wait_or_proxy_rolls_back_new_usb() {
    for phase in ["enable", "wait", "start"] {
        let mut f = fake(phase);
        assert!(connect_and_route(&mut f).is_err());
        assert_eq!(f.calls.last(), Some(&"undo"));
    }
}
#[test]
fn existing_or_newly_owned_connections_are_preserved() {
    for existing in [false, true] {
        let mut f = fake("start");
        f.existing = existing;
        f.keep = true;
        assert!(connect_and_route(&mut f).is_err());
        assert!(!f.calls.contains(&"undo"));
        if existing {
            assert!(!f.calls.contains(&"enable"));
        }
    }
}
#[test]
fn no_implicit_fallback_to_another_usb_adapter() {
    let cfg = Config {
        usb_id: "saved".into(),
        ..Config::default()
    };
    let adapter = Adapter {
        id: "other".into(),
        name: "USB".into(),
        description: "Remote NDIS".into(),
        index: 1,
        ip: "192.0.2.1".parse().unwrap(),
        dns: vec![],
        if_type: 6,
    };
    assert!(usb_candidate(&cfg, &[adapter.clone()]).unwrap().is_none());
    let mut second = adapter.clone();
    second.id = "second".into();
    assert!(usb_candidate(&Config::default(), &[adapter, second]).is_err());
}
#[test]
fn auto_suggestions_do_not_choose_between_two_usb_adapters() {
    let mut cfg = Config::default();
    let first = Adapter {
        id: "one".into(),
        name: "USB one".into(),
        description: "Remote NDIS".into(),
        index: 1,
        ip: "192.0.2.1".parse().unwrap(),
        dns: vec![],
        if_type: 6,
    };
    let mut second = first.clone();
    second.id = "two".into();
    cfg.auto_select(&[first, second]);
    assert!(cfg.usb_id.is_empty());
}
#[test]
fn existing_ncm_network_is_not_reconfigured_or_undone() {
    let mut f = fake("start");
    f.existing = netbridge::devices::usb_network_enabled("ncm,adb");
    assert!(connect_and_route(&mut f).is_err());
    assert!(!f.calls.contains(&"enable"));
    assert!(!f.calls.contains(&"undo"));
}

use super::*;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct SimulatedWindows {
    settings: RefCell<Settings>,
    owner: OwnerState,
    identity: Owner,
    listener: Option<u32>,
    foreign: bool,
    fail_apply: Cell<bool>,
}
impl ProxyIo for SimulatedWindows {
    fn identity(&self) -> Owner {
        self.identity
    }
    fn owner_state(&self, _: Owner) -> OwnerState {
        self.owner
    }
    fn settings(&self) -> io::Result<Settings> {
        Ok(self.settings.borrow().clone())
    }
    fn apply(&self, value: &Settings) -> io::Result<()> {
        *self.settings.borrow_mut() = value.clone();
        if self.fail_apply.replace(false) {
            Err(io::Error::other("Simulated partial Windows apply failure"))
        } else {
            Ok(())
        }
    }
    fn listener(&self, _: &Settings) -> io::Result<Option<u32>> {
        Ok(self.listener)
    }
    fn foreign_session(&self, _: &Settings) -> io::Result<bool> {
        Ok(self.foreign)
    }
}
fn fixture(owner: OwnerState) -> (PathBuf, Backup, SimulatedWindows) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/qa-tests")
        .join(format!(
            "session-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
        .join("proxy-backup.json");
    let backup = Backup {
        active: true,
        owner_pid: 99,
        owner_created: Some(990),
        original: Settings {
            flags: 9,
            server: "127.0.0.1:5559".into(),
            bypass: "*.local".into(),
            pac_url: String::new(),
        },
        applied: Settings {
            flags: 3,
            server: "127.0.0.1:17890".into(),
            bypass: "localhost;127.0.0.1;[::1]".into(),
            pac_url: String::new(),
        },
    };
    save(&path, &backup).unwrap();
    let env = SimulatedWindows {
        settings: RefCell::new(backup.applied.clone()),
        owner,
        identity: Owner {
            pid: 7,
            created: Some(70),
        },
        listener: None,
        foreign: false,
        fail_apply: Cell::new(false),
    };
    (path, backup, env)
}
#[test]
fn recovery_does_not_touch_another_live_or_unverifiable_owner() {
    for state in [OwnerState::Alive, OwnerState::Unknown] {
        let (path, backup, env) = fixture(state);
        let before = fs::read(&path).unwrap();
        assert!(!restore_with(&path, None, false, &env).unwrap());
        assert_eq!(env.settings().unwrap(), backup.applied);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
#[test]
fn legacy_live_snapshot_is_protected_without_creation_time() {
    let (path, mut backup, env) = fixture(OwnerState::Alive);
    backup.owner_created = None;
    save(&path, &backup).unwrap();
    assert!(!restore_with(&path, None, false, &env).unwrap());
    assert_eq!(env.settings().unwrap(), backup.applied);
}
#[test]
fn recovery_cannot_treat_the_current_pid_as_permission_to_restore() {
    let (path, mut backup, env) = fixture(OwnerState::Alive);
    backup.owner_pid = 7;
    backup.owner_created = Some(70);
    save(&path, &backup).unwrap();
    assert!(!restore_with(&path, None, false, &env).unwrap());
    assert_eq!(env.settings().unwrap(), backup.applied);
}
#[test]
fn explicit_stop_by_the_matching_owner_restores_the_original_settings() {
    let (path, mut backup, env) = fixture(OwnerState::Alive);
    backup.owner_pid = 7;
    backup.owner_created = Some(70);
    save(&path, &backup).unwrap();
    assert!(restore_with(&path, None, true, &env).unwrap());
    assert_eq!(env.settings().unwrap(), backup.original);
    assert!(
        !serde_json::from_slice::<Backup>(&fs::read(path).unwrap())
            .unwrap()
            .active
    );
}
#[test]
fn a_terminated_or_reused_process_identity_can_be_recovered() {
    let (path, backup, env) = fixture(OwnerState::Gone);
    assert!(restore_with(&path, None, false, &env).unwrap());
    assert_eq!(env.settings().unwrap(), backup.original);
}
#[test]
fn an_old_watchdog_cannot_restore_a_new_generation_even_if_pid_matches() {
    let (path, backup, env) = fixture(OwnerState::Gone);
    let before = fs::read(&path).unwrap();
    assert!(!restore_with(
        &path,
        Some(Owner {
            pid: 99,
            created: Some(980)
        }),
        false,
        &env
    )
    .unwrap());
    assert_eq!(env.settings().unwrap(), backup.applied);
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn an_orphan_snapshot_cannot_disable_a_different_live_listener() {
    let (path, backup, mut env) = fixture(OwnerState::Gone);
    env.listener = Some(101);
    assert!(!restore_with(&path, None, false, &env).unwrap());
    assert_eq!(env.settings().unwrap(), backup.applied);
}
#[test]
fn settings_changed_by_another_application_are_preserved() {
    let (path, _, env) = fixture(OwnerState::Gone);
    let user = Settings {
        flags: 1,
        server: "user-choice".into(),
        bypass: String::new(),
        pac_url: String::new(),
    };
    *env.settings.borrow_mut() = user.clone();
    assert!(!restore_with(&path, None, false, &env).unwrap());
    assert_eq!(env.settings().unwrap(), user);
    assert!(
        !serde_json::from_slice::<Backup>(&fs::read(path).unwrap())
            .unwrap()
            .active
    );
}
#[test]
fn enable_refuses_to_replace_another_active_session_or_snapshot() {
    for foreign in [false, true] {
        let (path, backup, mut env) = fixture(OwnerState::Alive);
        env.foreign = foreign;
        let before = fs::read(&path).unwrap();
        assert!(enable_with(&path, 17891, &env).is_err());
        assert_eq!(env.settings().unwrap(), backup.applied);
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

#[test]
fn another_profile_cannot_claim_proxy_while_a_foreign_session_is_active() {
    let (path, backup, mut env) = fixture(OwnerState::Alive);
    env.foreign = true;
    let separate = path.with_file_name("other-profile.json");
    assert!(enable_with(&separate, 17891, &env).is_err());
    assert!(!separate.exists());
    assert_eq!(env.settings().unwrap(), backup.applied);
}

#[test]
fn partial_apply_failure_rolls_back_and_marks_the_new_snapshot_inactive() {
    let (path, backup, env) = fixture(OwnerState::Gone);
    *env.settings.borrow_mut() = backup.original.clone();
    env.fail_apply.set(true);
    let separate = path.with_file_name("failed-enable.json");
    assert!(enable_with(&separate, 17891, &env).is_err());
    assert_eq!(env.settings().unwrap(), backup.original);
    assert!(!load(&separate).unwrap().unwrap().active);
}

#[cfg(windows)]
#[test]
fn windows_owner_probe_distinguishes_live_identity_from_reused_pid() {
    let identity = WindowsProxy.identity();
    assert!(identity.created.is_some());
    assert_eq!(WindowsProxy.owner_state(identity), OwnerState::Alive);
    assert_eq!(
        WindowsProxy.owner_state(Owner {
            pid: identity.pid,
            created: identity.created.map(|time| time + 1)
        }),
        OwnerState::Gone
    );
    assert_eq!(
        WindowsProxy.owner_state(Owner {
            pid: 0,
            created: None
        }),
        OwnerState::Unknown
    );
}

#[cfg(windows)]
#[test]
fn windows_listener_probe_reports_the_actual_socket_owner() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let settings = Settings {
        flags: 3,
        server: format!("127.0.0.1:{}", listener.local_addr().unwrap().port()),
        bypass: String::new(),
        pac_url: String::new(),
    };
    assert_eq!(
        WindowsProxy.listener(&settings).unwrap(),
        Some(std::process::id())
    );
}

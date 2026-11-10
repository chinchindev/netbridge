use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};
#[cfg(windows)]
#[path = "proxy_windows.rs"]
mod platform;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub flags: u32,
    pub server: String,
    pub bypass: String,
    pub pac_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Backup {
    active: bool,
    #[serde(default)]
    owner_pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_created: Option<u64>,
    original: Settings,
    applied: Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Owner {
    pid: u32,
    created: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerState {
    Alive,
    Gone,
    Unknown,
}
trait ProxyIo {
    fn identity(&self) -> Owner;
    fn owner_state(&self, owner: Owner) -> OwnerState;
    fn settings(&self) -> io::Result<Settings>;
    fn apply(&self, settings: &Settings) -> io::Result<()>;
    fn listener(&self, settings: &Settings) -> io::Result<Option<u32>>;
    fn foreign_session(&self, settings: &Settings) -> io::Result<bool>;
}
struct WindowsProxy;
impl ProxyIo for WindowsProxy {
    fn identity(&self) -> Owner {
        #[cfg(windows)]
        {
            platform::identity()
        }
        #[cfg(not(windows))]
        {
            Owner {
                pid: std::process::id(),
                created: None,
            }
        }
    }
    fn owner_state(&self, owner: Owner) -> OwnerState {
        #[cfg(windows)]
        {
            platform::owner_state(owner)
        }
        #[cfg(not(windows))]
        {
            let _ = owner;
            OwnerState::Unknown
        }
    }
    fn settings(&self) -> io::Result<Settings> {
        read()
    }
    fn apply(&self, settings: &Settings) -> io::Result<()> {
        apply(settings)
    }
    fn listener(&self, settings: &Settings) -> io::Result<Option<u32>> {
        #[cfg(windows)]
        {
            platform::listener(settings)
        }
        #[cfg(not(windows))]
        {
            let _ = settings;
            Ok(None)
        }
    }
    fn foreign_session(&self, settings: &Settings) -> io::Result<bool> {
        #[cfg(windows)]
        {
            platform::foreign_session(settings)
        }
        #[cfg(not(windows))]
        {
            let _ = settings;
            Ok(false)
        }
    }
}

#[cfg(windows)]
pub fn read() -> io::Result<Settings> {
    use std::{mem, ptr};
    use windows_sys::Win32::{Foundation::GlobalFree, Networking::WinInet::*};
    unsafe {
        let mut options = [
            INTERNET_PER_CONN_FLAGS,
            INTERNET_PER_CONN_PROXY_SERVER,
            INTERNET_PER_CONN_PROXY_BYPASS,
            INTERNET_PER_CONN_AUTOCONFIG_URL,
        ]
        .map(|dw_option| INTERNET_PER_CONN_OPTIONW {
            dwOption: dw_option,
            Value: INTERNET_PER_CONN_OPTIONW_0 { dwValue: 0 },
        });
        let mut list = INTERNET_PER_CONN_OPTION_LISTW {
            dwSize: mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
            pszConnection: ptr::null_mut(),
            dwOptionCount: 4,
            dwOptionError: 0,
            pOptions: options.as_mut_ptr(),
        };
        let mut size = list.dwSize;
        if InternetQueryOptionW(
            ptr::null_mut(),
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
            &mut size,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut values = Vec::new();
        for option in options.iter().skip(1) {
            let p = option.Value.pszValue;
            if p.is_null() {
                values.push(String::new());
                continue;
            }
            let n = (0..65536).find(|i| *p.add(*i) == 0).unwrap_or(65536);
            values.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
            GlobalFree(p.cast());
        }
        Ok(Settings {
            flags: options[0].Value.dwValue,
            server: values[0].clone(),
            bypass: values[1].clone(),
            pac_url: values[2].clone(),
        })
    }
}

#[cfg(windows)]
fn apply(settings: &Settings) -> io::Result<()> {
    use std::{mem, ptr};
    use windows_sys::Win32::Networking::WinInet::*;
    let mut strings = [&settings.server, &settings.bypass, &settings.pac_url]
        .map(|s| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    let mut options = [
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_FLAGS,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                dwValue: settings.flags,
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_PROXY_SERVER,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: strings[0].as_mut_ptr(),
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_PROXY_BYPASS,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: strings[1].as_mut_ptr(),
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_AUTOCONFIG_URL,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: strings[2].as_mut_ptr(),
            },
        },
    ];
    let mut list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        pszConnection: ptr::null_mut(),
        dwOptionCount: 4,
        dwOptionError: 0,
        pOptions: options.as_mut_ptr(),
    };
    unsafe {
        if InternetSetOptionW(
            ptr::null_mut(),
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
            list.dwSize,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        InternetSetOptionW(
            ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            ptr::null_mut(),
            0,
        );
        InternetSetOptionW(ptr::null_mut(), INTERNET_OPTION_REFRESH, ptr::null_mut(), 0);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn read() -> io::Result<Settings> {
    Err(io::Error::other("Windows only"))
}
#[cfg(not(windows))]
fn apply(_: &Settings) -> io::Result<()> {
    Err(io::Error::other("Windows only"))
}

fn save(path: &Path, backup: &Backup) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let token = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let temporary = path.with_extension(format!("{token}.tmp"));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    use io::Write;
    file.write_all(&serde_json::to_vec_pretty(backup).map_err(io::Error::other)?)?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        fs::copy(path, path.with_extension(format!("{token}.bak")))?;
    }
    #[cfg(windows)]
    {
        platform::replace_file(&temporary, path)
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, path)
    }
}

pub fn enable(path: &Path, port: u16) -> io::Result<()> {
    #[cfg(windows)]
    let _guard = platform::mutation_lock()?;
    #[cfg(windows)]
    platform::acquire_session(path)?;
    let result = enable_with(path, port, &WindowsProxy);
    #[cfg(windows)]
    if result.is_err()
        && !load(path)
            .ok()
            .flatten()
            .is_some_and(|b| b.active && b.owner_pid == std::process::id())
    {
        platform::release_session(path);
    }
    result
}
fn enable_with(path: &Path, port: u16, env: &impl ProxyIo) -> io::Result<()> {
    let identity = env.identity();
    if identity.created.is_none() {
        return Err(io::Error::other(
            "Không xác minh được danh tính phiên proxy.",
        ));
    }
    if env.foreign_session(&env.settings()?)? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Phiên NetBridge khác đang phân luồng; không thay đổi proxy.",
        ));
    }
    if let Some(backup) = load(path)? {
        if backup.active
            && env.owner_state(Owner {
                pid: backup.owner_pid,
                created: backup.owner_created,
            }) != OwnerState::Gone
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Snapshot thuộc phiên còn sống hoặc chưa xác minh được.",
            ));
        }
        restore_with(path, None, false, env)?;
        if load(path)?.is_some_and(|backup| backup.active) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Không thể chiếm quyền snapshot của phiên khác.",
            ));
        }
    }
    let original = env.settings()?;
    let applied = Settings {
        flags: 3,
        server: format!("127.0.0.1:{port}"),
        bypass: "localhost;127.0.0.1;[::1]".into(),
        pac_url: String::new(),
    };
    let mut backup = Backup {
        active: true,
        owner_pid: identity.pid,
        owner_created: identity.created,
        original,
        applied: applied.clone(),
    };
    save(path, &backup)?;
    let outcome = env.apply(&applied).and_then(|_| {
        if env.settings()? == applied {
            Ok(())
        } else {
            Err(io::Error::other("Proxy setting verification failed"))
        }
    });
    if let Err(error) = outcome {
        if env.apply(&backup.original).is_ok() && env.settings().is_ok_and(|s| s == backup.original)
        {
            backup.active = false;
            save(path, &backup)?;
        }
        return Err(error);
    }
    Ok(())
}

pub fn restore(path: &Path) -> io::Result<bool> {
    restore_guarded(path, None, true)
}
pub fn recover(path: &Path) -> io::Result<bool> {
    restore_guarded(path, None, false)
}
fn restore_guarded(path: &Path, expected: Option<Owner>, allow_own: bool) -> io::Result<bool> {
    #[cfg(windows)]
    let _guard = platform::mutation_lock()?;
    let result = restore_with(path, expected, allow_own, &WindowsProxy);
    #[cfg(windows)]
    if result.is_ok() && !load(path)?.is_some_and(|backup| backup.active) {
        platform::release_session(path);
    }
    result
}
fn load(path: &Path) -> io::Result<Option<Backup>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(path)?;
    if data.len() > 1024 * 1024 {
        return Err(io::Error::other("Proxy backup too large"));
    }
    serde_json::from_slice(&data)
        .map(Some)
        .map_err(io::Error::other)
}
fn restore_with(
    path: &Path,
    expected: Option<Owner>,
    allow_own: bool,
    env: &impl ProxyIo,
) -> io::Result<bool> {
    let Some(mut backup) = load(path)? else {
        return Ok(false);
    };
    if !backup.active {
        return Ok(false);
    }
    let owner = Owner {
        pid: backup.owner_pid,
        created: backup.owner_created,
    };
    if expected.is_some_and(|expected| expected != owner) {
        return Ok(false);
    }
    let own = allow_own && owner.created.is_some() && owner == env.identity();
    if !own && env.owner_state(owner) != OwnerState::Gone {
        return Ok(false);
    }
    if env.settings()? != backup.applied {
        // A newer setting belongs to the user or another app. Preserve it and relinquish ownership.
        backup.active = false;
        save(path, &backup)?;
        return Ok(false);
    }
    if env.foreign_session(&backup.applied)?
        || env
            .listener(&backup.applied)?
            .is_some_and(|pid| pid != env.identity().pid)
    {
        return Ok(false);
    }
    env.apply(&backup.original)?;
    if env.settings()? != backup.original {
        return Err(io::Error::other("Proxy restore verification failed"));
    }
    backup.active = false;
    save(path, &backup)?;
    Ok(true)
}

pub fn watchdog(parent: u32, path: &Path) -> io::Result<()> {
    watchdog_for(parent, path, None)
}

pub fn watchdog_for(parent: u32, path: &Path, created: Option<u64>) -> io::Result<()> {
    let owner = Owner {
        pid: parent,
        created,
    };
    #[cfg(windows)]
    platform::wait_owner(owner);
    // Check the expected identity and restore under the same cross-process lock.
    restore_guarded(path, Some(owner), false)?;
    Ok(())
}

pub fn spawn_watchdog(path: &Path) -> io::Result<()> {
    let created = WindowsProxy
        .identity()
        .created
        .ok_or_else(|| io::Error::other("Cannot identify watchdog owner"))?;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("--watchdog")
        .arg(std::process::id().to_string())
        .arg(path)
        .arg(created.to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn()?;
    Ok(())
}

#[cfg(test)]
#[path = "proxy_session_tests.rs"]
mod tests;

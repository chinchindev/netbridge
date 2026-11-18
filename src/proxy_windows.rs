use super::{Owner, OwnerState, Settings};
use std::{
    io,
    path::{Path, PathBuf},
    ptr,
    sync::Mutex,
};
use windows_sys::Win32::{
    Foundation::*, NetworkManagement::IpHelper::*, Networking::WinSock::AF_INET,
    Storage::FileSystem::*, System::Threading::*, UI::WindowsAndMessaging::*,
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub(super) struct MutationGuard(HANDLE);
impl Drop for MutationGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}
pub(super) fn mutation_lock() -> io::Result<MutationGuard> {
    unsafe {
        let handle = CreateMutexW(
            ptr::null(),
            0,
            wide("Local\\NetBridge.ProxyMutation").as_ptr(),
        );
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let result = WaitForSingleObject(handle, 5000);
        if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
            CloseHandle(handle);
            return Err(io::Error::other(
                "Một phiên khác đang thay đổi proxy; thử lại sau.",
            ));
        }
        Ok(MutationGuard(handle))
    }
}

// An open named object is a process lease; it is not a mutex held by a thread.
// CloseHandle can safely release this lease from any thread.
struct Session {
    handle: usize,
    path: PathBuf,
}
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle as HANDLE);
        }
    }
}
static SESSION: Mutex<Option<Session>> = Mutex::new(None);
pub(super) fn acquire_session(path: &Path) -> io::Result<()> {
    let mut session = SESSION
        .lock()
        .map_err(|_| io::Error::other("Proxy lease lock poisoned"))?;
    if session.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Tiến trình này đã có phiên proxy.",
        ));
    }
    unsafe {
        let handle = CreateMutexW(
            ptr::null(),
            0,
            wide("Local\\NetBridge.ProxySession").as_ptr(),
        );
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "NetBridge khác đang giữ phiên phân luồng.",
            ));
        }
        *session = Some(Session {
            handle: handle as usize,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}
pub(super) fn release_session(path: &Path) {
    if let Ok(mut guard) = SESSION.lock() {
        if guard.as_ref().is_some_and(|session| session.path == path) {
            guard.take();
        }
    }
}
fn foreign_lease() -> bool {
    if SESSION.lock().is_ok_and(|guard| guard.is_some()) {
        return false;
    }
    unsafe {
        let handle = OpenMutexW(
            SYNCHRONIZATION_SYNCHRONIZE,
            0,
            wide("Local\\NetBridge.ProxySession").as_ptr(),
        );
        if !handle.is_null() {
            CloseHandle(handle);
            return true;
        }
        GetLastError() != ERROR_FILE_NOT_FOUND
    }
}
fn created(handle: HANDLE) -> io::Result<u64> {
    unsafe {
        let (mut start, mut exit, mut kernel, mut user) = (
            std::mem::zeroed(),
            std::mem::zeroed(),
            std::mem::zeroed(),
            std::mem::zeroed(),
        );
        if GetProcessTimes(handle, &mut start, &mut exit, &mut kernel, &mut user) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((u64::from(start.dwHighDateTime) << 32) | u64::from(start.dwLowDateTime))
    }
}
pub(super) fn identity() -> Owner {
    Owner {
        pid: std::process::id(),
        created: unsafe { created(GetCurrentProcess()).ok() },
    }
}
pub(super) fn owner_state(owner: Owner) -> OwnerState {
    if owner.pid == 0 {
        return OwnerState::Unknown;
    }
    unsafe {
        let handle = OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            owner.pid,
        );
        if handle.is_null() {
            return if GetLastError() == ERROR_INVALID_PARAMETER {
                OwnerState::Gone
            } else {
                OwnerState::Unknown
            };
        }
        let status = WaitForSingleObject(handle, 0);
        let birth = created(handle);
        CloseHandle(handle);
        if status == WAIT_OBJECT_0 {
            return OwnerState::Gone;
        }
        if status != WAIT_TIMEOUT {
            return OwnerState::Unknown;
        }
        match birth {
            Ok(actual) if owner.created.is_some_and(|expected| expected != actual) => {
                OwnerState::Gone
            }
            Ok(_) => OwnerState::Alive,
            Err(_) => OwnerState::Unknown,
        }
    }
}
pub(super) fn wait_owner(owner: Owner) {
    unsafe {
        let handle = OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            owner.pid,
        );
        if handle.is_null() {
            return;
        }
        let matches = owner
            .created
            .is_none_or(|expected| created(handle).is_ok_and(|actual| actual == expected));
        if matches {
            WaitForSingleObject(handle, INFINITE);
        }
        CloseHandle(handle);
    }
}
fn loopback_port(settings: &Settings) -> Option<u16> {
    if settings.flags & 2 == 0 {
        return None;
    }
    settings.server.strip_prefix("127.0.0.1:")?.parse().ok()
}
pub(super) fn listener(settings: &Settings) -> io::Result<Option<u32>> {
    let Some(port) = loopback_port(settings) else {
        return Ok(None);
    };
    let mut length = 4096u32;
    for _ in 0..3 {
        let mut storage = vec![0u64; (length as usize).div_ceil(8)];
        let result = unsafe {
            GetExtendedTcpTable(
                storage.as_mut_ptr().cast(),
                &mut length,
                0,
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if result != NO_ERROR {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        unsafe {
            let table = storage.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
            let rows =
                std::slice::from_raw_parts((*table).table.as_ptr(), (*table).dwNumEntries as usize);
            return Ok(rows
                .iter()
                .find(|row| {
                    u16::from_be(row.dwLocalPort as u16) == port
                        && (row.dwLocalAddr == 0 || row.dwLocalAddr.to_ne_bytes() == [127, 0, 0, 1])
                })
                .map(|row| row.dwOwningPid));
        }
    }
    Err(io::Error::other("TCP listener table changed repeatedly"))
}
struct WindowProbe {
    pid: u32,
    found: bool,
}
unsafe extern "system" fn find_owner_window(hwnd: HWND, data: LPARAM) -> BOOL {
    let probe = &mut *(data as *mut WindowProbe);
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == probe.pid {
        let mut name = [0u16; 128];
        let length = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32);
        if String::from_utf16_lossy(&name[..length.max(0) as usize]) == "NetBridgeMainWindow" {
            probe.found = true;
            return 0;
        }
    }
    1
}
pub(super) fn foreign_session(settings: &Settings) -> io::Result<bool> {
    if foreign_lease() {
        return Ok(true);
    }
    // Legacy versions do not hold ProxySession. Recognize their active listener/window.
    if let Some(pid) = listener(settings)? {
        if pid != std::process::id() {
            let mut probe = WindowProbe { pid, found: false };
            unsafe {
                EnumWindows(
                    Some(find_owner_window),
                    (&mut probe as *mut WindowProbe) as isize,
                );
            }
            return Ok(probe.found);
        }
    }
    Ok(false)
}
pub(super) fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let source: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

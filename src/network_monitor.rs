//! Read-only interface counters and TCP endpoint inventory for the local computer.
use serde::Serialize;
use std::{
    collections::HashMap,
    io, mem,
    net::{Ipv4Addr, Ipv6Addr, SocketAddrV6},
    ptr, slice,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, INVALID_HANDLE_VALUE},
    NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
    Networking::WinSock::{AF_INET, AF_INET6},
    System::Diagnostics::ToolHelp::*,
};

#[derive(Serialize)]
pub struct Interface {
    pub id: String,
    pub name: String,
    pub received: String,
    pub sent: String,
}
#[derive(Serialize)]
pub struct Endpoint {
    pub pid: u32,
    pub process: String,
    pub local: String,
    pub remote: Option<String>,
    pub state: &'static str,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub interfaces: Vec<Interface>,
    pub endpoints: Vec<Endpoint>,
    pub warnings: Vec<String>,
}
fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}
fn interfaces() -> io::Result<Vec<Interface>> {
    unsafe {
        let mut table = ptr::null_mut();
        let status = GetIfTable2(&mut table);
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let rows = slice::from_raw_parts(
            ptr::addr_of!((*table).Table).cast::<MIB_IF_ROW2>(),
            (*table).NumEntries as usize,
        );
        let result = rows
            .iter()
            .filter(|r| r.OperStatus == IfOperStatusUp && r.Type != 24)
            .map(|r| Interface {
                id: r.InterfaceLuid.Value.to_string(),
                name: wide(&r.Alias),
                received: r.InOctets.to_string(),
                sent: r.OutOctets.to_string(),
            })
            .collect();
        FreeMibTable(table.cast());
        Ok(result)
    }
}
fn process_names() -> io::Result<HashMap<u32, String>> {
    let mut names = HashMap::new();
    unsafe {
        let handle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut item: PROCESSENTRY32W = mem::zeroed();
        item.dwSize = mem::size_of_val(&item) as u32;
        let mut more = Process32FirstW(handle, &mut item);
        while more != 0 {
            names.insert(item.th32ProcessID, wide(&item.szExeFile));
            more = Process32NextW(handle, &mut item);
        }
        CloseHandle(handle);
    }
    Ok(names)
}
fn state_name(state: u32) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RCVD",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT1",
        7 => "FIN_WAIT2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        _ => "UNKNOWN",
    }
}
fn tcp(family: u16, names: &HashMap<u32, String>, out: &mut Vec<Endpoint>) -> io::Result<()> {
    let mut size = 4096;
    for _ in 0..4 {
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut size,
                1,
                family as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let mut push = |pid, state, local, remote| {
            out.push(Endpoint {
                pid,
                process: names.get(&pid).cloned().unwrap_or_default(),
                local,
                remote: if state == 2 { None } else { Some(remote) },
                state: state_name(state),
            })
        };
        unsafe {
            if family == AF_INET {
                let table = buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
                let rows = slice::from_raw_parts(
                    ptr::addr_of!((*table).table).cast::<MIB_TCPROW_OWNER_PID>(),
                    (*table).dwNumEntries as usize,
                );
                let addr = |ip: u32, port: u32| {
                    format!(
                        "{}:{}",
                        Ipv4Addr::from(ip.to_ne_bytes()),
                        u16::from_be(port as u16)
                    )
                };
                for r in rows {
                    push(
                        r.dwOwningPid,
                        r.dwState,
                        addr(r.dwLocalAddr, r.dwLocalPort),
                        addr(r.dwRemoteAddr, r.dwRemotePort),
                    );
                }
            } else {
                let table = buffer.as_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>();
                let rows = slice::from_raw_parts(
                    ptr::addr_of!((*table).table).cast::<MIB_TCP6ROW_OWNER_PID>(),
                    (*table).dwNumEntries as usize,
                );
                let addr = |ip: [u8; 16], port: u32, scope: u32| {
                    SocketAddrV6::new(Ipv6Addr::from(ip), u16::from_be(port as u16), 0, scope)
                        .to_string()
                };
                for r in rows {
                    push(
                        r.dwOwningPid,
                        r.dwState,
                        addr(r.ucLocalAddr, r.dwLocalPort, r.dwLocalScopeId),
                        addr(r.ucRemoteAddr, r.dwRemotePort, r.dwRemoteScopeId),
                    );
                }
            }
        }
        return Ok(());
    }
    Err(io::Error::other("TCP table changed repeatedly"))
}
pub fn snapshot() -> Snapshot {
    let mut warnings = Vec::new();
    let interfaces = interfaces().unwrap_or_else(|e| {
        warnings.push(format!("Card mạng: {e}"));
        Vec::new()
    });
    let names = process_names().unwrap_or_else(|e| {
        warnings.push(format!("Tên tiến trình: {e}"));
        HashMap::new()
    });
    let mut endpoints = Vec::new();
    for (family, label) in [(AF_INET, "IPv4"), (AF_INET6, "IPv6")] {
        if let Err(e) = tcp(family, &names, &mut endpoints) {
            warnings.push(format!("TCP {label}: {e}"));
        }
    }
    Snapshot {
        interfaces,
        endpoints,
        warnings,
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn reports_the_current_process_and_its_loopback_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let sample = super::snapshot();
        assert!(sample.endpoints.iter().any(|e| e.pid == std::process::id()
            && !e.process.is_empty()
            && e.local == addr
            && e.remote.is_none()
            && e.state == "LISTEN"));
    }
}

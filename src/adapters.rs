use serde::{Deserialize, Serialize};
use std::{io, net::Ipv4Addr};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Adapter {
    pub id: String,
    pub name: String,
    pub description: String,
    pub index: u32,
    pub ip: Ipv4Addr,
    pub dns: Vec<Ipv4Addr>,
    pub if_type: u32,
}

#[cfg(windows)]
pub fn enumerate() -> io::Result<Vec<Adapter>> {
    use std::{ffi::CStr, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR},
        NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
        Networking::WinSock::*,
    };
    unsafe fn wide(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let n = (0..2048).find(|i| *p.add(*i) == 0).unwrap_or(2048);
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }
    unsafe fn ipv4(address: SOCKET_ADDRESS) -> Option<Ipv4Addr> {
        if address.lpSockaddr.is_null()
            || address.iSockaddrLength < (std::mem::size_of::<SOCKADDR_IN>() as i32)
            || (*address.lpSockaddr).sa_family != AF_INET
        {
            return None;
        }
        let sin = &*(address.lpSockaddr as *const SOCKADDR_IN);
        Some(Ipv4Addr::from(sin.sin_addr.S_un.S_addr.to_ne_bytes()))
    }
    let mut size = 16384u32;
    for _ in 0..3 {
        // Align the buffer for all IP Helper structs, including pointer and u64 members.
        let mut storage = vec![0u64; (size as usize).div_ceil(8)];
        let first = storage.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        let code = unsafe {
            GetAdaptersAddresses(
                AF_INET as u32,
                GAA_FLAG_INCLUDE_GATEWAYS,
                ptr::null(),
                first,
                &mut size,
            )
        };
        if code == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if code != NO_ERROR {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let mut out = Vec::new();
        unsafe {
            let mut p = first;
            while !p.is_null() {
                let a = &*p;
                if a.OperStatus == IfOperStatusUp && a.IfType != 24 {
                    let mut addr = a.FirstUnicastAddress;
                    let mut ip = None;
                    while !addr.is_null() {
                        if let Some(v) = ipv4((*addr).Address) {
                            if !v.is_unspecified() && !v.is_link_local() {
                                ip = Some(v);
                                break;
                            }
                        }
                        addr = (*addr).Next;
                    }
                    if let Some(ip) = ip {
                        let mut dns = Vec::new();
                        let mut d = a.FirstDnsServerAddress;
                        while !d.is_null() {
                            if let Some(v) = ipv4((*d).Address) {
                                dns.push(v)
                            }
                            d = (*d).Next;
                        }
                        out.push(Adapter {
                            id: CStr::from_ptr(a.AdapterName.cast())
                                .to_string_lossy()
                                .into_owned(),
                            name: wide(a.FriendlyName),
                            description: wide(a.Description),
                            index: a.Anonymous1.Anonymous.IfIndex,
                            ip,
                            dns,
                            if_type: a.IfType,
                        });
                    }
                }
                p = a.Next;
            }
        }
        return Ok(out);
    }
    Err(io::Error::other(
        "Adapters changed repeatedly; refresh and retry",
    ))
}

#[cfg(not(windows))]
pub fn enumerate() -> io::Result<Vec<Adapter>> {
    Ok(Vec::new())
}

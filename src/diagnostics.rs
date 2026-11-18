use crate::{adapters, config::Config, dial, policy::Route};
use serde::Serialize;
use std::{io, net::Ipv4Addr, time::Duration};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub ok: bool,
    pub source_ip: Option<String>,
    pub predicted_source_ip: Option<String>,
    pub adapter: Option<String>,
    pub remote_ip: Option<String>,
    pub detail: String,
}
#[derive(Clone, Debug)]
pub struct WindowsRoute {
    pub source: Ipv4Addr,
    pub interface_index: u32,
}
#[cfg(windows)]
pub fn windows_route(destination: Ipv4Addr) -> io::Result<WindowsRoute> {
    use windows_sys::Win32::{
        NetworkManagement::IpHelper::{GetBestRoute2, MIB_IPFORWARD_ROW2},
        Networking::WinSock::{AF_INET, SOCKADDR_INET},
    };
    // Both SOCKADDR_INET values are IPv4 and are initialized before crossing the FFI boundary.
    unsafe {
        let mut target: SOCKADDR_INET = std::mem::zeroed();
        target.Ipv4.sin_family = AF_INET;
        target.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(destination.octets());
        let mut route: MIB_IPFORWARD_ROW2 = std::mem::zeroed();
        let mut source: SOCKADDR_INET = std::mem::zeroed();
        let status = GetBestRoute2(
            std::ptr::null(),
            0,
            std::ptr::null(),
            &target,
            0,
            &mut route,
            &mut source,
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        if source.si_family != AF_INET {
            return Err(io::Error::other("Windows did not select an IPv4 route"));
        }
        Ok(WindowsRoute {
            source: Ipv4Addr::from(source.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes()),
            interface_index: route.InterfaceIndex,
        })
    }
}
#[cfg(not(windows))]
pub fn windows_route(_: Ipv4Addr) -> io::Result<WindowsRoute> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows route query unavailable",
    ))
}

pub async fn windows_probe(host: &str, port: u16) -> Probe {
    let mut probe = Probe::default();
    let addresses = match tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    {
        Ok(Ok(values)) => values
            .filter(|address| address.is_ipv4())
            .take(3)
            .collect::<Vec<_>>(),
        Ok(Err(error)) => {
            probe.detail = format!("Windows DNS: {error}");
            return probe;
        }
        Err(_) => {
            probe.detail = "Windows DNS timed out".into();
            return probe;
        }
    };
    let available = adapters::enumerate().unwrap_or_default();
    for address in addresses {
        let std::net::IpAddr::V4(ip) = address.ip() else {
            continue;
        };
        probe.remote_ip = Some(ip.to_string());
        if let Ok(route) = windows_route(ip) {
            probe.predicted_source_ip = Some(route.source.to_string());
            probe.adapter = available
                .iter()
                .find(|a| a.index == route.interface_index)
                .map(|a| a.name.clone());
        }
        match dial::connect_ip(address, None).await {
            Ok(stream) => {
                probe.ok = true;
                probe.source_ip = stream.local_addr().ok().map(|a| a.ip().to_string());
                probe.detail = "TCP đạt qua đường Windows; không dùng proxy.".into();
                return probe;
            }
            Err(error) => probe.detail = error.to_string(),
        }
    }
    if probe.detail.is_empty() {
        probe.detail = "Windows DNS did not return an IPv4 address".into();
    }
    probe
}
pub async fn policy_probe(config: &Config, host: &str, port: u16) -> Probe {
    let mut probe = Probe::default();
    let selected = match config.select(host) {
        Route::Lan => Some(&config.lan_id),
        Route::Usb => Some(&config.usb_id),
        Route::System => None,
    };
    if let Some(id) = selected {
        if let Ok(available) = adapters::enumerate() {
            if let Some(adapter) = available.iter().find(|a| &a.id == id) {
                probe.adapter = Some(adapter.name.clone());
                probe.predicted_source_ip = Some(adapter.ip.to_string());
            }
        }
    }
    match dial::connect(config, host, port).await {
        Ok((stream, _)) => {
            probe.ok = true;
            probe.source_ip = stream.local_addr().ok().map(|a| a.ip().to_string());
            probe.remote_ip = stream.peer_addr().ok().map(|a| a.ip().to_string());
            probe.detail = "TCP đạt qua đường theo quy tắc; chưa kiểm tra TLS.".into();
        }
        Err(error) => probe.detail = error.to_string(),
    }
    probe
}
pub async fn compare(config: &Config, host: &str, port: u16) -> (Probe, Probe) {
    tokio::join!(windows_probe(host, port), policy_probe(config, host, port))
}

use crate::{adapters, config::Config, policy::Route};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{
    net::{lookup_host, TcpSocket, TcpStream},
    time::timeout,
};

pub async fn connect_ip(target: SocketAddr, source: Option<Ipv4Addr>) -> io::Result<TcpStream> {
    let socket = TcpSocket::new_v4()?;
    if let Some(ip) = source {
        socket.bind(SocketAddr::from((ip, 0)))?;
    }
    let stream = timeout(Duration::from_secs(8), socket.connect(target))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TCP timed out"))??;
    stream.set_nodelay(true)?;
    Ok(stream)
}

pub async fn connect(config: &Config, host: &str, port: u16) -> io::Result<(TcpStream, Route)> {
    let route = config.select(host);
    let (ips, source) = match route {
        Route::System => {
            let addresses = timeout(Duration::from_secs(5), lookup_host((host, port)))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "System DNS timed out"))??;
            (
                addresses
                    .filter_map(|a| {
                        if let std::net::IpAddr::V4(ip) = a.ip() {
                            Some(ip)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                None,
            )
        }
        Route::Lan | Route::Usb => {
            let id = if route == Route::Lan {
                &config.lan_id
            } else {
                &config.usb_id
            };
            let adapter = adapters::enumerate()?
                .into_iter()
                .find(|a| &a.id == id)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotConnected,
                        format!("{route:?} adapter unavailable; no fallback"),
                    )
                })?;
            (crate::dns::resolve(host, &adapter).await?, Some(adapter.ip))
        }
    };
    let mut error = io::Error::new(io::ErrorKind::NotFound, "No IPv4 address");
    for ip in ips.into_iter().take(3) {
        match connect_ip(SocketAddr::from((ip, port)), source).await {
            Ok(s) => return Ok((s, route)),
            Err(e) => error = e,
        }
    }
    Err(error)
}

use crate::adapters::Adapter;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::atomic::{AtomicU16, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::timeout,
};

static NEXT_ID: AtomicU16 = AtomicU16::new(100);
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Malformed DNS response")
}
fn u16_at(data: &[u8], at: usize) -> io::Result<u16> {
    Ok(u16::from_be_bytes(
        data.get(at..at + 2)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    ))
}
fn skip_name(data: &[u8], mut at: usize) -> io::Result<usize> {
    for _ in 0..128 {
        let len = *data.get(at).ok_or_else(invalid)? as usize;
        if len == 0 {
            return Ok(at + 1);
        }
        if len & 0xc0 == 0xc0 {
            data.get(at + 1).ok_or_else(invalid)?;
            return Ok(at + 2);
        }
        if len > 63 {
            return Err(invalid());
        }
        at += len + 1;
    }
    Err(invalid())
}

pub fn parse(data: &[u8], id: u16) -> io::Result<Vec<Ipv4Addr>> {
    if data.len() < 12 || u16_at(data, 0)? != id || u16_at(data, 2)? & 0x8000 == 0 {
        return Err(invalid());
    }
    let flags = u16_at(data, 2)?;
    if flags & 0x200 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Truncated DNS response",
        ));
    }
    if flags & 15 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("DNS rcode {}", flags & 15),
        ));
    }
    let mut at = 12;
    for _ in 0..u16_at(data, 4)? {
        at = skip_name(data, at)? + 4;
        if at > data.len() {
            return Err(invalid());
        }
    }
    let mut out = Vec::new();
    for _ in 0..u16_at(data, 6)? {
        at = skip_name(data, at)?;
        let kind = u16_at(data, at)?;
        let class = u16_at(data, at + 2)?;
        let len = u16_at(data, at + 8)? as usize;
        at += 10;
        let body = data.get(at..at + len).ok_or_else(invalid)?;
        if kind == 1 && class == 1 && len == 4 {
            out.push(Ipv4Addr::new(body[0], body[1], body[2], body[3]));
        }
        at += len;
    }
    if out.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No IPv4 DNS answer",
        ));
    }
    Ok(out)
}

pub async fn resolve(host: &str, adapter: &Adapter) -> io::Result<Vec<Ipv4Addr>> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(vec![ip]);
    }
    if !crate::policy::valid_host(host) {
        return Err(invalid());
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut query = Vec::from(id.to_be_bytes());
    query.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in host.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.extend_from_slice(&[0, 0, 1, 0, 1]);
    let mut last = io::Error::new(
        io::ErrorKind::NotFound,
        "Selected adapter has no IPv4 DNS server",
    );
    for server in adapter.dns.iter().take(2) {
        let attempt = async {
            let socket = UdpSocket::bind((adapter.ip, 0)).await?;
            socket.connect((*server, 53)).await?;
            socket.send(&query).await?;
            let mut bytes = [0u8; 4096];
            let n = socket.recv(&mut bytes).await?;
            match parse(&bytes[..n], id) {
                Err(e) if e.kind() == io::ErrorKind::Unsupported => {
                    let mut stream: TcpStream =
                        super::dial::connect_ip(SocketAddr::from((*server, 53)), Some(adapter.ip))
                            .await?;
                    stream.write_u16(query.len() as u16).await?;
                    stream.write_all(&query).await?;
                    let size = stream.read_u16().await? as usize;
                    let mut body = vec![0; size];
                    stream.read_exact(&mut body).await?;
                    parse(&body, id)
                }
                result => result,
            }
        };
        match timeout(Duration::from_secs(4), attempt).await {
            Ok(Ok(v)) => return Ok(v),
            Ok(Err(e)) => last = e,
            Err(_) => last = io::Error::new(io::ErrorKind::TimedOut, "Adapter DNS timed out"),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_dns_never_panics() {
        for n in 0..80 {
            assert!(parse(&vec![0; n], 1).is_err())
        }
    }
    #[test]
    fn compressed_a_record_is_parsed() {
        let mut d = vec![
            0, 1, 0x81, 0x80, 0, 0, 0, 1, 0, 0, 0, 0, 0xc0, 0, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4,
        ];
        d.extend_from_slice(&[10, 3, 6, 5]);
        assert_eq!(parse(&d, 1).unwrap(), vec![Ipv4Addr::new(10, 3, 6, 5)]);
        assert!(parse(&d, 2).is_err());
    }
}

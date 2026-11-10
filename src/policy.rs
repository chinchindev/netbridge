use serde::{Deserialize, Serialize};
use std::{io, net::Ipv4Addr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Lan,
    Usb,
    System,
}

pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

pub fn matches(pattern: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let pattern = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some((network, bits)) = pattern.split_once('/') {
        if let (Ok(network), Ok(ip), Ok(bits)) = (
            network.parse::<Ipv4Addr>(),
            host.parse::<Ipv4Addr>(),
            bits.parse::<u32>(),
        ) {
            if bits <= 32 {
                let mask = if bits == 0 {
                    0
                } else {
                    u32::MAX << (32 - bits)
                };
                return u32::from(network) & mask == u32::from(ip) & mask;
            }
        }
        return false;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return valid_host(suffix) && (host == suffix || host.ends_with(&format!(".{suffix}")));
    }
    valid_host(&pattern) && host == pattern
}

pub fn valid_pattern(pattern: &str) -> bool {
    if let Some((ip, bits)) = pattern.split_once('/') {
        return ip.parse::<Ipv4Addr>().is_ok() && bits.parse::<u32>().is_ok_and(|n| n <= 32);
    }
    valid_host(
        pattern
            .strip_prefix("*.")
            .unwrap_or(pattern)
            .trim_end_matches('.'),
    )
}

pub fn authority(input: &str) -> io::Result<(String, u16)> {
    let (host, port) = input
        .rsplit_once(':')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing port"))?;
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let port = port.parse::<u16>().ok().filter(|p| *p > 0);
    if !valid_host(&host) || port.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid IPv4/hostname authority",
        ));
    }
    Ok((host, port.unwrap()))
}

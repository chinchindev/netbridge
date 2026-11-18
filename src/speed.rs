use crate::adapters::Adapter;
use reqwest::blocking::Client;
use serde::Serialize;
use std::{
    io::{self, Read},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub const SERVICE: &str = "https://speed.cloudflare.com";
pub const DOWNLOAD_BYTES: u64 = 10_000_000;
pub const UPLOAD_BYTES: u64 = 2_000_000;
#[derive(Clone, Debug, Serialize)]
pub struct Transfer {
    pub bytes: u64,
    pub mbps: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ResultRow {
    pub adapter_id: String,
    pub adapter: String,
    pub source_ip: Ipv4Addr,
    pub latency_ms: Option<f64>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub errors: Vec<String>,
}
pub fn mbps(bytes: u64, elapsed: Duration) -> Option<f64> {
    (!elapsed.is_zero()).then(|| bytes as f64 * 8.0 / elapsed.as_secs_f64() / 1_000_000.0)
}
pub fn median_latency(samples: &[Duration]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort();
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 {
        (sorted[mid - 1].as_secs_f64() + sorted[mid].as_secs_f64()) * 500.0
    } else {
        sorted[mid].as_secs_f64() * 1000.0
    })
}
pub fn bound_client(source: Ipv4Addr, host: &str, addresses: &[SocketAddr]) -> io::Result<Client> {
    if addresses.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No IPv4 DNS answer on the selected adapter",
        ));
    }
    crate::http_client::builder(Duration::from_secs(15))
        .local_address(IpAddr::V4(source))
        .resolve_to_addrs(host, addresses)
        .build()
        .map_err(io::Error::other)
}

fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Đã dừng phép đo",
        ))
    } else {
        Ok(())
    }
}

fn validate_response(response: &reqwest::blocking::Response) -> io::Result<()> {
    if response.status() != reqwest::StatusCode::OK {
        return Err(io::Error::other(format!(
            "Máy chủ đo trả HTTP {}",
            response.status()
        )));
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|v| v != "identity")
    {
        return Err(io::Error::other(
            "Compressed response cannot be used for this measurement",
        ));
    }
    Ok(())
}

pub fn download(
    client: &Client,
    url: &str,
    expected: u64,
    cancel: &AtomicBool,
) -> io::Result<Transfer> {
    check_cancel(cancel)?;
    let start = Instant::now();
    let mut response = client
        .get(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .map_err(io::Error::other)?;
    validate_response(&response)?;
    if response.content_length().is_some_and(|len| len != expected) {
        return Err(io::Error::other("Unexpected download payload size"));
    }
    let mut count = 0;
    let mut chunk = [0u8; 32768];
    loop {
        check_cancel(cancel)?;
        let n = response.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > expected {
            return Err(io::Error::other("Download exceeds the measurement budget"));
        }
    }
    if count != expected {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Incomplete download measurement",
        ));
    }
    Ok(Transfer {
        bytes: count,
        mbps: mbps(count, start.elapsed())
            .ok_or_else(|| io::Error::other("Measurement duration was zero"))?,
    })
}

fn empty_result(adapter: &Adapter) -> ResultRow {
    ResultRow {
        adapter_id: adapter.id.clone(),
        adapter: adapter.name.clone(),
        source_ip: adapter.ip,
        latency_ms: None,
        download_mbps: None,
        upload_mbps: None,
        errors: Vec::new(),
    }
}

pub fn measure_with_client(
    adapter: &Adapter,
    client: &Client,
    base: &str,
    download_bytes: u64,
    upload_bytes: u64,
    cancel: &AtomicBool,
    progress: &dyn Fn(String),
) -> ResultRow {
    let mut result = empty_result(adapter);
    let mut samples = Vec::new();
    for index in 0..3 {
        progress(format!(
            "{} · Đo độ trễ HTTP {}/3…",
            adapter.name,
            index + 1
        ));
        let attempt = (|| {
            check_cancel(cancel)?;
            let start = Instant::now();
            let mut response = client
                .get(format!("{base}/__down?bytes=0"))
                .header(reqwest::header::CACHE_CONTROL, "no-cache")
                .send()
                .map_err(io::Error::other)?;
            validate_response(&response)?;
            let elapsed = start.elapsed();
            let mut extra = [0u8; 1];
            if response.read(&mut extra)? != 0 {
                return Err(io::Error::other("Latency response must be empty"));
            }
            Ok::<_, io::Error>(elapsed)
        })();
        match attempt {
            Ok(elapsed) => samples.push(elapsed),
            Err(error) => {
                result.errors.push(format!("Độ trễ: {error}"));
                break;
            }
        }
    }
    result.latency_ms = median_latency(&samples);
    progress(format!(
        "{} · Đang tải xuống {:.0} MB…",
        adapter.name,
        download_bytes as f64 / 1_000_000.0
    ));
    match download(
        client,
        &format!("{base}/__down?bytes={download_bytes}"),
        download_bytes,
        cancel,
    ) {
        Ok(transfer) => result.download_mbps = Some(transfer.mbps),
        Err(error) => result.errors.push(format!("Download: {error}")),
    }
    progress(format!(
        "{} · Đang tải lên {:.0} MB…",
        adapter.name,
        upload_bytes as f64 / 1_000_000.0
    ));
    let upload = (|| {
        check_cancel(cancel)?;
        if upload_bytes > 64 * 1024 * 1024 {
            return Err(io::Error::other("Upload exceeds the measurement budget"));
        }
        let payload = vec![0u8; upload_bytes as usize];
        let start = Instant::now();
        let mut response = client
            .post(format!("{base}/__up"))
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(payload)
            .send()
            .map_err(io::Error::other)?;
        validate_response(&response)?;
        let mut reply = Vec::new();
        response.by_ref().take(65537).read_to_end(&mut reply)?;
        if reply.len() > 65536 {
            return Err(io::Error::other("Unexpected upload response"));
        }
        check_cancel(cancel)?;
        mbps(upload_bytes, start.elapsed())
            .ok_or_else(|| io::Error::other("Measurement duration was zero"))
    })();
    match upload {
        Ok(rate) => result.upload_mbps = Some(rate),
        Err(error) => result.errors.push(format!("Upload: {error}")),
    }
    result
}

pub fn measure_adapter(
    selected: &Adapter,
    cancel: &AtomicBool,
    progress: &dyn Fn(String),
) -> ResultRow {
    let setup = (|| {
        check_cancel(cancel)?;
        let adapter = crate::adapters::enumerate()?
            .into_iter()
            .find(|a| a.id == selected.id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "Adapter không còn hoạt động; không chuyển mạng khác",
                )
            })?;
        progress(format!(
            "{} · Phân giải DNS qua {}…",
            adapter.name, adapter.ip
        ));
        let runtime = tokio::runtime::Runtime::new()?;
        let ips = runtime.block_on(crate::dns::resolve("speed.cloudflare.com", &adapter))?;
        check_cancel(cancel)?;
        let addresses: Vec<_> = ips
            .into_iter()
            .map(|ip| SocketAddr::from((ip, 443)))
            .collect();
        let client = bound_client(adapter.ip, "speed.cloudflare.com", &addresses)?;
        Ok::<_, io::Error>((adapter, client))
    })();
    match setup {
        Ok((adapter, client)) => measure_with_client(
            &adapter,
            &client,
            SERVICE,
            DOWNLOAD_BYTES,
            UPLOAD_BYTES,
            cancel,
            progress,
        ),
        Err(error) => {
            let mut result = empty_result(selected);
            result.errors.push(error.to_string());
            result
        }
    }
}

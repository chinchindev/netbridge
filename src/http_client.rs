use reqwest::blocking::{Client, ClientBuilder};
use std::{
    io::{self, Read},
    time::Duration,
};

pub fn builder(timeout: Duration) -> ClientBuilder {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(timeout)
        .user_agent("NetBridge/0.1 (Windows portable)")
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT_ENCODING,
                reqwest::header::HeaderValue::from_static("identity"),
            );
            headers
        })
}

pub fn download(url: &str, limit: u64, progress: &dyn Fn(u64)) -> io::Result<Vec<u8>> {
    let client = builder(Duration::from_secs(90))
        .https_only(true)
        .build()
        .map_err(io::Error::other)?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(io::Error::other)?;
    if response.content_length().is_some_and(|size| size > limit) {
        return Err(io::Error::other("Download exceeds size limit"));
    }
    let mut output = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let count = response.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if output.len() as u64 + count as u64 > limit {
            return Err(io::Error::other("Download exceeds size limit"));
        }
        output.extend_from_slice(&chunk[..count]);
        progress(output.len() as u64);
    }
    Ok(output)
}

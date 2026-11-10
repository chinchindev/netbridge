use crate::{config::Config, dial, policy};
use std::{
    collections::VecDeque,
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex, RwLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{watch, Semaphore},
    time::timeout,
};

pub type SharedConfig = Arc<RwLock<Config>>;
pub type Log = Arc<Mutex<VecDeque<String>>>;
pub fn log(log: &Log, message: impl Into<String>) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86400;
    let mut lines = log.lock().unwrap_or_else(|e| e.into_inner());
    lines.push_back(format!(
        "{:02}:{:02}:{:02}  {}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        message.into()
    ));
    while lines.len() > 250 {
        lines.pop_front();
    }
}

pub struct Server {
    stop: watch::Sender<bool>,
    worker: Option<thread::JoinHandle<()>>,
    pub port: u16,
}
impl Server {
    pub fn start(config: SharedConfig, logs: Log) -> io::Result<Self> {
        let port = config
            .read()
            .map_err(|_| io::Error::other("Configuration unavailable"))?
            .port;
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        listener.set_nonblocking(true)?;
        let actual = listener.local_addr()?.port();
        let (stop, mut shutdown) = watch::channel(false);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let worker = thread::spawn(move || {
            runtime.block_on(async move{
            let listener=match TcpListener::from_std(listener){Ok(v)=>v,Err(e)=>{log(&logs,format!("Listener: {e}"));return;}};
            let limit=Arc::new(Semaphore::new(128));
            log(&logs,format!("Proxy 127.0.0.1:{actual} đang chạy"));
            loop {
                tokio::select!{
                    _=shutdown.changed()=>break,
                    incoming=listener.accept()=>{
                        let Ok((client,_))=incoming else{break};
                        let Ok(permit)=limit.clone().try_acquire_owned() else{drop(client);continue};
                        let cfg=config.clone();let logs=logs.clone();let mut cancel=shutdown.clone();
                        tokio::spawn(async move{
                            let _permit=permit;
                            tokio::select!{
                                _=cancel.changed()=>{},
                                result=handle(client,cfg,logs.clone())=>{
                                    if let Err(e)=result{log(&logs,format!("Kết nối: {}",e.kind()));}
                                }
                            }
                        });
                    }
                }
            }
        })
        });
        Ok(Self {
            stop,
            worker: Some(worker),
            port: actual,
        })
    }
    pub fn stop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn handle(mut client: TcpStream, config: SharedConfig, logs: Log) -> io::Result<()> {
    let current = config
        .read()
        .map_err(|_| io::Error::other("Configuration unavailable"))?
        .clone();
    let upstream = timeout(Duration::from_secs(35), async {
        let byte = client.read_u8().await?;
        if byte == 5 {
            socks(&mut client, &current, &logs).await
        } else {
            http(&mut client, byte, &current, &logs).await
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Proxy handshake timed out"))??;
    let mut upstream = upstream;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

async fn outbound(config: &Config, host: &str, port: u16, logs: &Log) -> io::Result<TcpStream> {
    let path = config.select(host);
    match dial::connect(config, host, port).await {
        Ok((s, _)) => {
            log(
                logs,
                format!("{host}:{port} → {path:?} ({})", s.local_addr()?.ip()),
            );
            Ok(s)
        }
        Err(e) => {
            log(logs, format!("{host}:{port} → {path:?}: {}", e));
            Err(e)
        }
    }
}

async fn http(
    client: &mut TcpStream,
    first: u8,
    config: &Config,
    logs: &Log,
) -> io::Result<TcpStream> {
    let mut bytes = vec![first];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 32768 {
            return reject(client, 431).await;
        }
        bytes.push(client.read_u8().await?);
    }
    let mut headers = [httparse::EMPTY_HEADER; 96];
    let mut request = httparse::Request::new(&mut headers);
    if !matches!(request.parse(&bytes), Ok(httparse::Status::Complete(_))) {
        return reject(client, 400).await;
    }
    let method = request.method.unwrap_or("");
    let target = request.path.unwrap_or("");
    if method == "CONNECT" {
        let (host, port) = match policy::authority(target) {
            Ok(v) => v,
            Err(_) => return reject(client, 400).await,
        };
        if request.headers.iter().any(|h| {
            h.name.eq_ignore_ascii_case("transfer-encoding")
                || (h.name.eq_ignore_ascii_case("content-length") && h.value != b"0")
        }) {
            return reject(client, 400).await;
        }
        match outbound(config, &host, port, logs).await {
            Ok(upstream) => {
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await?;
                Ok(upstream)
            }
            Err(_) => reject(client, 502).await,
        }
    } else {
        let uri = match url::Url::parse(target) {
            Ok(v) if v.scheme() == "http" && v.username().is_empty() && v.password().is_none() => v,
            _ => return reject(client, 400).await,
        };
        let host = uri.host_str().unwrap_or("");
        if !policy::valid_host(host) {
            return reject(client, 400).await;
        }
        let port = uri.port().unwrap_or(80);
        let mut upstream = match outbound(config, host, port, logs).await {
            Ok(v) => v,
            Err(_) => return reject(client, 502).await,
        };
        let path = format!(
            "{}{}",
            uri.path(),
            uri.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        let mut head =
            format!("{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n")
                .into_bytes();
        for header in request.headers.iter() {
            if [
                "host",
                "connection",
                "proxy-connection",
                "proxy-authorization",
                "keep-alive",
            ]
            .iter()
            .any(|s| header.name.eq_ignore_ascii_case(s))
            {
                continue;
            }
            head.extend_from_slice(header.name.as_bytes());
            head.extend_from_slice(b": ");
            head.extend_from_slice(header.value);
            head.extend_from_slice(b"\r\n");
        }
        head.extend_from_slice(b"\r\n");
        upstream.write_all(&head).await?;
        Ok(upstream)
    }
}

async fn reject(client: &mut TcpStream, code: u16) -> io::Result<TcpStream> {
    client
        .write_all(
            format!(
                "HTTP/1.1 {code} Proxy Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await?;
    Err(io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "Proxy rejected request",
    ))
}

async fn socks(client: &mut TcpStream, config: &Config, logs: &Log) -> io::Result<TcpStream> {
    let count = client.read_u8().await? as usize;
    let mut methods = vec![0; count];
    client.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        client.write_all(&[5, 255]).await?;
        return Err(io::Error::other("SOCKS auth unsupported"));
    }
    client.write_all(&[5, 0]).await?;
    let mut header = [0; 4];
    client.read_exact(&mut header).await?;
    if header[..3] != [5, 1, 0] {
        return socks_fail(client, 7).await;
    }
    let host = match header[3] {
        1 => {
            let mut ip = [0; 4];
            client.read_exact(&mut ip).await?;
            Ipv4Addr::from(ip).to_string()
        }
        3 => {
            let n = client.read_u8().await? as usize;
            let mut name = vec![0; n];
            client.read_exact(&mut name).await?;
            match String::from_utf8(name) {
                Ok(v) => v.to_ascii_lowercase(),
                Err(_) => return socks_fail(client, 8).await,
            }
        }
        _ => return socks_fail(client, 8).await,
    };
    let port = client.read_u16().await?;
    if port == 0 || !policy::valid_host(&host) {
        return socks_fail(client, 8).await;
    }
    match outbound(config, &host, port, logs).await {
        Ok(upstream) => {
            client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            Ok(upstream)
        }
        Err(_) => socks_fail(client, 5).await,
    }
}
async fn socks_fail(client: &mut TcpStream, code: u8) -> io::Result<TcpStream> {
    client.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    Err(io::Error::other("SOCKS connection failed"))
}

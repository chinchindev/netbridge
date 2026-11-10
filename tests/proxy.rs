use netbridge::{
    config::Config,
    proxy::{Log, Server},
};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, RwLock},
    thread,
    time::Duration,
};

fn server() -> (Server, Log) {
    let c = Config {
        port: 0,
        rules: vec![],
        ..Config::default()
    };
    let log = Arc::new(Mutex::new(VecDeque::new()));
    (
        Server::start(Arc::new(RwLock::new(c)), log.clone()).unwrap(),
        log,
    )
}

#[test]
fn missing_selected_adapter_does_not_fall_back_to_windows() {
    use netbridge::{config::Rule, policy::Route};
    let cfg = Config {
        port: 0,
        lan_id: "missing-adapter".into(),
        rules: vec![Rule {
            pattern: "127.0.0.1".into(),
            route: Route::Lan,
        }],
        ..Config::default()
    };
    let proxy = Server::start(
        Arc::new(RwLock::new(cfg)),
        Arc::new(Mutex::new(VecDeque::new())),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        client,
        "CONNECT 127.0.0.1:{} HTTP/1.1\r\n\r\n",
        listener.local_addr().unwrap().port()
    )
    .unwrap();
    let mut out = String::new();
    client.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 502"));
    assert!(listener.accept().is_err());
}

#[test]
fn plain_http_uses_origin_form_and_drops_proxy_credentials() {
    let (proxy, _) = server();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let upstream = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut b = [0];
            s.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        let text = String::from_utf8(head).unwrap();
        assert!(text.starts_with("GET /sample?q=1 HTTP/1.1"));
        assert!(!text.to_ascii_lowercase().contains("proxy-authorization"));
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .unwrap();
    });
    let mut c = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    write!(c,"GET http://127.0.0.1:{port}/sample?q=1 HTTP/1.1\r\nProxy-Authorization: fixture-only\r\n\r\n").unwrap();
    let mut result = String::new();
    c.read_to_string(&mut result).unwrap();
    assert!(result.ends_with("OK"));
    upstream.join().unwrap();
}
fn echo() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let t = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0; 4];
        s.read_exact(&mut buf).unwrap();
        s.write_all(&buf).unwrap();
    });
    (port, t)
}
#[test]
fn connect_tunnel_preserves_bytes_and_half_close() {
    let (proxy, _) = server();
    let (port, t) = echo();
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    write!(s, "CONNECT 127.0.0.1:{port} HTTP/1.1\r\nHost: test\r\n\r\n").unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        s.read_exact(&mut b).unwrap();
        head.push(b[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 200"));
    s.write_all(&[0, 255, 1, 2]).unwrap();
    s.shutdown(std::net::Shutdown::Write).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    assert_eq!(out, [0, 255, 1, 2]);
    t.join().unwrap();
}
#[test]
fn socks5_supports_raw_tcp_clients() {
    let (proxy, _) = server();
    let (port, t) = echo();
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.write_all(&[5, 1, 0]).unwrap();
    let mut auth = [0; 2];
    s.read_exact(&mut auth).unwrap();
    assert_eq!(auth, [5, 0]);
    let mut req = vec![5, 1, 0, 1, 127, 0, 0, 1];
    req.extend_from_slice(&port.to_be_bytes());
    s.write_all(&req).unwrap();
    let mut reply = [0; 10];
    s.read_exact(&mut reply).unwrap();
    assert_eq!(reply[1], 0);
    s.write_all(b"test").unwrap();
    let mut out = [0; 4];
    s.read_exact(&mut out).unwrap();
    assert_eq!(&out, b"test");
    t.join().unwrap();
}
#[test]
fn rejects_authority_injection_and_releases_port_on_stop() {
    let (mut proxy, _) = server();
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.write_all(b"CONNECT user@host:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 400"));
    let port = proxy.port;
    proxy.stop();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
}

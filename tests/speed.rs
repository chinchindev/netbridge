use netbridge::{adapters::Adapter, speed};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener},
    sync::atomic::AtomicBool,
    thread,
};

#[test]
fn measured_traffic_is_bound_to_the_requested_source_and_counts_real_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut peers = Vec::new();
        for _ in 0..5 {
            let (mut socket, peer) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            peers.push(peer.ip());
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
            }
            let text = String::from_utf8(headers).unwrap();
            let upload = text
                .to_ascii_lowercase()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .and_then(|v| v.parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0; upload];
            socket.read_exact(&mut body).unwrap();
            let requested = text
                .lines()
                .next()
                .unwrap()
                .split("bytes=")
                .nth(1)
                .and_then(|s| s.split(['&', ' ']).next())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {requested}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            socket.write_all(&vec![0; requested]).unwrap();
        }
        peers
    });
    let client =
        speed::bound_client(Ipv4Addr::new(127, 0, 0, 2), "speed.example", &[address]).unwrap();
    let adapter = Adapter {
        id: "test".into(),
        name: "Loopback source".into(),
        description: String::new(),
        index: 0,
        ip: Ipv4Addr::new(127, 0, 0, 2),
        dns: vec![],
        if_type: 6,
    };
    let result = speed::measure_with_client(
        &adapter,
        &client,
        &format!("http://speed.example:{}", address.port()),
        100_000,
        50_000,
        &AtomicBool::new(false),
        &|_| {},
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(result.latency_ms.is_some_and(|v| v > 0.0));
    assert!(result
        .download_mbps
        .is_some_and(|v| v > 0.0 && v.is_finite()));
    assert!(result.upload_mbps.is_some_and(|v| v > 0.0 && v.is_finite()));
    assert!(server
        .join()
        .unwrap()
        .iter()
        .all(|ip| *ip == std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))));
}

#[test]
fn a_truncated_download_is_an_error_not_a_successful_speed_result() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0u8; 1];
            socket.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\nConnection: close\r\n\r\nshort",
            )
            .unwrap();
    });
    let client = speed::bound_client(Ipv4Addr::LOCALHOST, "speed.example", &[address]).unwrap();
    assert!(speed::download(
        &client,
        &format!("http://speed.example:{}/__down?bytes=10000", address.port()),
        10_000,
        &AtomicBool::new(false)
    )
    .is_err());
    server.join().unwrap();
}

#[test]
fn cancellation_prevents_starting_a_new_network_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let client = speed::bound_client(Ipv4Addr::LOCALHOST, "speed.example", &[address]).unwrap();
    let error = speed::download(
        &client,
        &format!("http://speed.example:{}/__down?bytes=1", address.port()),
        1,
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn a_missing_adapter_reports_failure_instead_of_using_the_default_network() {
    let adapter = Adapter {
        id: "netbridge-test-adapter-does-not-exist".into(),
        name: "Missing".into(),
        description: String::new(),
        index: 0,
        ip: Ipv4Addr::new(192, 0, 2, 10),
        dns: vec![],
        if_type: 6,
    };
    let row = speed::measure_adapter(&adapter, &AtomicBool::new(false), &|_| {});
    assert!(row.latency_ms.is_none() && row.download_mbps.is_none() && row.upload_mbps.is_none());
    assert!(!row.errors.is_empty());
}

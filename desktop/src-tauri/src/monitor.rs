use crate::service::AppState;
use netbridge::{
    adapters::{self, Adapter},
    config::Config,
    network_monitor::{self, Endpoint},
    traffic::Connection,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::State;

#[derive(Clone, Serialize)]
pub struct Event {
    time: u64,
    kind: String,
    detail: String,
}
#[derive(Default)]
struct Log {
    entries: VecDeque<Event>,
    last: HashMap<String, String>,
    seen: HashSet<u64>,
}
#[derive(Default)]
pub struct Journal(Mutex<Log>);
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl Log {
    fn add(&mut self, time: u64, kind: &str, detail: String) {
        if self.entries.len() >= 200 {
            self.entries.pop_front();
        }
        self.entries.push_back(Event {
            time,
            kind: kind.into(),
            detail,
        });
    }
    fn transition(&mut self, kind: &str, value: String) {
        if self.last.get(kind) == Some(&value) {
            return;
        }
        self.last.insert(kind.into(), value.clone());
        self.add(now(), kind, value);
    }
    fn observe(&mut self, proxy: String, usb: String, rows: &[Connection]) {
        self.transition("Proxy Windows", proxy);
        self.transition("USB / IPv4", usb);
        let mut current = HashSet::new();
        for row in rows.iter().rev().filter(|r| {
            matches!(r.stage.as_str(), "DNS" | "TCP" | "adapter")
                && !r.result.is_empty()
                && r.result != "session stopped"
        }) {
            current.insert(row.id);
            if !self.seen.contains(&row.id) {
                self.add(
                    row.started.saturating_add((row.elapsed_ms / 1000) as u64),
                    &row.stage,
                    format!("{}:{} · {}", row.host, row.port, row.result),
                );
            }
        }
        self.seen = current;
    }
}
pub fn start_history(state: Arc<AppState>, journal: Arc<Journal>) {
    if state.session.lock().map_or(true, |s| s.preview) {
        return;
    }
    let state = Arc::downgrade(&state);
    std::thread::spawn(move || loop {
        let Some(app) = state.upgrade() else { break };
        let observation = app
            .session
            .lock()
            .ok()
            .map(|s| (s.config().usb_id, s.view().label, s.traffic.clone()));
        drop(app);
        if let Some((id, proxy, traffic)) = observation {
            let usb = if id.is_empty() {
                "Chưa chọn card USB".into()
            } else {
                match adapters::enumerate() {
                    Ok(list) => list.into_iter().find(|a| a.id == id).map_or_else(
                        || "Card đã chọn mất kết nối hoặc chưa có IPv4".into(),
                        |a| format!("{} · {}", a.name, a.ip),
                    ),
                    Err(e) => format!("Không đọc được card: {e}"),
                }
            };
            let rows = traffic.snapshot().rows;
            journal
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .observe(proxy, usb, &rows);
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}
fn card_for<'a>(
    ip: std::net::IpAddr,
    indices: &HashMap<std::net::IpAddr, Option<u32>>,
    cards: &'a [Adapter],
) -> Option<&'a Adapter> {
    let index = indices.get(&ip).copied().flatten()?;
    cards.iter().find(|a| a.index == index)
}
fn route_name(config: &Config, a: &Adapter) -> String {
    if a.id == config.usb_id {
        format!("USB · {}", a.name)
    } else if a.id == config.lan_id {
        format!("LAN · {}", a.name)
    } else {
        a.name.clone()
    }
}
fn describe(
    rows: &mut [Endpoint],
    cards: &[Adapter],
    cfg: &Config,
    port: Option<u16>,
    tunnels: &[Connection],
    indices: &HashMap<std::net::IpAddr, Option<u32>>,
) {
    let active: HashMap<_, _> = tunnels
        .iter()
        .filter(|r| r.result.is_empty())
        .filter_map(|r| Some((r.peer?, r)))
        .collect();
    // Verify that Windows attributes the reverse half of this loopback socket to this instance.
    let accepted: HashSet<_> = rows
        .iter()
        .filter(|e| e.pid == std::process::id() && e.state == "ESTABLISHED")
        .filter_map(|e| Some((e.local.clone(), e.remote.clone()?)))
        .collect();
    for e in rows {
        let Some(remote) = e.remote.as_ref().and_then(|r| r.parse::<SocketAddr>().ok()) else {
            continue;
        };
        let Ok(local) = e.local.parse::<SocketAddr>() else {
            continue;
        };
        if remote.ip().is_loopback() {
            if remote.ip() != Ipv4Addr::LOCALHOST || Some(remote.port()) != port {
                e.path = "Nội bộ máy".into();
                continue;
            }
            e.path = "NetBridge → chưa xác định".into();
            if let Some(r) = active.get(&local).filter(|_| {
                e.state == "ESTABLISHED"
                    && accepted.contains(&(remote.to_string(), local.to_string()))
            }) {
                if let Some(a) = r
                    .source
                    .parse()
                    .ok()
                    .and_then(|ip| card_for(ip, indices, cards))
                {
                    e.path = format!("NetBridge → {}", route_name(cfg, a));
                }
                let rule = r
                    .rule
                    .map_or_else(|| "mặc định".into(), |n| format!("quy tắc #{n}"));
                e.evidence = format!(
                    "Tunnel #{} · {}:{} · {} · {} · IP nguồn {}",
                    r.id,
                    r.host,
                    r.port,
                    rule,
                    r.stage,
                    if r.source.is_empty() {
                        "chưa có"
                    } else {
                        &r.source
                    }
                );
            }
        } else if let Some(a) = card_for(local.ip(), indices, cards) {
            e.path = format!("Trực tiếp → {}", route_name(cfg, a));
            e.evidence = format!("IP nguồn TCP {} khớp card {}", local.ip(), a.name);
        } else {
            e.path = "Chưa xác định".into();
        }
    }
}

#[derive(Serialize)]
pub struct MonitorView {
    #[serde(flatten)]
    snapshot: network_monitor::Snapshot,
    events: Vec<Event>,
    targets: Vec<String>,
}
const DEFAULT_TARGET: &str = "speed.cloudflare.com:443 · Cloudflare";
fn target_names(cfg: &Config) -> Vec<String> {
    let mut result: Vec<_> = cfg
        .check_targets
        .iter()
        .map(|t| format!("{}:{} · {}", t.host, t.port, t.name))
        .collect();
    if !result.iter().any(|name| name == DEFAULT_TARGET) {
        result.push(DEFAULT_TARGET.into());
    }
    result
}
#[tauri::command]
pub async fn network_monitor(
    state: State<'_, Arc<AppState>>,
    journal: State<'_, Arc<Journal>>,
) -> Result<MonitorView, String> {
    let (cfg, port, traffic) = {
        let s = state.session.lock().map_err(|_| "Session lock")?;
        (s.config(), s.listener_port(), s.traffic.clone())
    };
    let mut events: Vec<Event> = journal
        .0
        .lock()
        .map_err(|_| "Journal lock")?
        .entries
        .iter()
        .rev()
        .cloned()
        .collect();
    events.sort_by_key(|e| std::cmp::Reverse(e.time));
    tauri::async_runtime::spawn_blocking(move || {
        let mut snapshot = network_monitor::snapshot();
        let cards = adapters::enumerate().unwrap_or_else(|e| {
            snapshot.warnings.push(e.to_string());
            Vec::new()
        });
        let indices = network_monitor::address_indices().unwrap_or_else(|e| {
            snapshot.warnings.push(e.to_string());
            HashMap::new()
        });
        describe(
            &mut snapshot.endpoints,
            &cards,
            &cfg,
            port,
            &traffic.snapshot().rows,
            &indices,
        );
        let targets = target_names(&cfg);
        MonitorView {
            snapshot,
            events,
            targets,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct Step {
    label: &'static str,
    status: &'static str,
    detail: String,
}
fn step(label: &'static str, status: &'static str, detail: impl Into<String>) -> Step {
    Step {
        label,
        status,
        detail: detail.into(),
    }
}
#[tauri::command]
pub async fn usb_health(
    state: State<'_, Arc<AppState>>,
    journal: State<'_, Arc<Journal>>,
    target: Option<String>,
) -> Result<Vec<Step>, String> {
    let cfg = {
        let s = state.session.lock().map_err(|_| "Session lock")?;
        if s.preview {
            return Err("Chỉ kiểm tra được ở phiên chạy thực.".into());
        }
        s.config()
    };
    let steps = inspect_health(cfg, target).await?;
    let mut log = journal.0.lock().map_err(|_| "Journal lock")?;
    for s in &steps {
        log.transition(
            s.label,
            format!(
                "{} · {}",
                match s.status {
                    "ok" => "Đạt",
                    "fail" => "Lỗi",
                    _ => "Chưa rõ",
                },
                s.detail
            ),
        );
    }
    Ok(steps)
}

async fn inspect_health(cfg: Config, target: Option<String>) -> Result<Vec<Step>, String> {
    let selected = match target.as_deref() {
        None | Some(DEFAULT_TARGET) => netbridge::config::CheckTarget {
            name: "Cloudflare".into(),
            host: "speed.cloudflare.com".into(),
            port: 443,
        },
        Some(key) => cfg
            .check_targets
            .iter()
            .find(|t| format!("{}:{} · {}", t.host, t.port, t.name) == key)
            .ok_or("Đích đã thay đổi. Chọn lại đích.")?
            .clone(),
    };
    let (mut steps, card) = tauri::async_runtime::spawn_blocking(move || {
        let card = adapters::enumerate()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|a| a.id == cfg.usb_id);
        let ids = netbridge::usb_identity::read();
        let phone = ids.serial(&cfg.usb_id).and_then(|serial| {
            netbridge::devices::list(&cfg.adb_path)
                .ok()?
                .into_iter()
                .find(|p| p.usb && p.state == "device" && p.serial.eq_ignore_ascii_case(serial))
        });
        let tether = match phone {
            Some(p) => match netbridge::devices::usb_state(&cfg.adb_path, &p.serial) {
                Ok(s) if s.service_verified() => step(
                    "Điện thoại",
                    if s.enabled { "ok" } else { "fail" },
                    format!(
                        "{} · {}",
                        p.model,
                        if s.enabled {
                            "Android xác nhận chia sẻ USB"
                        } else {
                            "Chia sẻ USB đã tắt"
                        }
                    ),
                ),
                Ok(s) => step(
                    "Điện thoại",
                    "unknown",
                    format!(
                        "{} · USB {} · chưa xác minh dịch vụ tethering",
                        p.model, s.functions
                    ),
                ),
                Err(e) => step("Điện thoại", "unknown", e.to_string()),
            },
            None => step(
                "Điện thoại",
                "unknown",
                "Chưa ghép được thiết bị ADB đã cấp quyền với card trong profile",
            ),
        };
        let ip = match &card {
            Some(a) => step("IPv4", "ok", format!("{} · {}", a.name, a.ip)),
            None => step(
                "IPv4",
                "fail",
                "Card USB trong profile chưa có IPv4 hoặc chưa được chọn",
            ),
        };
        let gateway = match card.as_ref().and_then(|a| a.gateway.map(|gw| (a.ip, gw))) {
            Some((src, gw)) => match network_monitor::gateway_echo(src, gw) {
                Ok(ms) => step("Gateway", "ok", format!("{gw} · {ms} ms")),
                Err(e) => step(
                    "Gateway",
                    "fail",
                    format!("{gw} · ICMP chưa thành công: {e}"),
                ),
            },
            None => step("Gateway", "unknown", "Chưa có gateway IPv4 trên card USB"),
        };
        Ok::<_, String>((vec![tether, ip, gateway], card))
    })
    .await
    .map_err(|e| e.to_string())??;
    let t = selected;
    let target = match card {
        Some(a) => {
            let probe = async {
                let ips = netbridge::dns::resolve(&t.host, &a).await?;
                for ip in ips.into_iter().take(3) {
                    if netbridge::dial::connect_ip((ip, t.port).into(), Some(a.ip))
                        .await
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
                Err(std::io::Error::other("TCP chưa kết nối được"))
            };
            let (status, msg) = match tokio::time::timeout(Duration::from_secs(10), probe).await {
                Ok(Ok(())) => ("ok", "TCP thành công".into()),
                Ok(Err(e)) => ("fail", e.to_string()),
                Err(_) => ("fail", "Hết thời gian chờ".into()),
            };
            step(
                "Đích qua USB",
                status,
                format!("{}:{} · {} · nguồn {}", t.host, t.port, msg, a.ip),
            )
        }
        _ => step("Đích qua USB", "unknown", "Chưa có IP nguồn"),
    };
    steps.push(target);
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use netbridge::traffic::Traffic;
    #[test]
    fn attributes_only_live_matching_peers_and_known_interface_addresses() {
        let card = Adapter {
            id: "usb".into(),
            name: "Ethernet 4".into(),
            description: "".into(),
            index: 4,
            ip: "192.0.2.4".parse().unwrap(),
            gateway: None,
            dns: vec![],
            if_type: 6,
        };
        let cfg = Config {
            usb_id: "usb".into(),
            ..Config::default()
        };
        let traffic = Traffic::default();
        let trace = traffic.begin();
        trace.update(|r| {
            r.peer = Some("127.0.0.1:51000".parse().unwrap());
            r.source = card.ip.to_string();
            r.host = "example.test".into();
            r.port = 443;
        });
        let endpoint = |local: &str, remote: &str| Endpoint {
            pid: 123,
            process: "app".into(),
            app: "app".into(),
            local: local.into(),
            remote: Some(remote.into()),
            state: "ESTABLISHED",
            path: String::new(),
            evidence: String::new(),
        };
        let rows = [
            endpoint("127.0.0.1:51000", "127.0.0.1:17890"),
            endpoint("127.0.0.1:51001", "127.0.0.1:17890"),
            endpoint("[2001:db8::4]:50000", "[2001:db8::2]:443"),
        ];
        let mut rows = rows.to_vec();
        let mut reverse = endpoint("127.0.0.1:17890", "127.0.0.1:51000");
        reverse.pid = std::process::id();
        rows.push(reverse);
        let indices = HashMap::from([
            (card.ip.into(), Some(4)),
            ("2001:db8::4".parse().unwrap(), Some(4)),
        ]);
        let cards = [card];
        describe(
            &mut rows,
            &cards,
            &cfg,
            Some(17890),
            &traffic.snapshot().rows,
            &indices,
        );
        assert!(rows[0].path.contains("USB"));
        assert!(rows[0].evidence.contains("example.test"));
        assert!(rows[1].path.contains("chưa xác định"));
        assert!(rows[2].path.contains("Trực tiếp → USB"));
        drop(trace);
        rows[0].evidence.clear();
        describe(
            &mut rows,
            &cards,
            &cfg,
            Some(17890),
            &traffic.snapshot().rows,
            &indices,
        );
        assert!(rows[0].path.contains("chưa xác định"));
    }
    #[test]
    fn timeline_deduplicates_samples_and_tunnel_failures() {
        let traffic = Traffic::default();
        let t = traffic.begin();
        t.update(|r| {
            r.stage = "DNS".into();
            r.result = "timeout".into();
        });
        drop(t);
        let rows = traffic.snapshot().rows;
        let mut log = Log::default();
        for _ in 0..3 {
            log.observe("on".into(), "USB 1".into(), &rows);
        }
        assert_eq!(log.entries.len(), 3);
        log.observe("off".into(), "USB 1".into(), &rows);
        assert_eq!(log.entries.len(), 4);
    }
    #[test]
    #[ignore = "requires local USB profile and an explicit diagnostic request"]
    fn inspect_configured_usb_path() {
        let cfg = Config::load(&netbridge::config::data_dir().join("config.json")).unwrap();
        let steps = tauri::async_runtime::block_on(inspect_health(cfg, None)).unwrap();
        for s in &steps {
            println!("{}: {} ({})", s.label, s.status, s.detail);
        }
        assert_eq!(steps.len(), 4);
    }
}

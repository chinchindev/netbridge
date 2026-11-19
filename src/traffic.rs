use crate::policy::Route;
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: u64,
    pub started: u64,
    pub host: String,
    pub port: u16,
    pub route: Option<Route>,
    pub rule: Option<usize>,
    pub adapter: String,
    pub source: String,
    pub stage: String,
    pub result: String,
    pub elapsed_ms: u128,
    pub sent: u64,
    pub received: u64,
}
#[derive(Default, Clone, Serialize)]
pub struct TrafficView {
    pub active: usize,
    pub rejected: u64,
    pub rows: Vec<Connection>,
}
#[derive(Default)]
pub struct Bytes {
    pub sent: AtomicU64,
    pub received: AtomicU64,
}
struct Active {
    row: Connection,
    started: Instant,
    bytes: Arc<Bytes>,
}
impl Active {
    fn snapshot(&self) -> Connection {
        let mut row = self.row.clone();
        row.elapsed_ms = self.started.elapsed().as_millis();
        row.sent = self.bytes.sent.load(Ordering::Relaxed);
        row.received = self.bytes.received.load(Ordering::Relaxed);
        row
    }
}
#[derive(Default)]
struct History {
    next: u64,
    active: BTreeMap<u64, Active>,
    finished: VecDeque<Connection>,
    rejected: u64,
}
#[derive(Default, Clone)]
pub struct Traffic(Arc<Mutex<History>>);
impl Traffic {
    pub fn snapshot(&self) -> TrafficView {
        let h = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<_> = h
            .active
            .values()
            .map(Active::snapshot)
            .chain(h.finished.iter().cloned())
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.id));
        TrafficView {
            active: h.active.len(),
            rejected: h.rejected,
            rows,
        }
    }
    pub fn reject(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).rejected += 1;
    }
    pub fn begin(&self) -> Trace {
        let mut h = self.0.lock().unwrap_or_else(|e| e.into_inner());
        h.next += 1;
        let id = h.next;
        let bytes = Arc::new(Bytes::default());
        h.active.insert(
            id,
            Active {
                started: Instant::now(),
                bytes: bytes.clone(),
                row: Connection {
                    id,
                    started: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    host: String::new(),
                    port: 0,
                    route: None,
                    rule: None,
                    adapter: String::new(),
                    source: String::new(),
                    stage: "handshake".into(),
                    result: String::new(),
                    elapsed_ms: 0,
                    sent: 0,
                    received: 0,
                },
            },
        );
        Trace {
            traffic: self.clone(),
            id,
            bytes,
        }
    }
}
pub struct Trace {
    traffic: Traffic,
    id: u64,
    pub bytes: Arc<Bytes>,
}
impl Trace {
    pub fn update(&self, update: impl FnOnce(&mut Connection)) {
        if let Some(row) = self
            .traffic
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active
            .get_mut(&self.id)
        {
            update(&mut row.row)
        }
    }
    pub fn outcome(&self, result: String) {
        self.update(|row| {
            if row.result.is_empty() {
                row.result = result;
            }
        });
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        let mut h = self.traffic.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = h.active.remove(&self.id) {
            let mut row = record.snapshot();
            if row.result.is_empty() {
                row.result = "session stopped".into();
            }
            if h.finished.len() == 250 {
                h.finished.pop_front();
            }
            h.finished.push_back(row);
        }
    }
}

// Count bytes actually written in either direction, including on partial transfer.
pub struct CountWrites<'a, T> {
    pub stream: &'a mut T,
    pub bytes: &'a AtomicU64,
}
impl<T: AsyncRead + Unpin> AsyncRead for CountWrites<'_, T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.stream).poll_read(cx, buf)
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for CountWrites<'_, T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut *self.stream).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = result {
            self.bytes.fetch_add(n as u64, Ordering::Relaxed);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.stream).poll_shutdown(cx)
    }
}

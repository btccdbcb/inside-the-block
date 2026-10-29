//! Slot clock anchored on first-shred events.
//!
//! Two (or more) WebSocket endpoints stream `slotsUpdatesSubscribe`; we keep the `firstShredReceived`
//! events, whose timestamps come from the RPC node's own clock (ms). For each slot the earliest timestamp
//! across sources is the observed first shred. The first shred of a future slot is predicted from the latest
//! observed slot plus the median slot-to-slot gap for each position in the leader turn (slot % 4).
//! Streams that stay silent for 3 s are reconnected. Every event is logged (raw node and receive times),
//! so the analysis recomputes achieved send offsets from the same data.

use crate::util::{log, wall_ms, Jsonl};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

pub const DEFAULT_WS: &[&str] = &["wss://solana-rpc.publicnode.com", "wss://public.rpc.solanavibestation.com"];
/// The slot event used as the anchor. Mainnet: `firstShredReceived`. A single-node localnet never receives shreds,
/// so rehearsals set `ITB_CLOCK_EVENT=createdBank`.
fn anchor_event() -> &'static str {
    static EV: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    EV.get_or_init(|| std::env::var("ITB_CLOCK_EVENT").unwrap_or_else(|_| "firstShredReceived".into()))
}

const SILENCE: Duration = Duration::from_secs(3);
const GAP_WINDOW: usize = 2_000;
const DEFAULT_GAP_MS: f64 = 200.0;
const KEEP_SLOTS: u64 = 20_000;

#[derive(Clone, Copy, Debug)]
pub struct Obs {
    pub node_ms: f64,
    pub recv_ms: i64,
    pub source: usize,
}

#[derive(Default)]
struct State {
    first: BTreeMap<u64, Obs>,
    gaps: [VecDeque<f64>; 4],
    last_recv_ms: Vec<i64>,
    /// Rolling minimum of (receive time − node time) per source over recent events.
    delays: Vec<VecDeque<(i64, f64)>>,
}

#[derive(Clone)]
pub struct SlotClock {
    st: Arc<RwLock<State>>,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Prediction {
    /// Predicted first-shred time of the slot (ms since epoch, our clock frame).
    pub ms: f64,
    pub anchor_slot: u64,
    pub anchor_age_ms: i64,
    pub horizon_slots: u64,
    pub observed: bool,
}

fn median(v: &VecDeque<f64>) -> Option<f64> {
    if v.len() < 20 {
        return None;
    }
    let mut x: Vec<f64> = v.iter().copied().collect();
    x.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(x[x.len() / 2])
}

impl SlotClock {
    pub fn start(urls: Vec<String>, sink: Option<Arc<Jsonl>>) -> SlotClock {
        let n = urls.len();
        let st = State { last_recv_ms: vec![0; n], delays: vec![VecDeque::new(); n], ..Default::default() };
        let clock = SlotClock { st: Arc::new(RwLock::new(st)) };
        for (i, url) in urls.into_iter().enumerate() {
            let c = clock.clone();
            let sink = sink.clone();
            tokio::spawn(async move { c.run_source(i, url, sink).await });
        }
        clock
    }

    pub fn urls_from_env() -> Vec<String> {
        match std::env::var("ITB_WS_URLS") {
            Ok(v) if !v.trim().is_empty() => v.split(',').map(|s| s.trim().to_string()).collect(),
            _ => DEFAULT_WS.iter().map(|s| s.to_string()).collect(),
        }
    }

    async fn run_source(&self, i: usize, url: String, sink: Option<Arc<Jsonl>>) {
        let host = crate::util::redact_url(&url);
        loop {
            match self.stream_once(i, &url, sink.as_deref()).await {
                Ok(()) => log("ws_silent_reconnect", json!({"source": i, "host": host})),
                Err(e) => log("ws_error_reconnect", json!({"source": i, "host": host, "error": e.to_string()})),
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn stream_once(&self, i: usize, url: &str, sink: Option<&Jsonl>) -> anyhow::Result<()> {
        let mut req = url.into_client_request()?;
        req.headers_mut().insert("User-Agent", crate::rpc::USER_AGENT.parse()?);
        let (mut ws, _) =
            tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(req)).await??;
        let sub = json!({"jsonrpc": "2.0", "id": 1, "method": "slotsUpdatesSubscribe"}).to_string();
        ws.send(Message::Text(sub.into())).await?;
        loop {
            let msg = match tokio::time::timeout(SILENCE, ws.next()).await {
                Err(_) => return Ok(()),
                Ok(None) => anyhow::bail!("stream closed"),
                Ok(Some(m)) => m?,
            };
            let recv_ms = wall_ms();
            let text = match &msg {
                Message::Text(_) => msg.to_text()?,
                Message::Ping(p) => {
                    ws.send(Message::Pong(p.clone())).await?;
                    continue;
                }
                Message::Close(_) => anyhow::bail!("server closed"),
                _ => continue,
            };
            let v: Value = match serde_json::from_str(text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let r = &v["params"]["result"];
            if r["type"].as_str() != Some(anchor_event()) {
                if r.is_object() {
                    self.st.write().unwrap().last_recv_ms[i] = recv_ms;
                }
                continue;
            }
            let (Some(slot), Some(ts)) = (r["slot"].as_u64(), r["timestamp"].as_i64()) else {
                continue;
            };
            if let Some(s) = sink {
                let _ = s.write(&json!({"slot": slot, "src": i, "node_ms": ts, "recv_ms": recv_ms}));
            }
            self.record(slot, ts as f64, recv_ms, i);
        }
    }

    fn record(&self, slot: u64, node_ms: f64, recv_ms: i64, source: usize) {
        let mut st = self.st.write().unwrap();
        st.last_recv_ms[source] = recv_ms;
        let d = &mut st.delays[source];
        d.push_back((recv_ms, recv_ms as f64 - node_ms));
        while d.front().is_some_and(|(t, _)| *t < recv_ms - 600_000) {
            d.pop_front();
        }
        let corrected = node_ms + st.correction(source);
        let entry = st.first.entry(slot).or_insert(Obs { node_ms: corrected, recv_ms, source });
        if corrected < entry.node_ms {
            *entry = Obs { node_ms: corrected, recv_ms, source };
        }
        let this = st.first[&slot].node_ms;
        if let Some(prev) = st.first.get(&(slot - 1)).map(|o| o.node_ms) {
            let g = &mut st.gaps[(slot % 4) as usize];
            g.push_back(this - prev);
            if g.len() > GAP_WINDOW {
                g.pop_front();
            }
        }
        if st.first.len() as u64 > KEEP_SLOTS {
            let cut = slot.saturating_sub(KEEP_SLOTS);
            st.first = st.first.split_off(&cut);
        }
    }

    /// Median gap (ms) ending at a slot of each position in the turn (0 = first slot of a leader's turn).
    pub fn gap_medians(&self) -> [f64; 4] {
        let st = self.st.read().unwrap();
        std::array::from_fn(|k| median(&st.gaps[k]).unwrap_or(DEFAULT_GAP_MS))
    }

    pub fn gap_counts(&self) -> [usize; 4] {
        let st = self.st.read().unwrap();
        std::array::from_fn(|k| st.gaps[k].len())
    }

    pub fn observed(&self, slot: u64) -> Option<Obs> {
        self.st.read().unwrap().first.get(&slot).copied()
    }

    pub fn latest(&self) -> Option<(u64, Obs)> {
        self.st.read().unwrap().first.iter().next_back().map(|(s, o)| (*s, *o))
    }

    /// Milliseconds since any source last delivered an event.
    pub fn silence_ms(&self) -> i64 {
        let st = self.st.read().unwrap();
        wall_ms() - st.last_recv_ms.iter().copied().max().unwrap_or(0)
    }

    pub fn predict(&self, slot: u64) -> Option<Prediction> {
        let gaps = self.gap_medians();
        let st = self.st.read().unwrap();
        if let Some(o) = st.first.get(&slot) {
            return Some(Prediction {
                ms: o.node_ms,
                anchor_slot: slot,
                anchor_age_ms: wall_ms() - o.recv_ms,
                horizon_slots: 0,
                observed: true,
            });
        }
        let (&a, o) = st.first.range(..slot).next_back()?;
        let mut t = o.node_ms;
        for k in (a + 1)..=slot {
            t += gaps[(k % 4) as usize];
        }
        Some(Prediction {
            ms: t,
            anchor_slot: a,
            anchor_age_ms: wall_ms() - o.recv_ms,
            horizon_slots: slot - a,
            observed: false,
        })
    }
}

impl State {
    /// If a node's clock is ahead of ours (receive time before node time), shift its timestamps back.
    fn correction(&self, source: usize) -> f64 {
        let min = self.delays[source].iter().map(|(_, d)| *d).fold(f64::INFINITY, f64::min);
        if min.is_finite() && min < 0.0 {
            min
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicts_from_latest_anchor_and_position_gaps() {
        let c = SlotClock {
            st: Arc::new(RwLock::new(State {
                last_recv_ms: vec![0],
                delays: vec![VecDeque::new()],
                ..Default::default()
            })),
        };
        // 400 slots with gaps 260 (position 0) and 150/200/200 (positions 1..3)
        let gaps = [260.0, 150.0, 200.0, 200.0];
        let mut t = 1_000_000.0;
        for s in 1_000u64..1_400 {
            t += gaps[(s % 4) as usize];
            c.record(s, t, t as i64 + 50, 0);
        }
        assert_eq!(c.gap_medians(), gaps);
        let p = c.predict(1_403).unwrap();
        let expected = c.observed(1_399).unwrap().node_ms + 260.0 + 150.0 + 200.0 + 200.0;
        assert!((p.ms - expected).abs() < 1e-6);
        assert_eq!(p.horizon_slots, 4);
        assert!(c.predict(1_399).unwrap().observed);
    }

    #[test]
    fn earliest_source_wins() {
        let c = SlotClock {
            st: Arc::new(RwLock::new(State {
                last_recv_ms: vec![0, 0],
                delays: vec![VecDeque::new(), VecDeque::new()],
                ..Default::default()
            })),
        };
        c.record(10, 5_000.0, 5_040, 0);
        c.record(10, 4_990.0, 5_030, 1);
        assert_eq!(c.observed(10).unwrap().node_ms, 4_990.0);
        assert_eq!(c.observed(10).unwrap().source, 1);
    }
}

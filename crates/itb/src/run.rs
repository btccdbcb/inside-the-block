//! The quad runner: fires the design rows of one session into the targets' turns.
//!
//! For every upcoming turn of a target that still has rows left, the runner decides ~0.9 s before the planned
//! fire time whether to use that turn (pacing spreads the target's rows evenly over the session window), builds
//! and signs the four probes, reserves their fees in the ledger, waits for the fire time and writes one QUIC
//! stream per probe. A dry run does all of this with throw-away signers and writes the signed bytes to the log
//! instead of the socket.
//!
//! Fire time = first shred of slot0 − 1 (the preceding leader's last slot) + τ (D-023). For τ ≥ +20 ms the runner
//! waits for the observed first shred of slot0 − 1 and fires relative to it (the observation arrives ~10–15 ms
//! after the shred); for earlier offsets it re-predicts from the newest observed slot until the moment of firing.
//!
//! Within a pair, each member transfers 1 lamport to the other member's payer, so both write-lock the same two
//! payers (R_IND: each transfers to itself). The two pairs of a quad use four different payers of one pool.

use crate::auth::LiveAuthorization;
use crate::clock::{Prediction, SlotClock};
use crate::design::DesignRow;
use crate::gossip::{self, Snapshot, TargetView};
use crate::keys::Keypair;
use crate::ledger::{Ledger, Reservation};
use crate::quic::{self, QuicClient, SendTiming};
use crate::rpc::Rpc;
use crate::schedule::Schedule;
use crate::tx::library::{FeeScale, PairType, Probe};
use crate::tx::{self, cost, legacy, v1, Format, Shape};
use crate::util::{log, mono_ns, mono_ns_at_wall_ms, mono_to_wall_ms, rand_unit, utc_now, wall_ms, Jsonl};
use anyhow::{bail, Context, Result};
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Decide and build this long before the planned fire time.
pub const PREP_MS: f64 = 900.0;
/// Less lead than this: leave the row for the target's next turn.
pub const MIN_PREP_MS: f64 = 350.0;
/// Skip rule: a quad whose first send would leave more than this after plan is not sent.
pub const LATE_SKIP_MS: f64 = 100.0;
/// Re-check the connection this long before the fire time: 2 x handshake + 20 ms, within these bounds.
pub const RECHECK_MIN_MS: f64 = 30.0;
pub const RECHECK_MAX_MS: f64 = 150.0;
/// Used when the connection came from the cache (handshake time unknown).
pub const RECHECK_DEFAULT_MS: f64 = 50.0;
/// τ at or above this fires relative to the observed first shred of slot0 − 1.
pub const OBSERVED_ANCHOR_MIN_TAU: i32 = 20;
/// How long past the predicted fire time to wait for that observation before skipping.
pub const OBSERVATION_WAIT_MS: f64 = 300.0;
/// The control pair's first send follows the main pair's first send by this much.
pub const CONTROL_OFFSET_US: u64 = 500;
pub const MAX_CONCURRENT_QUADS: usize = 2;
pub const POOLS: usize = 2;
pub const POOL_SIZE: usize = 4;
/// A skipped row goes back to the front of its target's queue at most this many times.
pub const MAX_ATTEMPTS: u32 = 3;
const BLOCKHASH_MAX_AGE_MS: i64 = 10_000;
const ACK_WAIT: Duration = Duration::from_secs(2);
const GOSSIP_EVERY: Duration = Duration::from_secs(60);
const GOSSIP_FULL_EVERY_MS: i64 = 30 * 60_000;
const CLOCK_MAX_SILENCE_MS: i64 = 3_000;

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[derive(Clone, Copy, Debug)]
pub struct Blockhash {
    pub hash: [u8; 32],
    pub last_valid_height: u64,
    pub slot: u64,
    pub fetched_ms: i64,
}

/// Money side of a live run. Absent in a dry run.
pub struct Live {
    pub auth: LiveAuthorization,
    pub ledger: Arc<Mutex<Ledger>>,
    pub sub_budget: String,
    /// payer top-ups at each quiescent batch boundary
    pub refill: Option<Refill>,
}

/// Top up payers below `low` to `float` from the funder (fees charged to the setup sub-budget).
pub struct Refill {
    pub auth: LiveAuthorization,
    pub funder: Keypair,
    pub float: u64,
    pub low: u64,
}

pub struct Logs {
    pub dir: PathBuf,
    pub windows: Jsonl,
    pub quads: Jsonl,
    pub sends: Jsonl,
    pub streams: Jsonl,
    pub gossip: Jsonl,
}

impl Logs {
    pub fn open(dir: &Path) -> Result<Logs> {
        std::fs::create_dir_all(dir)?;
        Ok(Logs {
            dir: dir.to_path_buf(),
            windows: Jsonl::open(&dir.join("windows.jsonl"), true)?,
            quads: Jsonl::open(&dir.join("quads.jsonl"), true)?,
            sends: Jsonl::open(&dir.join("sends.jsonl"), true)?,
            streams: Jsonl::open(&dir.join("streams.jsonl"), true)?,
            gossip: Jsonl::open(&dir.join("gossip.jsonl"), false)?,
        })
    }
}

/// Everything one session needs.
pub struct Plan {
    pub session: String,
    pub rows: Vec<DesignRow>,
    /// design target slot name (T1..T13, X1) -> identity actually probed (primary or alternate)
    pub targets: HashMap<String, String>,
    pub window_end_ms: i64,
    /// used for rows without their own fee scale
    pub fee_scale: FeeScale,
    pub v1_enabled: bool,
    /// live: quads per batch before a quiescent reconciliation
    pub batch_quads: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct SessionResult {
    pub session: String,
    pub fired: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub unfired: Vec<String>,
    #[serde(skip)]
    pub unfired_rows: Vec<DesignRow>,
}

pub struct Runner {
    pub rpc: Arc<Rpc>,
    pub clock: SlotClock,
    pub quic: QuicClient,
    pub live: Option<Live>,
    pub payers: Vec<Keypair>,
    pub logs: Logs,
    pub run_id: String,
    bh: RwLock<Option<Blockhash>>,
    gossip: RwLock<Option<Snapshot>>,
    conns: tokio::sync::Mutex<HashMap<String, quinn::Connection>>,
    pools_busy: Mutex<[bool; POOLS]>,
    stopped: std::sync::atomic::AtomicBool,
}

/// One probe of a quad, signed and ready.
struct Built {
    role: &'static str,
    probe: Probe,
    payer: String,
    /// recipient of the 1-lamport transfer: the other member's payer, or the payer itself (R_IND)
    to: String,
    signature: String,
    bytes: Vec<u8>,
    fee: u64,
    /// microseconds after the quad's fire time
    offset_us: u64,
}

#[derive(Debug)]
enum QuadEnd {
    Fired,
    Skipped(String),
}

impl Runner {
    pub fn new(
        rpc: Arc<Rpc>,
        clock: SlotClock,
        live: Option<Live>,
        payers: Vec<Keypair>,
        logs: Logs,
        run_id: String,
    ) -> Result<Arc<Runner>> {
        if payers.len() < POOLS * POOL_SIZE {
            bail!("need {} payers, have {}", POOLS * POOL_SIZE, payers.len());
        }
        Ok(Arc::new(Runner {
            rpc,
            clock,
            quic: QuicClient::new()?,
            live,
            payers,
            logs,
            run_id,
            bh: RwLock::new(None),
            gossip: RwLock::new(None),
            conns: tokio::sync::Mutex::new(HashMap::new()),
            pools_busy: Mutex::new([false; POOLS]),
            stopped: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    /// Stop the background refreshers (the runner is not used afterwards).
    pub fn shutdown(&self) {
        self.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn is_live(&self) -> bool {
        self.live.is_some()
    }

    /// Background refreshers: blockhash every 2 s, gossip every 60 s (targets' view logged, full table every 30 min).
    pub fn start_background(self: &Arc<Self>, identities: Vec<String>) {
        let me = self.clone();
        tokio::spawn(async move {
            while !me.is_stopped() {
                match me.rpc.latest_blockhash().await {
                    Ok((hash, lvh, slot)) => {
                        *me.bh.write().unwrap() =
                            Some(Blockhash { hash, last_valid_height: lvh, slot, fetched_ms: wall_ms() });
                    }
                    Err(e) => log("blockhash_error", json!({"error": e.to_string()})),
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        let me = self.clone();
        tokio::spawn(async move {
            let mut last_full = 0i64;
            while !me.is_stopped() {
                if let Err(e) = me.refresh_gossip(&identities, &mut last_full).await {
                    log("gossip_error", json!({"error": e.to_string()}));
                }
                tokio::time::sleep(GOSSIP_EVERY).await;
            }
        });
    }

    async fn refresh_gossip(&self, identities: &[String], last_full: &mut i64) -> Result<()> {
        let snap = gossip::snapshot(&self.rpc).await?;
        let views: Vec<TargetView> = identities.iter().map(|id| snap.view(id)).collect();
        self.logs.gossip.write(&json!({"utc": snap.utc, "views": views}))?;
        if wall_ms() - *last_full >= GOSSIP_FULL_EVERY_MS {
            let dir = self.logs.dir.join("gossip_full");
            std::fs::create_dir_all(&dir)?;
            let name = snap.utc.replace([':', '.'], "-");
            std::fs::write(dir.join(format!("{name}.json")), serde_json::to_vec(&snap.compact())?)?;
            *last_full = wall_ms();
        }
        *self.gossip.write().unwrap() = Some(snap);
        Ok(())
    }

    /// Wait until the background tasks have a blockhash, a gossip snapshot and a running slot clock.
    pub async fn wait_ready(&self, max: Duration) -> Result<()> {
        let t0 = std::time::Instant::now();
        loop {
            let ok = self.bh.read().unwrap().is_some()
                && self.gossip.read().unwrap().is_some()
                && self.clock.gap_counts().iter().all(|&n| n >= 20)
                && self.clock.silence_ms() < CLOCK_MAX_SILENCE_MS;
            if ok {
                return Ok(());
            }
            if t0.elapsed() > max {
                bail!("runner not ready after {:?} (blockhash, gossip or slot clock missing)", max);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub fn view(&self, identity: &str) -> Option<TargetView> {
        self.gossip.read().unwrap().as_ref().map(|s| s.view(identity))
    }

    async fn connection(&self, identity: &str, addr: &str) -> Result<(quinn::Connection, Option<u64>)> {
        let mut conns = self.conns.lock().await;
        if let Some(c) = conns.get(identity) {
            if quic::is_open(c) && c.remote_address().to_string() == addr {
                return Ok((c.clone(), None));
            }
        }
        let (c, hs_us) = self.quic.connect(addr, Duration::from_millis(1_500)).await?;
        conns.insert(identity.to_string(), c.clone());
        Ok((c, Some(hs_us)))
    }

    /// A fresh connection (the cached one was closed by the server), replacing the cached one.
    async fn reconnect(&self, identity: &str, addr: &str, timeout_ms: f64) -> Result<(quinn::Connection, u64)> {
        let (c, us) = self.quic.connect(addr, Duration::from_micros((timeout_ms.max(1.0) * 1e3) as u64)).await?;
        self.conns.lock().await.insert(identity.to_string(), c.clone());
        Ok((c, us))
    }

    fn take_pool(&self, prefer: usize) -> Option<usize> {
        let mut busy = self.pools_busy.lock().unwrap();
        for k in 0..POOLS {
            let p = (prefer + k) % POOLS;
            if !busy[p] {
                busy[p] = true;
                return Some(p);
            }
        }
        None
    }

    fn release_pool(&self, p: usize) {
        self.pools_busy.lock().unwrap()[p] = false;
    }

    /// Run one session: fire its rows into the targets' turns until the window ends or every row is used.
    pub async fn run_session(self: &Arc<Self>, plan: Plan) -> Result<SessionResult> {
        let Plan { session, rows, targets, window_end_ms, fee_scale, v1_enabled, batch_quads } = plan;
        let mut result = SessionResult { session: session.clone(), ..Default::default() };
        // identity -> design target slots probed through it (several only in a localnet rehearsal)
        let mut by_identity: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in &targets {
            by_identity.entry(v.clone()).or_default().push(k.clone());
        }
        for names in by_identity.values_mut() {
            names.sort();
        }
        let mut queues: HashMap<String, VecDeque<(DesignRow, u32)>> = HashMap::new();
        let mut rows = rows;
        rows.sort_by_key(|r| r.seq);
        for r in rows {
            if !targets.contains_key(&r.target_slot) {
                log("row_without_target", json!({"quad_id": r.quad_id, "target_slot": r.target_slot}));
                result.unfired.push(r.quad_id.clone());
                result.unfired_rows.push(r);
                continue;
            }
            queues.entry(r.target_slot.clone()).or_default().push_back((r, 0));
        }
        let identities: Vec<String> = targets.values().cloned().collect();
        self.logs.windows.write(
            &json!({"event": "session_start", "session": session, "run_id": self.run_id, "utc": utc_now(),
            "window_end_utc": crate::util::utc_from_ms(window_end_ms), "targets": targets, "fee_scale": fee_scale,
            "v1_enabled": v1_enabled, "rows": queues.values().map(|q| q.len()).sum::<usize>(), "live": self.is_live()}),
        )?;

        let mut sched = self.load_schedule(&identities, window_end_ms).await?;
        let mut considered: HashSet<u64> = HashSet::new();
        let (tx_done, mut rx_done) = tokio::sync::mpsc::unbounded_channel::<(DesignRow, u32, usize, QuadEnd)>();
        let mut in_flight = 0usize;
        let mut since_reconcile = 0usize;
        let mut quad_counter = 0usize;
        let mut last_sched_refresh = wall_ms();

        loop {
            while let Ok((row, attempts, pool, end)) = rx_done.try_recv() {
                in_flight -= 1;
                self.release_pool(pool);
                match end {
                    QuadEnd::Fired => result.fired.push(row.quad_id.clone()),
                    QuadEnd::Skipped(reason) => {
                        if attempts + 1 < MAX_ATTEMPTS && reason != "cap" {
                            queues.entry(row.target_slot.clone()).or_default().push_front((row.clone(), attempts + 1));
                        }
                        result.skipped.push((row.quad_id.clone(), reason.clone()));
                        if reason == "cap" {
                            bail!("spend cap reached during session {session}");
                        }
                    }
                }
            }
            let now = wall_ms();
            let rows_left: usize = queues.values().map(|q| q.len()).sum();
            if now >= window_end_ms || rows_left == 0 {
                if in_flight == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            // quiescent reconciliation between batches (live)
            if self.is_live() && since_reconcile >= batch_quads {
                if in_flight > 0 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                self.batch_boundary(&session).await?;
                since_reconcile = 0;
                continue;
            }
            if now - last_sched_refresh > 10 * 60_000 {
                match self.load_schedule(&identities, window_end_ms).await {
                    Ok(s) => sched = s,
                    Err(e) => log("schedule_refresh_error", json!({"error": e.to_string()})),
                }
                last_sched_refresh = now;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            if self.clock.silence_ms() > CLOCK_MAX_SILENCE_MS {
                continue;
            }
            let Some((latest, _)) = self.clock.latest() else {
                continue;
            };
            let slot_ms = self.clock.gap_medians().iter().sum::<f64>() / 4.0;
            let end_slot = latest + ((window_end_ms - now).max(0) as f64 / slot_ms) as u64;
            for (slot0, identity) in sched.turns_between(latest + 1, latest + 24) {
                if considered.contains(&slot0) {
                    continue;
                }
                let Some(names) = by_identity.get(&identity) else {
                    considered.insert(slot0);
                    continue;
                };
                // the target slot with the most rows left (ties: first by name)
                let rows_here: usize = names.iter().map(|n| queues.get(n).map_or(0, |q| q.len())).sum();
                let Some(name) =
                    names.iter().max_by_key(|n| (queues.get(*n).map_or(0, |q| q.len()), std::cmp::Reverse(*n)))
                else {
                    considered.insert(slot0);
                    continue;
                };
                let Some(q) = queues.get_mut(name) else {
                    considered.insert(slot0);
                    continue;
                };
                let Some((row, _)) = q.front() else {
                    considered.insert(slot0);
                    continue;
                };
                let Some(pred) = self.clock.predict(slot0 - 1) else {
                    continue;
                };
                let lead = pred.ms + row.tau_ms as f64 - wall_ms() as f64;
                if lead > PREP_MS {
                    continue;
                }
                considered.insert(slot0);
                if lead < MIN_PREP_MS {
                    log("turn_too_close", json!({"slot0": slot0, "target": name, "lead_ms": lead}));
                    continue;
                }
                if in_flight >= MAX_CONCURRENT_QUADS {
                    log("turn_busy", json!({"slot0": slot0, "target": name}));
                    continue;
                }
                // pacing: use this turn with probability rows left / turns left for this identity
                let turns_left =
                    sched.turns.range(slot0..end_slot.max(slot0 + 1)).filter(|(_, id)| **id == identity).count().max(1);
                let p_use = (rows_here as f64 / turns_left as f64).min(1.0);
                if rand_unit() >= p_use {
                    continue;
                }
                let Some(pool) = self.take_pool(quad_counter % POOLS) else {
                    log("turn_no_free_pool", json!({"slot0": slot0, "target": name}));
                    continue;
                };
                let (row, attempts) = q.pop_front().unwrap();
                quad_counter += 1;
                since_reconcile += 1;
                in_flight += 1;
                let me = self.clone();
                let tx_done = tx_done.clone();
                let scale = row.fee_scale.unwrap_or(fee_scale);
                let prev_leader = sched.leader(slot0 - 1).map(String::from);
                let name = name.clone();
                tokio::spawn(async move {
                    let end = match me
                        .fire_quad(&row, &name, &identity, slot0, pool, scale, v1_enabled, attempts, prev_leader)
                        .await
                    {
                        Ok(e) => e,
                        Err(e) => {
                            log("quad_error", json!({"quad_id": row.quad_id, "error": format!("{e:#}")}));
                            let s = format!("{e:#}");
                            QuadEnd::Skipped(if s.contains("cap reached") || s.contains("sub-budget") {
                                "cap".into()
                            } else {
                                "error".into()
                            })
                        }
                    };
                    let _ = tx_done.send((row, attempts, pool, end));
                });
            }
        }
        for q in queues.into_values() {
            for (r, _) in q {
                result.unfired.push(r.quad_id.clone());
                result.unfired_rows.push(r);
            }
        }
        if self.is_live() {
            self.batch_boundary(&session).await?;
        }
        self.logs
            .windows
            .write(&json!({"event": "session_end", "session": session, "run_id": self.run_id, "utc": utc_now(),
            "fired": result.fired.len(), "skipped": result.skipped.len(), "unfired": result.unfired.len()}))?;
        Ok(result)
    }

    async fn load_schedule(&self, identities: &[String], window_end_ms: i64) -> Result<Schedule> {
        let slot = self.rpc.slot("confirmed").await?;
        let horizon = ((window_end_ms - wall_ms()).max(0) / 150) as u64 + 2_000;
        Schedule::load(&self.rpc, identities, slot.saturating_sub(10), slot + horizon.min(400_000)).await
    }

    /// Live only: wait until every pending probe has landed or expired and the chain agrees with the ledger.
    async fn batch_boundary(&self, session: &str) -> Result<()> {
        let live = self.live.as_ref().expect("live");
        let r = crate::reconcile::settle(&self.rpc, &live.ledger, Duration::from_secs(600))
            .await
            .with_context(|| format!("reconciliation failed in session {session}"))?;
        self.logs.windows.write(&json!({"event": "reconciled", "session": session, "utc": utc_now(), "report": r}))?;
        if let Some(f) = &live.refill {
            let n = crate::money::top_up(
                &self.rpc,
                &live.ledger,
                &f.auth,
                &format!("{}/top-up", self.run_id),
                &f.funder,
                &self.payers,
                f.float,
                f.low,
            )
            .await
            .with_context(|| format!("payer top-up failed in session {session}"))?;
            if n > 0 {
                self.logs
                    .windows
                    .write(&json!({"event": "top_up", "session": session, "utc": utc_now(), "payers": n}))?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn fire_quad(
        &self,
        row: &DesignRow,
        target_name: &str,
        identity: &str,
        slot0: u64,
        pool: usize,
        scale: FeeScale,
        v1_enabled: bool,
        attempt: u32,
        prev_leader: Option<String>,
    ) -> Result<QuadEnd> {
        let decided_ms = wall_ms();
        let view = self.view(identity);
        let mut rec = json!({"quad_id": row.quad_id, "session": row.session, "run_id": self.run_id, "attempt": attempt,
            "target_slot": target_name, "identity": identity, "slot0": slot0, "prev_leader": prev_leader,
            "tau_ms": row.tau_ms, "main_type": row.main_type, "dt_ms": row.dt_ms, "main_first": row.main_first,
            "control_type_design": row.control_type, "control_gap_ms": row.control_gap_ms, "control_first": row.control_first,
            "fee_scale": scale, "pool": pool, "decided_utc": crate::util::utc_from_ms(decided_ms), "view": view,
            "live": self.is_live()});
        let skip = |rec: &mut Value, reason: &str| -> Result<QuadEnd> {
            rec["status"] = json!("skipped");
            rec["reason"] = json!(reason);
            self.logs.quads.write(rec)?;
            Ok(QuadEnd::Skipped(reason.to_string()))
        };
        let Some(addr) = view.as_ref().and_then(|v| v.tpu_quic.clone()) else {
            return skip(&mut rec, "no_tpu_quic");
        };
        rec["tpu_quic"] = json!(addr);

        // connection first (Harmonic-style endpoints need ~2 RTT)
        let (conn, handshake_us) = match self.connection(identity, &addr).await {
            Ok(c) => c,
            Err(e) => {
                rec["error"] = json!(e.to_string());
                return skip(&mut rec, "quic_connect");
            }
        };
        rec["handshake_us"] = json!(handshake_us);

        // build and sign
        let Some(bh) = *self.bh.read().unwrap() else {
            return skip(&mut rec, "no_blockhash");
        };
        if wall_ms() - bh.fetched_ms > BLOCKHASH_MAX_AGE_MS {
            return skip(&mut rec, "stale_blockhash");
        }
        rec["blockhash"] = json!(tx::b58(&bh.hash));
        rec["blockhash_slot"] = json!(bh.slot);
        rec["last_valid_height"] = json!(bh.last_valid_height);
        let main = row.main()?;
        let mut control = row.control()?;
        if !v1_enabled && matches!(control, PairType::VEq | PairType::VHi) {
            control = PairType::EQ;
        }
        rec["control_type"] = json!(control);
        let built = self.build_quad(row, main, control, scale, pool, &bh)?;
        rec["probes"] = json!(built
            .iter()
            .map(|b| json!({"role": b.role, "probe": b.probe.name(), "payer": b.payer,
            "to": b.to, "signature": b.signature, "fee": b.fee, "offset_us": b.offset_us}))
            .collect::<Vec<_>>());

        // reserve (live) before any byte can leave
        if let Some(live) = &self.live {
            let rs: Vec<Reservation> = built
                .iter()
                .map(|b| Reservation {
                    kind: "probe".into(),
                    sub_budget: live.sub_budget.clone(),
                    run_id: format!("{}/{}", self.run_id, row.quad_id),
                    signature: b.signature.clone(),
                    fee_payer: b.payer.clone(),
                    blockhash: tx::b58(&bh.hash),
                    last_valid_height: bh.last_valid_height,
                    fee: b.fee,
                    transfers: vec![(b.payer.clone(), b.to.clone(), tx::PROBE_TRANSFER_LAMPORTS)],
                })
                .collect();
            let limit = live.auth.limit();
            self.on_ledger(move |l| l.reserve_many(&rs, limit)).await?;
        }
        for b in &built {
            self.logs.sends.write(&json!({"quad_id": row.quad_id, "run_id": self.run_id, "role": b.role, "probe": b.probe.name(),
                "signature": b.signature, "payer": b.payer, "to": b.to, "offset_us": b.offset_us, "tx_b64": b64(&b.bytes),
                "live": self.is_live()}))?;
        }

        // wait until shortly before the fire time; some servers drop idle unstaked connections near their turn,
        // so re-check and re-handshake there (the handshake time is known from the first connect)
        let mut conn = conn;
        let hs_ms = handshake_us.map(|u| u as f64 / 1e3).unwrap_or(RECHECK_DEFAULT_MS);
        let recheck_ms = (2.0 * hs_ms + 20.0).clamp(RECHECK_MIN_MS, RECHECK_MAX_MS);
        if let Err(reason) = self.wait_fire_time(slot0 - 1, row.tau_ms, recheck_ms).await {
            self.release_all(&built).await?;
            rec["fire_wait"] = json!(reason);
            return skip(&mut rec, &reason);
        }
        let mut reconnects = Vec::new();
        if !quic::is_open(&conn) {
            match self.reconnect(identity, &addr, recheck_ms + LATE_SKIP_MS / 2.0).await {
                Ok((c, us)) => {
                    conn = c;
                    reconnects.push(json!({"at": "recheck", "handshake_us": us}));
                }
                Err(e) => {
                    self.release_all(&built).await?;
                    rec["error"] = json!(e.to_string());
                    return skip(&mut rec, "connection_closed");
                }
            }
        }
        let (fire_ms, pred, anchor) = match self.wait_fire_time(slot0 - 1, row.tau_ms, 0.0).await {
            Ok(x) => x,
            Err(reason) => {
                self.release_all(&built).await?;
                rec["fire_wait"] = json!(reason);
                return skip(&mut rec, &reason);
            }
        };
        rec["planned_fire_ms"] = json!(fire_ms);
        rec["prediction"] = json!(pred);
        rec["anchor"] = json!(anchor);
        let fire_mono = mono_ns_at_wall_ms(fire_ms).max(0) as u64;
        if !quic::is_open(&conn) {
            let left = LATE_SKIP_MS - (mono_ns() as f64 - fire_mono as f64) / 1e6 - 5.0;
            let again = if left >= 10.0 { self.reconnect(identity, &addr, left).await.ok() } else { None };
            match again {
                Some((c, us)) => {
                    conn = c;
                    reconnects.push(json!({"at": "fire", "handshake_us": us}));
                }
                None => {
                    self.release_all(&built).await?;
                    rec["reconnects"] = json!(reconnects);
                    return skip(&mut rec, "connection_closed");
                }
            }
        }
        if !reconnects.is_empty() {
            rec["reconnects"] = json!(reconnects);
        }
        // live: take the stream credit now, so the timed writes below never wait for the peer
        let mut pre_opened = Vec::new();
        if self.live.is_some() {
            let left = LATE_SKIP_MS - (mono_ns() as f64 - fire_mono as f64) / 1e6 - 5.0;
            match quic::open_streams(&conn, built.len(), Duration::from_micros((left.max(1.0) * 1e3) as u64)).await {
                Ok(v) => pre_opened = v,
                Err(e) => {
                    self.release_all(&built).await?;
                    rec["error"] = json!(e.to_string());
                    return skip(&mut rec, "stream_credit");
                }
            }
        }
        let mut pre_opened = pre_opened.into_iter();
        let late_ms = (mono_ns() as f64 - fire_mono as f64) / 1e6;
        if late_ms > LATE_SKIP_MS {
            self.release_all(&built).await?;
            rec["late_ms"] = json!(late_ms);
            return skip(&mut rec, "late");
        }

        // send; the ledger is updated after the last send (an fsync per probe would push the later sends back by
        // milliseconds). Until then the rows stay 'reserved', which counts as charged and pending, so a crash in
        // between leaves them charged until reconciliation.
        let mut timings: Vec<SendTiming> = Vec::with_capacity(4);
        let mut streams = Vec::new();
        for b in &built {
            let planned = fire_mono + b.offset_us * 1_000;
            spin_until(planned).await;
            let t = match &self.live {
                Some(live) => {
                    let stream = pre_opened.next().expect("one pre-opened stream per probe");
                    let (t, s) = quic::send_tx(&live.auth, &conn, stream, &b.bytes, planned).await;
                    if let Some(s) = s {
                        streams.push((b.signature.clone(), s));
                    }
                    t
                }
                None => {
                    let now = mono_ns();
                    SendTiming {
                        planned_ns: planned,
                        open_ns: now,
                        write_ns: now,
                        finish_ns: now,
                        rtt_us: conn.rtt().as_micros() as u64,
                        bytes_may_have_left: false,
                        error: None,
                    }
                }
            };
            timings.push(t);
        }
        if self.live.is_some() {
            let updates: Vec<(String, bool)> =
                built.iter().zip(&timings).map(|(b, t)| (b.signature.clone(), t.bytes_may_have_left)).collect();
            self.on_ledger(move |l| {
                for (sig, left) in &updates {
                    if *left {
                        l.set_status(sig, "sent")?;
                    } else {
                        l.release_never_sent(sig)?;
                    }
                }
                Ok(())
            })
            .await?;
        }
        let first_wall = mono_to_wall_ms(timings[0].write_ns);
        rec["first_send_ms"] = json!(first_wall);
        rec["send_late_ms"] = json!(first_wall - fire_ms);
        rec["status"] = json!("fired");
        rec["fired_utc"] = json!(utc_now());
        self.logs.quads.write(&rec)?;

        // acknowledgements (live), then per-stream timing
        let mut acks: HashMap<String, Option<bool>> = HashMap::new();
        let waits = streams.into_iter().map(|(sig, s)| async move { (sig, quic::wait_ack(s, ACK_WAIT).await) });
        for (sig, a) in futures_util::future::join_all(waits).await {
            acks.insert(sig, a);
        }
        for (b, t) in built.iter().zip(&timings) {
            self.logs.streams.write(&json!({"quad_id": row.quad_id, "run_id": self.run_id, "role": b.role, "signature": b.signature,
                "planned_ms": mono_to_wall_ms(t.planned_ns), "open_ms": mono_to_wall_ms(t.open_ns), "write_ms": mono_to_wall_ms(t.write_ns),
                "finish_ms": mono_to_wall_ms(t.finish_ns), "rtt_us": t.rtt_us, "bytes_left": t.bytes_may_have_left,
                "error": t.error, "acked": acks.get(&b.signature).copied().flatten(), "live": self.is_live()}))?;
        }
        Ok(QuadEnd::Fired)
    }

    async fn release_all(&self, built: &[Built]) -> Result<()> {
        if self.live.is_none() {
            return Ok(());
        }
        let sigs: Vec<String> = built.iter().map(|b| b.signature.clone()).collect();
        self.on_ledger(move |l| {
            for sig in &sigs {
                l.release_never_sent(sig)?;
            }
            Ok(())
        })
        .await
    }

    /// Run a ledger operation on the blocking pool. The ledger is SQLite with fsync behind a std mutex; called on the
    /// async workers it can stall both of them (one in fsync, one waiting for the lock) and make other quads miss
    /// their fire time.
    async fn on_ledger<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Ledger) -> Result<T> + Send + 'static,
    {
        let ledger = self.live.as_ref().context("ledger outside a live run")?.ledger.clone();
        tokio::task::spawn_blocking(move || f(&mut ledger.lock().unwrap())).await?
    }

    /// Build the four probes. Roles: main pref, main other, control pref, control other; payer of role r is
    /// `pool[payer_perm[r]]`. Each member's transfer goes to the other member's payer (R_IND: to itself).
    fn build_quad(
        &self,
        row: &DesignRow,
        main: PairType,
        control: PairType,
        scale: FeeScale,
        pool: usize,
        bh: &Blockhash,
    ) -> Result<Vec<Built>> {
        let (mp, mo) = main.members();
        let (cp, co) = control.members();
        let dt_us = row.dt_ms as u64 * 1_000;
        let gap_us = row.control_gap_ms as u64 * 1_000;
        let main_pref_first = row.main_first == "pref";
        let control_pref_first = row.control_first == "pref";
        // (role, probe, pair shape, partner role, offset)
        let specs: [(&'static str, Probe, Shape, usize, u64); 4] = [
            ("main_pref", mp, main.shape(), 1, if main_pref_first { 0 } else { dt_us }),
            ("main_other", mo, main.shape(), 0, if main_pref_first { dt_us } else { 0 }),
            ("control_pref", cp, control.shape(), 3, CONTROL_OFFSET_US + if control_pref_first { 0 } else { gap_us }),
            ("control_other", co, control.shape(), 2, CONTROL_OFFSET_US + if control_pref_first { gap_us } else { 0 }),
        ];
        let payer_of = |r: usize| &self.payers[pool * POOL_SIZE + row.payer_perm[r] as usize];
        let mut out = Vec::with_capacity(4);
        for (r, (role, probe, shape, partner_role, offset_us)) in specs.into_iter().enumerate() {
            let payer = payer_of(r);
            let pk = payer.pubkey();
            let partner = match shape {
                Shape::Partner => Some(payer_of(partner_role).pubkey()),
                Shape::SelfTransfer => None,
            };
            let params = probe.params(scale);
            let bytes = match params.format {
                Format::Legacy => {
                    legacy::probe_message(&pk, partner.as_ref(), &params, &bh.hash)?.sign(&[&payer.signing])?
                }
                Format::V1 => v1::probe_message(&pk, partner.as_ref(), &params, &bh.hash)?.sign(&[&payer.signing])?,
            };
            // legacy puts the signature first, v1 last: take it from the decoded bytes
            let decoded = tx::decode(&bytes)?;
            let signature = tx::b58(&decoded.signatures[0]);
            let to = tx::b58(&partner.unwrap_or(pk));
            let moved = decoded.system_transfers_b58()?;
            if moved != [(payer.pubkey_b58(), to.clone(), tx::PROBE_TRANSFER_LAMPORTS)] {
                bail!("probe {role} moves {moved:?}, not 1 lamport to {to}");
            }
            out.push(Built {
                role,
                probe,
                payer: payer.pubkey_b58(),
                to,
                signature,
                bytes,
                fee: cost::probe_fee(&params),
                offset_us,
            });
        }
        // send order: by offset; at equal offsets main before control and the designed first member first
        out.sort_by_key(|b| {
            let designed_first = match b.role {
                "main_pref" => main_pref_first,
                "main_other" => !main_pref_first,
                "control_pref" => control_pref_first,
                _ => !control_pref_first,
            };
            (b.offset_us, b.role.starts_with("control"), !designed_first)
        });
        Ok(out)
    }

    /// Fire time = first shred of `anchor_slot` + τ. Returns (planned fire time in wall ms, last prediction,
    /// anchor kind) or a skip reason.
    /// Wait until `early_ms` before the fire time of (anchor_slot, tau). Returns the fire time and its prediction.
    async fn wait_fire_time(
        &self,
        anchor_slot: u64,
        tau_ms: i32,
        early_ms: f64,
    ) -> std::result::Result<(f64, Option<Prediction>, &'static str), String> {
        let observed_anchor = tau_ms >= OBSERVED_ANCHOR_MIN_TAU;
        loop {
            let Some(p) = self.clock.predict(anchor_slot) else {
                return Err("no_prediction".into());
            };
            let fire = p.ms + tau_ms as f64;
            let now = crate::util::wall_ns() as f64 / 1e6;
            if observed_anchor && !p.observed {
                if now > fire + OBSERVATION_WAIT_MS {
                    return Err("anchor_not_observed".into());
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
                continue;
            }
            let lead = fire - now - early_ms;
            if lead <= 3.0 {
                let anchor = if p.observed { "observed" } else { "predicted" };
                return Ok((fire, Some(p), anchor));
            }
            // sleep in short steps so newer observations re-anchor the prediction
            tokio::time::sleep(Duration::from_micros(((lead - 2.0).min(20.0) * 1_000.0) as u64)).await;
        }
    }
}

/// Spin (yielding briefly) until the monotonic clock reaches `target_ns`.
async fn spin_until(target_ns: u64) {
    loop {
        let now = mono_ns();
        if now >= target_ns {
            return;
        }
        let left = target_ns - now;
        if left > 3_000_000 {
            tokio::time::sleep(Duration::from_nanos(left - 2_000_000)).await;
        } else {
            std::hint::spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(main: &str, dt: u32, main_first: &str, control: &str, gap: u32, control_first: &str) -> DesignRow {
        DesignRow {
            quad_id: "q".into(),
            session: "A".into(),
            target_slot: "T1".into(),
            seq: 0,
            tau_ms: 0,
            main_type: main.into(),
            dt_ms: dt,
            main_first: main_first.into(),
            control_type: control.into(),
            control_gap_ms: gap,
            control_first: control_first.into(),
            payer_perm: [0, 1, 2, 3],
            fee_scale: None,
        }
    }

    fn runner() -> Arc<Runner> {
        let rpc = Arc::new(Rpc::new(&["http://127.0.0.1:1".to_string()]).unwrap());
        let clock = SlotClock::start(vec![], None);
        let dir = std::env::temp_dir().join(format!("itb-runtest-{}", crate::util::wall_ns()));
        Runner::new(rpc, clock, None, crate::keys::ephemeral_payers(), Logs::open(&dir).unwrap(), "t".into()).unwrap()
    }

    #[tokio::test]
    async fn quad_send_order_and_offsets() {
        let r = runner();
        let bh = Blockhash { hash: [7; 32], last_valid_height: 1, slot: 1, fetched_ms: 0 };
        let q = r
            .build_quad(&row("R", 25, "other", "EQ", 10, "pref"), PairType::R, PairType::EQ, FeeScale::S1, 1, &bh)
            .unwrap();
        let order: Vec<_> = q.iter().map(|b| (b.role, b.offset_us)).collect();
        assert_eq!(
            order,
            vec![("main_other", 0), ("control_pref", 500), ("control_other", 10_500), ("main_pref", 25_000)]
        );
        // payers come from pool 1 (indices 4..8), distinct
        let payers: HashSet<_> = q.iter().map(|b| b.payer.clone()).collect();
        assert_eq!(payers.len(), 4);
        assert!(q.iter().all(|b| r.payers[4..8].iter().any(|p| p.pubkey_b58() == b.payer)));
        // each member pays the other member's payer
        let get = |role: &str| q.iter().find(|b| b.role == role).unwrap();
        assert_eq!(get("main_pref").to, get("main_other").payer);
        assert_eq!(get("main_other").to, get("main_pref").payer);
        assert_eq!(get("control_pref").to, get("control_other").payer);
        assert_eq!(get("control_other").to, get("control_pref").payer);
        // every probe decodes and its signature verifies
        for b in &q {
            let d = tx::decode(&b.bytes).unwrap();
            let vk = ed25519_dalek::VerifyingKey::from_bytes(&d.accounts[0]).unwrap();
            vk.verify_strict(&d.message, &ed25519_dalek::Signature::from_bytes(&d.signatures[0])).unwrap();
            assert_eq!(tx::b58(&d.signatures[0]), b.signature);
        }
    }

    #[tokio::test]
    async fn dt_zero_keeps_designed_first_member_first() {
        let r = runner();
        let bh = Blockhash { hash: [7; 32], last_valid_height: 1, slot: 1, fetched_ms: 0 };
        let q = r
            .build_quad(&row("C1", 0, "other", "EQ", 0, "other"), PairType::C1, PairType::EQ, FeeScale::S1, 0, &bh)
            .unwrap();
        let roles: Vec<_> = q.iter().map(|b| b.role).collect();
        assert_eq!(roles, vec!["main_other", "main_pref", "control_other", "control_pref"]);
        let q = r
            .build_quad(
                &row("R_IND", 0, "pref", "V_HI", 0, "pref"),
                PairType::RInd,
                PairType::VHi,
                FeeScale::S10,
                1,
                &bh,
            )
            .unwrap();
        let roles: Vec<_> = q.iter().map(|b| b.role).collect();
        assert_eq!(roles, vec!["main_pref", "main_other", "control_pref", "control_other"]);
        // R_IND members transfer to themselves; the v1 control still pays its partner
        let get = |role: &str| q.iter().find(|b| b.role == role).unwrap();
        assert_eq!(get("main_pref").to, get("main_pref").payer);
        assert_eq!(get("main_other").to, get("main_other").payer);
        assert_eq!(get("control_pref").probe, Probe::Hi1);
        for b in &q {
            assert_eq!(tx::b58(&tx::decode(&b.bytes).unwrap().signatures[0]), b.signature, "{}", b.role);
        }
        assert_eq!(get("control_pref").to, get("control_other").payer);
        assert_eq!(tx::decode(&get("main_pref").bytes).unwrap().accounts.len(), 3);
    }
}

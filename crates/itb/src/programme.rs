//! The live programme (D-012, D-019): setup → smoke → pilot → pilot rules → Memo → sessions → sweep, in one process
//! started by the service. Every stage is recorded in the ledger's meta table when it finishes, so a restart resumes
//! at the first unfinished stage; inside a session, quads already reserved are never fired again.
//!
//! A failed check is a *stop*: the process exits with code 3 and the service wrapper writes the STOP marker so the
//! service does not restart into the same failure.

use crate::auth::{LiveAuthorization, LiveChecks, MAINNET_GENESIS, MAINNET_SWEEP_TO};
use crate::clock::SlotClock;
use crate::design::{Campaign, Design, DesignRow};
use crate::keys::{self, Keypair};
use crate::ledger::Ledger;
use crate::live::run_id;
use crate::money;
use crate::reconcile::{reconcile, settle};
use crate::rpc::Rpc;
use crate::run::{Live, Logs, Plan, Refill, Runner};
use crate::tx;
use crate::tx::library::{quad_worst_case_fee, FeeScale};
use crate::util::{log, sha256_file, utc_from_ms, utc_now, wall_ms, Jsonl};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{Datelike, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const EXIT_STOPPED: i32 = 3;

/// A deliberate stop by a coded check (exit code 3, no restart).
#[derive(Debug)]
pub struct Stopped(pub String);

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stopped: {}", self.0)
    }
}

impl std::error::Error for Stopped {}

fn stop<T>(why: impl Into<String>) -> Result<T> {
    Err(Stopped(why.into()).into())
}

pub struct Args {
    pub live: bool,
    pub allow_non_mainnet: bool,
    pub max_spend_lamports: u64,
    pub design: PathBuf,
    pub state: PathBuf,
    pub secrets: PathBuf,
    pub funder_key: PathBuf,
    /// localnet rehearsal: every window is this many minutes, back to back from now
    pub rehearsal_minutes: Option<u64>,
    /// localnet rehearsal: all targets are this identity
    pub map_all_to: Option<String>,
}

/// Pilot decision (frozen rules in docs/PREREGISTRATION.md §Pilot).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PilotDecision {
    pub fee_scale: FeeScale,
    pub v1_enabled: bool,
    /// target slot -> identity probed in the campaign
    pub targets: BTreeMap<String, String>,
    pub swaps: Vec<Value>,
    pub landing: Value,
    pub stop_reasons: Vec<String>,
    pub utc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlannedSession {
    pub id: String,
    pub window: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// One pilot probe's outcome for the pilot rules.
struct PilotProbe {
    target: String,
    scale: String,
    v1: bool,
    /// landed in a slot of the quad's turn whose leader is the probed identity
    in_turn: bool,
}

pub struct Programme {
    a: Args,
    design: Design,
    rpc: Arc<Rpc>,
    ledger: Arc<Mutex<Ledger>>,
    clock: SlotClock,
    funder: Keypair,
    genesis: String,
}

fn stage_key(name: &str) -> String {
    format!("stage:{name}")
}

impl Programme {
    pub async fn start(a: Args) -> Result<Programme> {
        let design = Design::load(&a.design)?;
        let rpc = Arc::new(Rpc::from_env()?);
        let genesis = rpc.genesis_hash().await?;
        if genesis != design.campaign.genesis_hash && !a.allow_non_mainnet {
            bail!("cluster genesis {genesis} is not the design's {}", design.campaign.genesis_hash);
        }
        if genesis == MAINNET_GENESIS && (a.rehearsal_minutes.is_some() || a.map_all_to.is_some()) {
            bail!("--rehearsal-minutes and --map-all-to are refused on mainnet");
        }
        let ledger = Ledger::open(&a.state)?;
        let funder = Keypair::load(&a.funder_key)?;
        if genesis == MAINNET_GENESIS && funder.pubkey_b58() != design.campaign.funder {
            bail!("funder key {} is not the design's funder {}", funder.pubkey_b58(), design.campaign.funder);
        }
        // wallet rule: the sweep address is compiled in for mainnet and can never change once recorded
        let sweep_to = &design.campaign.sweep_to;
        tx::pubkey_from_b58(sweep_to)?;
        if genesis == MAINNET_GENESIS && sweep_to != MAINNET_SWEEP_TO {
            bail!("campaign sweep_to {sweep_to} is not the compiled sweep address {MAINNET_SWEEP_TO}");
        }
        match ledger.meta("sweep_to")? {
            Some(prev) if prev != *sweep_to => return stop(format!("sweep_to changed from {prev} to {sweep_to}")),
            Some(_) => {}
            None => ledger.set_meta("sweep_to", sweep_to)?,
        }
        // the design may only change before the Memo has committed to it
        match ledger.meta("design_sha256")? {
            Some(prev) if prev != design.design_sha256 => {
                if ledger.meta(&stage_key("memo"))?.is_some() {
                    return stop(format!("design.jsonl changed ({prev} -> {}) after the Memo", design.design_sha256));
                }
                ledger.event("design_changed", &json!({"from": prev, "to": design.design_sha256}))?;
                ledger.set_meta("design_sha256", &design.design_sha256)?;
            }
            None => ledger.set_meta("design_sha256", &design.design_sha256)?,
            _ => {}
        }
        let slots_dir = a.state.join("runs").join("slots");
        let sink = Arc::new(Jsonl::open(&slots_dir.join(format!("{}.jsonl", run_id("slots"))), false)?);
        let clock = SlotClock::start(SlotClock::urls_from_env(), Some(sink));
        Ok(Programme { a, design, rpc, ledger: Arc::new(Mutex::new(ledger)), clock, funder, genesis })
    }

    fn done(&self, stage: &str) -> Result<bool> {
        Ok(self.ledger.lock().unwrap().meta(&stage_key(stage))?.is_some())
    }

    fn mark(&self, stage: &str, detail: Value) -> Result<()> {
        let l = self.ledger.lock().unwrap();
        l.set_meta(&stage_key(stage), &utc_now())?;
        l.event(&format!("stage_done:{stage}"), &detail)?;
        log("stage_done", json!({"stage": stage, "detail": detail}));
        Ok(())
    }

    fn meta_json<T: for<'de> Deserialize<'de>>(&self, k: &str) -> Result<Option<T>> {
        match self.ledger.lock().unwrap().meta(k)? {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    fn set_meta_json<T: Serialize>(&self, k: &str, v: &T) -> Result<()> {
        self.ledger.lock().unwrap().set_meta(k, &serde_json::to_string(v)?)
    }

    /// Grant a live authorization for one stage, after a clean reconciliation.
    async fn authorize(&self, sub_budget: &str, worst_case: u64) -> Result<LiveAuthorization> {
        let earlier = self.ledger.lock().unwrap().unexplained_count()?;
        if earlier > 0 {
            return stop(format!("{earlier} unexplained balance drop(s) in the ledger: investigate before any send"));
        }
        let rec = reconcile(&self.rpc, &self.ledger).await?;
        if !rec.mismatches.is_empty() {
            return stop(format!("unexplained balance drop: {:?}", rec.mismatches));
        }
        let rec = if rec.clean { rec } else { settle(&self.rpc, &self.ledger, Duration::from_secs(600)).await? };
        let slot = self.rpc.slot("confirmed").await?;
        let l = self.ledger.lock().unwrap();
        LiveAuthorization::grant(&LiveChecks {
            live_flag: self.a.live,
            allow_non_mainnet: self.a.allow_non_mainnet,
            max_spend_lamports: self.a.max_spend_lamports,
            genesis_hash: &self.genesis,
            current_slot: slot,
            sub_budget,
            key_present: true,
            reconciliation_clean: rec.clean,
            charged_total: l.charged_total()?,
            charged_sub_budget: l.charged_sub(sub_budget)?,
            worst_case,
        })
    }

    fn payers(&self) -> Result<Vec<Keypair>> {
        keys::load_or_create_payers(&self.a.secrets)
    }

    pub async fn run(&self) -> Result<()> {
        log(
            "programme_start",
            json!({"utc": utc_now(), "genesis": self.genesis, "design_sha256": self.design.design_sha256,
                                       "funder": self.funder.pubkey_b58(), "rehearsal": self.a.rehearsal_minutes}),
        );
        if wall_ms() > self.campaign_end_ms()? && self.a.rehearsal_minutes.is_none() && !self.done("campaign")? {
            log("campaign_end_passed", json!({}));
        }
        if !self.done("setup")? {
            self.setup().await?;
        }
        if !self.done("smoke")? {
            self.smoke().await?;
        }
        if !self.done("pilot")? {
            self.pilot().await?;
        }
        if !self.done("memo")? {
            self.memo().await?;
        }
        if !self.done("campaign")? {
            self.campaign().await?;
        }
        if !self.done("sweep")? {
            self.sweep().await?;
        }
        std::fs::write(self.a.state.join("DONE"), utc_now())?;
        log("programme_done", json!({"utc": utc_now()}));
        Ok(())
    }

    fn campaign_end_ms(&self) -> Result<i64> {
        Ok(chrono::DateTime::parse_from_rfc3339(&self.design.campaign.campaign_end_utc)?.timestamp_millis())
    }

    async fn setup(&self) -> Result<()> {
        let payers = self.payers()?;
        let mut keys = vec![(self.funder.pubkey(), "funder".to_string())];
        keys.extend(payers.iter().enumerate().map(|(i, p)| (p.pubkey(), format!("payer-{i:02}"))));
        let pks: Vec<_> = keys.iter().map(|(k, _)| *k).collect();
        let (slot, bals) = self.rpc.balances(&pks).await?;
        {
            let l = self.ledger.lock().unwrap();
            for ((k, role), b) in keys.iter().zip(&bals) {
                if l.add_account(&tx::b58(k), role, *b, slot)? {
                    log("account_registered", json!({"pubkey": tx::b58(k), "role": role, "baseline": b, "slot": slot}));
                }
            }
        }
        let float = self.design.campaign.payer_float_lamports;
        let need: u64 = bals[1..].iter().map(|b| float.saturating_sub(*b)).sum();
        if need + money::PLAIN_FEE > bals[0] {
            return stop(format!("funder has {} lamports, funding needs {}", bals[0], need + money::PLAIN_FEE));
        }
        let auth = self.authorize("setup", money::PLAIN_FEE).await?;
        let n = money::top_up(&self.rpc, &self.ledger, &auth, &run_id("setup"), &self.funder, &payers, float, float)
            .await?;
        let (_, after) = self.rpc.balances(&pks[1..]).await?;
        if after.iter().any(|b| *b < float) {
            bail!("funding did not land (balances {after:?})");
        }
        self.mark("setup", json!({"funded_payers": n, "float": float}))
    }

    fn runner(&self, stage: &str, live: Option<Live>, payers: Vec<Keypair>) -> Result<Arc<Runner>> {
        let rid = run_id(stage);
        let logs = Logs::open(&self.a.state.join("runs").join(&rid))?;
        Runner::new(self.rpc.clone(), self.clock.clone(), live, payers, logs, rid)
    }

    fn window_end(&self, default_minutes: u64) -> i64 {
        wall_ms() + 60_000 * self.a.rehearsal_minutes.unwrap_or(default_minutes) as i64
    }

    fn targets(&self, base: &BTreeMap<String, String>) -> HashMap<String, String> {
        base.iter().map(|(k, v)| (k.clone(), self.a.map_all_to.clone().unwrap_or_else(|| v.clone()))).collect()
    }

    fn primaries(&self) -> BTreeMap<String, String> {
        self.design.targets.iter().map(|t| (t.slot.clone(), t.identity.clone())).collect()
    }

    fn pending_rows(&self, session: &str) -> Result<Vec<DesignRow>> {
        let done = self.ledger.lock().unwrap().attempted_quads()?;
        Ok(self.design.rows_for(session).into_iter().filter(|r| !done.contains(&r.quad_id)).collect())
    }

    /// Fire a list of rows as one session (live).
    #[allow(clippy::too_many_arguments)]
    async fn fire(
        &self,
        stage: &str,
        session: &str,
        rows: Vec<DesignRow>,
        sub_budget: &str,
        targets: HashMap<String, String>,
        window_end_ms: i64,
        scale: FeeScale,
        v1: bool,
    ) -> Result<crate::run::SessionResult> {
        let worst_row = rows
            .iter()
            .map(|r| quad_worst_case_fee(r.main().unwrap(), r.control().unwrap(), r.fee_scale.unwrap_or(scale)))
            .max()
            .unwrap_or(0);
        let batch = self.design.campaign.batch_quads;
        let worst = worst_row * rows.len().min(batch) as u64;
        let auth = self.authorize(sub_budget, worst).await?;
        let refill = Refill {
            auth: self.authorize("setup", money::PLAIN_FEE).await?,
            funder: Keypair::load(&self.a.funder_key)?,
            float: self.design.campaign.payer_float_lamports,
            low: self.design.campaign.payer_low_lamports,
        };
        let live = Live { auth, ledger: self.ledger.clone(), sub_budget: sub_budget.to_string(), refill: Some(refill) };
        let runner = self.runner(stage, Some(live), self.payers()?)?;
        let mut ids: Vec<String> = targets.values().cloned().collect();
        ids.sort();
        ids.dedup();
        runner.start_background(ids);
        runner.wait_ready(Duration::from_secs(300)).await?;
        let res = runner
            .run_session(Plan {
                session: session.into(),
                rows,
                targets,
                window_end_ms,
                fee_scale: scale,
                v1_enabled: v1,
                batch_quads: batch,
            })
            .await;
        runner.shutdown();
        res
    }

    async fn smoke(&self) -> Result<()> {
        let rows = self.pending_rows("smoke")?;
        let targets = self.targets(&self.primaries());
        let res = self.fire("smoke", "smoke", rows, "smoke", targets, self.window_end(30), FeeScale::S1, false).await?;
        let r = settle(&self.rpc, &self.ledger, Duration::from_secs(600)).await?;
        let landed = self.landed_probes("smoke")?;
        if landed.is_empty() {
            return stop("smoke: no probe landed");
        }
        let bad = self.check_fees(&landed).await?;
        if !bad.is_empty() {
            return stop(format!("smoke: fee or status check failed for {bad:?}"));
        }
        self.mark("smoke", json!({"fired": res.fired.len(), "landed": landed.len(), "reconcile": r}))
    }

    /// (signature, expected fee) of every landed probe of a session, from the ledger and the run logs.
    fn landed_probes(&self, session: &str) -> Result<Vec<(String, u64)>> {
        let quads = read_quads(&self.a.state.join("runs"), session)?;
        let sigs: HashSet<String> = quads
            .iter()
            .flat_map(|q| q["probes"].as_array().cloned().unwrap_or_default())
            .filter_map(|p| p["signature"].as_str().map(String::from))
            .collect();
        let l = self.ledger.lock().unwrap();
        Ok(l.all_rows()?
            .into_iter()
            .filter(|r| r.kind == "probe" && r.status == "landed" && sigs.contains(&r.signature))
            .map(|r| (r.signature, r.fee))
            .collect())
    }

    /// getTransaction for each landed probe: fee = formula and no error. Returns the failures.
    async fn check_fees(&self, landed: &[(String, u64)]) -> Result<Vec<String>> {
        let mut bad = Vec::new();
        for (sig, fee) in landed {
            let v = self.rpc.get_transaction(sig).await?;
            let got = v["meta"]["fee"].as_u64();
            if got != Some(*fee) || !v["meta"]["err"].is_null() {
                bad.push(format!("{sig}: fee {got:?} expected {fee}, err {}", v["meta"]["err"]));
            }
        }
        Ok(bad)
    }

    async fn pilot(&self) -> Result<()> {
        if self.meta_json::<PilotDecision>("pilot_decision")?.is_none() {
            let rows = self.pending_rows("pilot")?;
            let targets = self.targets(&self.primaries());
            if !rows.is_empty() {
                self.fire("pilot", "pilot", rows, "pilot", targets, self.window_end(180), FeeScale::S1, true).await?;
            }
            settle(&self.rpc, &self.ledger, Duration::from_secs(900)).await?;
            let d = self.pilot_rules().await?;
            std::fs::write(self.a.state.join("pilot_decision.json"), serde_json::to_vec_pretty(&d)?)?;
            self.set_meta_json("pilot_decision", &d)?;
        }
        let d: PilotDecision = self.meta_json("pilot_decision")?.unwrap();
        if !d.stop_reasons.is_empty() {
            return stop(format!("pilot checks failed: {:?}", d.stop_reasons));
        }
        self.mark("pilot", json!(d))
    }

    /// The frozen pilot rules (D-007, D-012; docs/PREREGISTRATION.md §Pilot).
    async fn pilot_rules(&self) -> Result<PilotDecision> {
        let quads = read_quads(&self.a.state.join("runs"), "pilot")?;
        let rows = self.ledger.lock().unwrap().all_rows()?;
        let status: HashMap<String, (String, Option<u64>)> =
            rows.into_iter().map(|r| (r.signature, (r.status, r.slot))).collect();
        let mut probes = Vec::new();
        let mut leader_cache: HashMap<u64, Vec<String>> = HashMap::new();
        for q in &quads {
            let slot0 = q["slot0"].as_u64().unwrap_or(0);
            let identity = q["identity"].as_str().unwrap_or("").to_string();
            let target = q["target_slot"].as_str().unwrap_or("").to_string();
            let scale = q["fee_scale"].as_str().unwrap_or("s1").to_string();
            if let std::collections::hash_map::Entry::Vacant(e) = leader_cache.entry(slot0) {
                e.insert(self.rpc.slot_leaders(slot0, 4).await.unwrap_or_default());
            }
            let leaders = &leader_cache[&slot0];
            for p in q["probes"].as_array().into_iter().flatten() {
                let sig = p["signature"].as_str().unwrap_or("");
                let name = p["probe"].as_str().unwrap_or("");
                let in_turn = match status.get(sig) {
                    Some((s, Some(slot))) if s == "landed" && *slot >= slot0 && *slot < slot0 + 4 => {
                        leaders.get((*slot - slot0) as usize).map(|l| *l == identity).unwrap_or(false)
                    }
                    _ => false,
                };
                probes.push(PilotProbe {
                    target: target.clone(),
                    scale: scale.clone(),
                    v1: name.ends_with('1'),
                    in_turn,
                });
            }
        }
        let rate = |f: &dyn Fn(&PilotProbe) -> bool| -> (usize, usize, f64) {
            let sel: Vec<_> = probes.iter().filter(|p| f(p)).collect();
            let n = sel.len();
            let k = sel.iter().filter(|p| p.in_turn).count();
            (k, n, if n > 0 { k as f64 / n as f64 } else { 0.0 })
        };
        let mut stop_reasons = Vec::new();
        let all = rate(&|_| true);
        if all.0 == 0 {
            stop_reasons.push("no pilot probe landed in a target turn".to_string());
        }
        // fee scale: s10 if s1 lands at under 70 % of the s10 rate
        let s1 = rate(&|p| p.scale == "s1" && !p.v1);
        let s10 = rate(&|p| p.scale == "s10" && !p.v1);
        let fee_scale = if s1.2 < 0.7 * s10.2 { FeeScale::S10 } else { FeeScale::S1 };
        // v1 gate (D-007): v1 lands on >= 6 targets and at >= 50 % of the legacy rate
        let legacy = rate(&|p| !p.v1);
        let v1 = rate(&|p| p.v1);
        let v1_targets: HashSet<&str> =
            probes.iter().filter(|p| p.v1 && p.in_turn).map(|p| p.target.as_str()).collect();
        let v1_enabled = v1_targets.len() >= 6 && v1.2 >= 0.5 * legacy.2;
        // per target: landing < 50 % -> alternate, if its TPU answers a QUIC handshake
        let mut targets = self.primaries();
        let mut swaps = Vec::new();
        let mut per_target = serde_json::Map::new();
        let snap = crate::gossip::snapshot(&self.rpc).await?;
        let qc = crate::quic::QuicClient::new()?;
        for t in &self.design.targets {
            let r = rate(&|p| p.target == t.slot);
            per_target.insert(t.slot.clone(), json!({"landed": r.0, "sent": r.1, "rate": r.2}));
            // a target whose pilot rows never fired (unreachable at its turns) counts as 0 % landing
            let planned = self.design.rows.iter().any(|d| d.session == "pilot" && d.target_slot == t.slot);
            if (r.1 > 0 && r.2 < 0.5) || (r.1 == 0 && planned) {
                let mut swapped = false;
                if let Some(alt) = &t.alternate {
                    if let Some(addr) = snap.view(alt).tpu_quic {
                        if qc.connect(&addr, Duration::from_secs(3)).await.is_ok() {
                            targets.insert(t.slot.clone(), alt.clone());
                            swapped = true;
                        }
                    }
                }
                swaps.push(json!({"target": t.slot, "from": t.identity, "to": if swapped { t.alternate.clone() } else { None },
                                  "rate": r.2, "reason": if swapped { "landing < 50 %" } else { "landing < 50 %, no working alternate" }}));
            }
        }
        // every landed pilot probe: fee = formula and no error
        let landed = self.landed_probes("pilot")?;
        let bad = self.check_fees(&landed).await?;
        if !bad.is_empty() {
            stop_reasons.push(format!("fee/status check failed: {bad:?}"));
        }
        let rec = reconcile(&self.rpc, &self.ledger).await?;
        if !rec.clean || !rec.mismatches.is_empty() {
            stop_reasons.push(format!("ledger != chain after the pilot: {:?}", rec.mismatches));
        }
        Ok(PilotDecision {
            fee_scale,
            v1_enabled,
            targets,
            swaps,
            landing: json!({"all": all, "s1_legacy": s1, "s10_legacy": s10, "legacy": legacy, "v1": v1,
                            "v1_targets": v1_targets, "per_target": per_target}),
            stop_reasons,
            utc: utc_now(),
        })
    }

    fn memo_text(&self) -> Result<String> {
        let d = &self.a.design;
        let pilot = sha256_file(&self.a.state.join("pilot_decision.json"))?;
        Ok(format!(
            "inside-the-block prereg design.jsonl={} campaign.json={} PREREGISTRATION.md={} pilot_decision={}",
            sha256_file(&d.join("design.jsonl"))?,
            sha256_file(&d.join("campaign.json"))?,
            self.design.campaign.prereg_sha256,
            pilot
        ))
    }

    async fn memo(&self) -> Result<()> {
        let text = self.memo_text()?;
        let auth = self.authorize("setup", money::PLAIN_FEE).await?;
        let sig = money::memo(&self.rpc, &self.ledger, &auth, &run_id("memo"), &self.funder, &text).await?;
        self.ledger.lock().unwrap().set_meta("memo_signature", &sig)?;
        self.mark("memo", json!({"signature": sig, "text": text}))
    }

    /// Sessions at the next occurrence of each window type (or back to back in a rehearsal), stored once.
    fn session_plan(&self) -> Result<Vec<PlannedSession>> {
        if let Some(p) = self.meta_json::<Vec<PlannedSession>>("session_plan")? {
            return Ok(p);
        }
        let plan = match self.a.rehearsal_minutes {
            Some(m) => {
                let mut t = wall_ms() + 60_000;
                self.design
                    .campaign
                    .sessions
                    .iter()
                    .map(|s| {
                        let p = PlannedSession {
                            id: s.id.clone(),
                            window: s.window.clone(),
                            start_ms: t,
                            end_ms: t + m as i64 * 60_000,
                        };
                        t = p.end_ms + 60_000;
                        p
                    })
                    .collect()
            }
            None => plan_sessions(&self.design.campaign, wall_ms())?,
        };
        self.set_meta_json("session_plan", &plan)?;
        self.ledger.lock().unwrap().event("session_plan", &json!(plan))?;
        Ok(plan)
    }

    async fn campaign(&self) -> Result<()> {
        let d: PilotDecision = self.meta_json("pilot_decision")?.ok_or_else(|| anyhow!("no pilot decision"))?;
        let plan = self.session_plan()?;
        log(
            "session_plan",
            json!(plan
                .iter()
                .map(|p| json!({"id": p.id, "start": utc_from_ms(p.start_ms), "end": utc_from_ms(p.end_ms)}))
                .collect::<Vec<_>>()),
        );
        let targets = self.targets(&d.targets);
        let carry_to =
            self.design.campaign.rest.get("carry_unfired_to").and_then(|v| v.as_str()).unwrap_or("D").to_string();
        let scheduled: HashSet<&str> = plan.iter().map(|p| p.id.as_str()).collect();
        let mut carried: Vec<DesignRow> = Vec::new();
        // sessions that could not be scheduled at all carry their rows to D
        for s in &self.design.campaign.sessions {
            if !scheduled.contains(s.id.as_str()) && s.id != carry_to {
                carried.extend(self.pending_rows(&s.id)?);
            }
        }
        for p in &plan {
            if self.done(&format!("session:{}", p.id))? {
                continue;
            }
            if wall_ms() >= p.end_ms {
                log("session_window_missed", json!({"session": p.id}));
                if p.id != carry_to {
                    carried.extend(self.pending_rows(&p.id)?);
                }
                self.mark(&format!("session:{}", p.id), json!({"missed": true}))?;
                continue;
            }
            while wall_ms() < p.start_ms {
                tokio::time::sleep(Duration::from_secs(20)).await;
            }
            let mut rows = self.pending_rows(&p.id)?;
            if p.id == carry_to {
                let done = self.ledger.lock().unwrap().attempted_quads()?;
                rows.extend(carried.drain(..).filter(|r| !done.contains(&r.quad_id)));
                rows = self.fit_budget(rows, d.fee_scale)?;
            }
            for r in rows.iter_mut() {
                r.fee_scale = Some(d.fee_scale);
            }
            log("session_start", json!({"session": p.id, "rows": rows.len()}));
            let res = self
                .fire(
                    &format!("session-{}", p.id),
                    &p.id,
                    rows,
                    "campaign",
                    targets.clone(),
                    p.end_ms,
                    d.fee_scale,
                    d.v1_enabled,
                )
                .await?;
            if p.id != carry_to {
                carried.extend(res.unfired_rows.iter().cloned());
            }
            self.mark(
                &format!("session:{}", p.id),
                json!({"fired": res.fired.len(), "skipped": res.skipped.len(),
                                                             "unfired": res.unfired.len()}),
            )?;
        }
        settle(&self.rpc, &self.ledger, Duration::from_secs(900)).await?;
        self.mark("campaign", json!({"sessions": plan.len(), "unfired_carried_not_run": carried.len()}))
    }

    /// Keep the rows (in their random firing order) whose worst-case fees fit in the campaign sub-budget left.
    fn fit_budget(&self, rows: Vec<DesignRow>, scale: FeeScale) -> Result<Vec<DesignRow>> {
        let l = self.ledger.lock().unwrap();
        let left = crate::auth::sub_budget_cap("campaign")?
            .saturating_sub(l.charged_sub("campaign")?)
            .min(self.a.max_spend_lamports.saturating_sub(l.charged_total()?));
        let mut used = 0u64;
        let mut out = Vec::new();
        let n = rows.len();
        for r in rows {
            let w = quad_worst_case_fee(r.main()?, r.control()?, scale);
            if used + w <= left {
                used += w;
                out.push(r);
            }
        }
        if out.len() < n {
            log("rows_trimmed_to_budget", json!({"kept": out.len(), "dropped": n - out.len(), "budget_left": left}));
        }
        Ok(out)
    }

    async fn sweep(&self) -> Result<()> {
        let payers = self.payers()?;
        let to = tx::pubkey_from_b58(&self.design.campaign.sweep_to)?;
        let auth = self.authorize("setup", money::PLAIN_FEE * (payers.len() as u64 + 1)).await?;
        for round in 0..3 {
            money::sweep(&self.rpc, &self.ledger, &auth, &run_id("sweep"), &self.funder, &payers, &to).await?;
            let mut pks: Vec<_> = payers.iter().map(|p| p.pubkey()).collect();
            pks.push(self.funder.pubkey());
            let (_, bals) = self.rpc.balances(&pks).await?;
            if bals.iter().all(|b| *b <= money::PLAIN_FEE) {
                let l = self.ledger.lock().unwrap();
                let detail =
                    json!({"round": round, "charged_total": l.charged_total()?, "landed_fees": l.landed_total()?});
                l.export_csv(&self.a.state.join("ledger.csv"))?;
                drop(l);
                return self.mark("sweep", detail);
            }
        }
        bail!("sweep left balances behind")
    }
}

/// Every fired quad record of a session in the run logs under `runs`.
pub fn read_quads(runs: &Path, session: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(runs) else {
        return Ok(out);
    };
    let mut dirs: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let p = d.join("quads.jsonl");
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v["session"] == session && v["status"] == "fired" && v["live"] == true {
                out.push(v);
            }
        }
    }
    Ok(out)
}

fn hm(s: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").with_context(|| format!("bad time {s}"))
}

/// Next occurrence of each window type after `after_ms`: sessions in design order take the earliest window of their
/// type that does not overlap an earlier choice and still has `min_minutes` left; the replication session (D) must
/// start after B ends and on another day. Windows are clipped at the campaign end. Returned in time order.
pub fn plan_sessions(c: &Campaign, after_ms: i64) -> Result<Vec<PlannedSession>> {
    let end_ms = chrono::DateTime::parse_from_rfc3339(&c.campaign_end_utc)?.timestamp_millis();
    let after = Utc.timestamp_millis_opt(after_ms).single().ok_or_else(|| anyhow!("bad time"))?;
    let mut chosen: Vec<PlannedSession> = Vec::new();
    for s in &c.sessions {
        let (st, en) = (hm(&s.start_utc)?, hm(&s.end_utc)?);
        let b_end = chosen.iter().find(|p| p.id == "B").map(|p| p.end_ms);
        let mut day = after.date_naive();
        let mut pick = None;
        while Utc.from_utc_datetime(&day.and_time(st)).timestamp_millis() < end_ms {
            if s.weekdays.contains(&day.weekday().number_from_monday()) {
                let ws = Utc.from_utc_datetime(&day.and_time(st)).timestamp_millis();
                let we = Utc.from_utc_datetime(&day.and_time(en)).timestamp_millis().min(end_ms);
                let start = ws.max(after_ms);
                let ok_len = we - start >= s.min_minutes as i64 * 60_000;
                let overlaps = chosen.iter().any(|p| start < p.end_ms && p.start_ms < we);
                let ok_d = s.id != "D"
                    || match b_end {
                        Some(be) => start >= be && Utc.timestamp_millis_opt(be).unwrap().date_naive() != day,
                        None => true,
                    };
                if ok_len && !overlaps && ok_d {
                    pick = Some(PlannedSession {
                        id: s.id.clone(),
                        window: s.window.clone(),
                        start_ms: start,
                        end_ms: we,
                    });
                    break;
                }
            }
            day = day.succ_opt().unwrap();
        }
        match pick {
            Some(p) => chosen.push(p),
            None => log("session_not_schedulable", json!({"session": s.id})),
        }
    }
    chosen.sort_by_key(|p| p.start_ms);
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::SessionWindow;

    fn campaign() -> Campaign {
        let w = |id: &str, window: &str, days: &[u32], s: &str, e: &str, m: u32| SessionWindow {
            id: id.into(),
            window: window.into(),
            weekdays: days.to_vec(),
            start_utc: s.into(),
            end_utc: e.into(),
            min_minutes: m,
        };
        Campaign {
            study: "t".into(),
            genesis_hash: String::new(),
            cap_lamports: 0,
            sweep_to: String::new(),
            funder: String::new(),
            campaign_end_utc: "2026-11-03T23:59:00Z".into(),
            sessions: vec![
                w("A", "weekend_day", &[6, 7], "10:00", "14:00", 120),
                w("B", "weekday_busy", &[1, 2, 3, 4, 5], "14:00", "18:00", 120),
                w("C", "night", &[1, 2, 3, 4, 5, 6, 7], "00:00", "05:00", 150),
                w("D", "replication", &[1, 2, 3, 4, 5], "14:00", "17:00", 90),
            ],
            payer_float_lamports: 0,
            payer_low_lamports: 0,
            batch_quads: 25,
            design_sha256: String::new(),
            targets_sha256: String::new(),
            prereg_sha256: String::new(),
            rest: Default::default(),
        }
    }

    fn ms(s: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }

    fn ids(p: &[PlannedSession]) -> Vec<(String, String)> {
        p.iter().map(|x| (x.id.clone(), utc_from_ms(x.start_ms))).collect()
    }

    #[test]
    fn saturday_evening_trigger() {
        let p = plan_sessions(&campaign(), ms("2026-10-31T19:00:00Z")).unwrap();
        assert_eq!(
            ids(&p),
            vec![
                ("C".into(), "2026-11-01T00:00:00.000Z".into()),
                ("A".into(), "2026-11-01T10:00:00.000Z".into()),
                ("B".into(), "2026-11-02T14:00:00.000Z".into()),
                ("D".into(), "2026-11-03T14:00:00.000Z".into()),
            ]
        );
    }

    #[test]
    fn monday_noon_trigger_drops_the_weekend() {
        let p = plan_sessions(&campaign(), ms("2026-11-02T12:00:00Z")).unwrap();
        assert_eq!(
            ids(&p),
            vec![
                ("B".into(), "2026-11-02T14:00:00.000Z".into()),
                ("C".into(), "2026-11-03T00:00:00.000Z".into()),
                ("D".into(), "2026-11-03T14:00:00.000Z".into()),
            ]
        );
    }

    #[test]
    fn late_start_needs_min_minutes() {
        // Sunday 12:30: A has only 90 min left (< 120), so no weekend session remains
        let p = plan_sessions(&campaign(), ms("2026-11-01T12:30:00Z")).unwrap();
        assert!(p.iter().all(|x| x.id != "A"));
        // Sunday 11:00: A starts late with 180 min
        let p = plan_sessions(&campaign(), ms("2026-11-01T11:00:00Z")).unwrap();
        assert_eq!(p[0].id, "A");
        assert_eq!(utc_from_ms(p[0].start_ms), "2026-11-01T11:00:00.000Z");
    }
}

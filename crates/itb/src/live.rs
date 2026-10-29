//! Session driver for dry runs (and, with a ledger and a live authorization, for the live programme).

use crate::clock::SlotClock;
use crate::design::Design;
use crate::rpc::Rpc;
use crate::run::{Logs, Plan, Runner};
use crate::tx::library::FeeScale;
use crate::util::{log, utc_now, wall_ms, Jsonl};
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub fn run_id(kind: &str) -> String {
    format!("{}-{kind}", utc_now().replace([':', '-'], "").replace('.', "").trim_end_matches('Z'))
}

/// Primary identity of every design target slot (alternates are chosen by the pilot rules).
pub fn primary_targets(d: &Design) -> HashMap<String, String> {
    d.targets.iter().map(|t| (t.slot.clone(), t.identity.clone())).collect()
}

pub struct DryArgs {
    pub design: PathBuf,
    pub sessions: Vec<String>,
    pub minutes: u64,
    pub out_root: PathBuf,
    pub fee_scale: FeeScale,
    pub v1: bool,
    pub map_all_to: Option<String>,
}

/// Dry run: the real schedule, slot clock, QUIC connections and signed quads, nothing written to a socket and
/// nothing reserved (throw-away signers). The rows of all given sessions are spread over `minutes`.
pub async fn dry_run(a: DryArgs) -> Result<()> {
    let design = Design::load(&a.design)?;
    let rid = run_id("dry");
    let dir = a.out_root.join(&rid);
    let logs = Logs::open(&dir)?;
    let slots = Arc::new(Jsonl::open(&dir.join("slots.jsonl"), false)?);
    let rpc = Arc::new(Rpc::from_env()?);
    let clock = SlotClock::start(SlotClock::urls_from_env(), Some(slots));
    let mut targets = primary_targets(&design);
    if let Some(id) = &a.map_all_to {
        for v in targets.values_mut() {
            *v = id.clone();
        }
    }
    let runner = Runner::new(rpc, clock, None, crate::keys::ephemeral_payers(), logs, rid.clone())?;
    let mut ids: Vec<String> = targets.values().cloned().collect();
    ids.sort();
    ids.dedup();
    runner.start_background(ids);
    runner.wait_ready(Duration::from_secs(180)).await?;
    let rows: Vec<_> = design.rows.iter().filter(|r| a.sessions.contains(&r.session)).cloned().collect();
    log("dry_run_start", json!({"run_id": rid, "dir": dir, "rows": rows.len(), "minutes": a.minutes}));
    let plan = Plan {
        session: a.sessions.join("+"),
        rows,
        targets,
        window_end_ms: wall_ms() + (a.minutes * 60_000) as i64,
        fee_scale: a.fee_scale,
        v1_enabled: a.v1,
        batch_quads: usize::MAX,
    };
    let res = runner.run_session(plan).await?;
    let summary = json!({"run_id": rid, "fired": res.fired.len(), "skipped": res.skipped.len(), "unfired": res.unfired.len(),
                         "skip_reasons": count(res.skipped.iter().map(|(_, r)| r.as_str()))});
    std::fs::write(dir.join("summary.json"), serde_json::to_vec_pretty(&summary)?)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn count<'a>(it: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for k in it {
        *m.entry(k.to_string()).or_default() += 1;
    }
    m
}

pub fn default_out_root() -> PathBuf {
    std::env::var("ITB_STATE_DIR").map(|s| Path::new(&s).join("runs")).unwrap_or_else(|_| PathBuf::from("state/runs"))
}

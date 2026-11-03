//! Reconciliation: resolve pending transactions from chain status, then compare every owned account's
//! on-chain balance with what the ledger predicts. Any unexplained drop is charged and stops sending.

use crate::ledger::Ledger;
use crate::rpc::Rpc;
use crate::tx::pubkey_from_b58;
use crate::util::log;
use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;

/// Blocks past `lastValidBlockHeight` before a transaction unknown to the chain counts as expired.
pub const EXPIRY_MARGIN_BLOCKS: u64 = 30;

#[derive(Debug, Serialize)]
pub struct Report {
    pub slot: u64,
    pub clean: bool,
    pub pending: usize,
    pub landed_now: usize,
    pub expired_now: usize,
    pub mismatches: Vec<(String, i128)>,
    pub foreign: Vec<(String, u64)>,
    pub charged_total: u64,
    pub landed_total: u64,
}

pub async fn reconcile(rpc: &Rpc, ledger: &Mutex<Ledger>) -> Result<Report> {
    let pending = ledger.lock().unwrap().pending()?;
    let sigs: Vec<String> = pending.iter().map(|r| r.signature.clone()).collect();
    let statuses = if sigs.is_empty() { vec![] } else { rpc.signature_statuses(&sigs).await? };
    let height = rpc.block_height().await?;
    let (mut landed_now, mut expired_now) = (0, 0);
    let mut still_pending = Vec::new();
    {
        let l = ledger.lock().unwrap();
        for (row, st) in pending.iter().zip(statuses) {
            match st {
                Some(s) if s.confirmation == "confirmed" || s.confirmation == "finalized" => {
                    l.set_landed(&row.signature, s.slot, s.err.as_deref())?;
                    landed_now += 1;
                }
                Some(_) => still_pending.push(row.fee_payer.clone()),
                None if height > row.last_valid_height + EXPIRY_MARGIN_BLOCKS => {
                    l.set_expired(&row.signature)?;
                    expired_now += 1;
                }
                None => still_pending.push(row.fee_payer.clone()),
            }
        }
    }
    let accounts = ledger.lock().unwrap().accounts()?;
    let keys: Vec<_> = accounts.iter().map(|(pk, _, _)| pubkey_from_b58(pk)).collect::<Result<_>>()?;
    let (slot, balances) = if keys.is_empty() { (0, vec![]) } else { rpc.balances(&keys).await? };
    let l = ledger.lock().unwrap();
    let expected = l.expected_balances()?;
    let mut mismatches = Vec::new();
    let mut foreign = Vec::new();
    let mut clean = still_pending.is_empty();
    for ((pk, _, _), onchain) in accounts.iter().zip(balances) {
        let exp = expected.get(pk).copied().unwrap_or(0);
        let diff = onchain as i128 - exp;
        if diff == 0 {
            continue;
        }
        if still_pending.contains(pk) {
            clean = false; // an account with in-flight transactions cannot be judged yet
            continue;
        }
        if diff > 0 {
            l.record_foreign(pk, diff as u64, slot)?;
            foreign.push((pk.clone(), diff as u64));
        } else {
            l.charge_unexplained(pk, (-diff) as u64, slot)?;
            mismatches.push((pk.clone(), diff));
            clean = false;
        }
    }
    let report = Report {
        slot,
        clean,
        pending: still_pending.len(),
        landed_now,
        expired_now,
        mismatches,
        foreign,
        charged_total: l.charged_total()?,
        landed_total: l.landed_total()?,
    };
    l.record_reconciliation(slot, clean, &serde_json::to_value(&report)?)?;
    drop(l);
    log("reconcile", json!(report));
    Ok(report)
}

/// Clear unexplained drops that the chain explains (D-038; a human runs `itb resolve-unexplained` with the service
/// stopped): every `expired` row is looked up again, and the ones that landed after all become `landed`, but only if
/// every owned account then matches the chain to the lamport. Otherwise the ledger is left as it is.
pub async fn resolve_unexplained(rpc: &Rpc, ledger: &Mutex<Ledger>) -> Result<serde_json::Value> {
    let (pending, expired, held) = {
        let l = ledger.lock().unwrap();
        (l.pending()?.len(), l.expired()?, l.unexplained_count()?)
    };
    if held == 0 {
        anyhow::bail!("no unexplained rows in the ledger");
    }
    if pending > 0 {
        anyhow::bail!("{pending} transactions still pending: let the ledger settle first");
    }
    let sigs: Vec<String> = expired.iter().map(|r| r.signature.clone()).collect();
    let statuses = if sigs.is_empty() { vec![] } else { rpc.signature_statuses(&sigs).await? };
    let landed: Vec<(String, u64, Option<String>)> = expired
        .iter()
        .zip(statuses)
        .filter_map(|(r, st)| match st {
            Some(s) if s.confirmation == "confirmed" || s.confirmation == "finalized" => {
                Some((r.signature.clone(), s.slot, s.err))
            }
            _ => None,
        })
        .collect();
    let accounts = ledger.lock().unwrap().accounts()?;
    let keys: Vec<_> = accounts.iter().map(|(pk, _, _)| pubkey_from_b58(pk)).collect::<Result<_>>()?;
    let (slot, balances) = rpc.balances(&keys).await?;
    let onchain = accounts.iter().map(|(pk, _, _)| pk.clone()).zip(balances).collect();
    let l = ledger.lock().unwrap();
    let lamports = l.resolve_unexplained(&landed, &onchain)?;
    let detail = json!({"slot": slot, "unexplained_rows": held, "lamports": lamports,
                        "now_landed": landed.iter().map(|(s, _, _)| s).collect::<Vec<_>>()});
    l.event("unexplained_resolved", &detail)?;
    drop(l);
    log("unexplained_resolved", detail.clone());
    Ok(detail)
}

/// Reconcile repeatedly until nothing is pending (every blockhash expired or landed). Errors on any
/// unexplained balance drop or if it does not settle within `max_wait`.
pub async fn settle(rpc: &Rpc, ledger: &Mutex<Ledger>, max_wait: Duration) -> Result<Report> {
    let t0 = std::time::Instant::now();
    loop {
        let r = reconcile(rpc, ledger).await?;
        if !r.mismatches.is_empty() {
            return Err(crate::programme::Stopped(format!("unexplained balance drop: {:?}", r.mismatches)).into());
        }
        if r.clean && r.pending == 0 {
            return Ok(r);
        }
        if t0.elapsed() > max_wait {
            anyhow::bail!("ledger did not settle within {:?} ({} pending)", max_wait, r.pending);
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

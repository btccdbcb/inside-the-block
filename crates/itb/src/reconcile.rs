//! Reconciliation: resolve pending transactions from chain status, then compare every owned account's
//! on-chain balance with what the ledger predicts. Any unexplained drop is charged and stops sending.

use crate::ledger::Ledger;
use crate::rpc::Rpc;
use crate::tx::pubkey_from_b58;
use crate::util::log;
use anyhow::Result;
use serde::Serialize;
use serde_json::json;

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

pub async fn reconcile(rpc: &Rpc, ledger: &Ledger) -> Result<Report> {
    let pending = ledger.pending()?;
    let sigs: Vec<String> = pending.iter().map(|r| r.signature.clone()).collect();
    let statuses = if sigs.is_empty() { vec![] } else { rpc.signature_statuses(&sigs).await? };
    let height = rpc.block_height().await?;
    let (mut landed_now, mut expired_now) = (0, 0);
    let mut still_pending = Vec::new();
    for (row, st) in pending.iter().zip(statuses) {
        match st {
            Some(s) if s.confirmation == "confirmed" || s.confirmation == "finalized" => {
                ledger.set_landed(&row.signature, s.slot, s.err.as_deref())?;
                landed_now += 1;
            }
            Some(_) => still_pending.push(row.fee_payer.clone()),
            None if height > row.last_valid_height + EXPIRY_MARGIN_BLOCKS => {
                ledger.set_expired(&row.signature)?;
                expired_now += 1;
            }
            None => still_pending.push(row.fee_payer.clone()),
        }
    }
    let accounts = ledger.accounts()?;
    let keys: Vec<_> = accounts.iter().map(|(pk, _, _)| pubkey_from_b58(pk)).collect::<Result<_>>()?;
    let (slot, balances) = if keys.is_empty() { (0, vec![]) } else { rpc.balances(&keys).await? };
    let expected = ledger.expected_balances()?;
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
            clean = false; // cannot judge an account with in-flight transactions
            continue;
        }
        if diff > 0 {
            ledger.record_foreign(pk, diff as u64, slot)?;
            foreign.push((pk.clone(), diff as u64));
        } else {
            ledger.charge_unexplained(pk, (-diff) as u64, slot)?;
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
        charged_total: ledger.charged_total()?,
        landed_total: ledger.landed_total()?,
    };
    ledger.record_reconciliation(slot, clean, &serde_json::to_value(&report)?)?;
    log("reconcile", json!(report));
    Ok(report)
}

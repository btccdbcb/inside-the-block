//! Funding, top-ups, the pre-registration Memo and the sweep. Each one is a single signed transaction sent
//! through RPC, reserved in the ledger first and resolved by reconciliation like every probe.

use crate::auth::LiveAuthorization;
use crate::keys::Keypair;
use crate::ledger::{Ledger, Reservation};
use crate::reconcile::settle;
use crate::rpc::Rpc;
use crate::run::b64;
use crate::tx::{self, legacy, Pubkey};
use crate::util::log;
use anyhow::{bail, Result};
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;

/// Base fee of a one-signature transaction without a priority fee.
pub const PLAIN_FEE: u64 = 5_000;

/// Sign, reserve, send (re-sending the same bytes every 2 s until the chain knows it or it expires).
#[allow(clippy::too_many_arguments)]
pub async fn send_tracked(
    rpc: &Rpc,
    ledger: &Mutex<Ledger>,
    auth: &LiveAuthorization,
    kind: &str,
    sub_budget: &str,
    run_id: &str,
    msg: &legacy::Message,
    signer: &Keypair,
    transfers: Vec<(String, String, u64)>,
) -> Result<String> {
    let (bh, lvh, _) = rpc.latest_blockhash().await?;
    let mut msg = msg.clone();
    msg.blockhash = bh;
    let bytes = msg.sign(&[&signer.signing])?;
    let sig = tx::b58(&bytes[1..65]);
    let r = Reservation {
        kind: kind.into(),
        sub_budget: sub_budget.into(),
        run_id: run_id.into(),
        signature: sig.clone(),
        fee_payer: signer.pubkey_b58(),
        blockhash: tx::b58(&bh),
        last_valid_height: lvh,
        fee: PLAIN_FEE,
        transfers,
    };
    ledger.lock().unwrap().reserve_many(&[r], auth.limit())?;
    let tx_b64 = b64(&bytes);
    log("money_send", json!({"kind": kind, "signature": sig, "tx_b64": tx_b64}));
    ledger.lock().unwrap().set_status(&sig, "sent")?;
    for attempt in 0..30 {
        if let Err(e) = rpc.send_transaction(auth, &tx_b64).await {
            log(
                "money_send_error",
                json!({"kind": kind, "signature": sig, "attempt": attempt, "error": format!("{e:#}")}),
            );
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Some(Some(s)) = rpc.signature_statuses(std::slice::from_ref(&sig)).await?.into_iter().next() {
            if s.confirmation == "confirmed" || s.confirmation == "finalized" {
                break;
            }
        }
        if rpc.block_height().await? > lvh {
            break;
        }
    }
    Ok(sig)
}

/// Bring every payer whose balance is below `low` up to `float` in one transaction from the funder.
#[allow(clippy::too_many_arguments)]
pub async fn top_up(
    rpc: &Rpc,
    ledger: &Mutex<Ledger>,
    auth: &LiveAuthorization,
    run_id: &str,
    funder: &Keypair,
    payers: &[Keypair],
    float: u64,
    low: u64,
) -> Result<usize> {
    let keys: Vec<Pubkey> = payers.iter().map(|p| p.pubkey()).collect();
    let (_, bals) = rpc.balances(&keys).await?;
    let need: Vec<(Pubkey, u64)> =
        keys.iter().zip(&bals).filter(|(_, b)| **b < low).map(|(k, b)| (*k, float - b)).collect();
    if need.is_empty() {
        return Ok(0);
    }
    let total: u64 = need.iter().map(|(_, a)| a).sum();
    let (_, fb) = rpc.balances(&[funder.pubkey()]).await?;
    if fb[0] < total + PLAIN_FEE {
        bail!("funder balance {} too low for a top-up of {total}", fb[0]);
    }
    let transfers = need.iter().map(|(k, a)| (funder.pubkey_b58(), tx::b58(k), *a)).collect();
    let msg = legacy::fund_message(&funder.pubkey(), &need, &[0; 32]);
    send_tracked(rpc, ledger, auth, "fund", "setup", run_id, &msg, funder, transfers).await?;
    let r = settle(rpc, ledger, Duration::from_secs(300)).await?;
    log("top_up", json!({"payers": need.len(), "lamports": total, "reconcile_clean": r.clean}));
    Ok(need.len())
}

/// Post the Memo; returns its signature once landed.
pub async fn memo(
    rpc: &Rpc,
    ledger: &Mutex<Ledger>,
    auth: &LiveAuthorization,
    run_id: &str,
    funder: &Keypair,
    text: &str,
) -> Result<String> {
    let msg = legacy::memo_message(&funder.pubkey(), text, &[0; 32])?;
    let sig = send_tracked(rpc, ledger, auth, "memo", "setup", run_id, &msg, funder, vec![]).await?;
    settle(rpc, ledger, Duration::from_secs(300)).await?;
    let st = rpc.signature_statuses(std::slice::from_ref(&sig)).await?;
    match st.into_iter().next().flatten() {
        Some(s) if s.err.is_none() => Ok(sig),
        _ => bail!("memo {sig} did not land"),
    }
}

/// Move everything from the payers and then the funder to `to`. Refuses while anything is pending.
pub async fn sweep(
    rpc: &Rpc,
    ledger: &Mutex<Ledger>,
    auth: &LiveAuthorization,
    run_id: &str,
    funder: &Keypair,
    payers: &[Keypair],
    to: &Pubkey,
) -> Result<()> {
    let r = settle(rpc, ledger, Duration::from_secs(900)).await?;
    if r.pending > 0 {
        bail!("sweep refused: {} transactions still pending", r.pending);
    }
    for p in payers.iter().chain(std::iter::once(funder)) {
        let (_, b) = rpc.balances(&[p.pubkey()]).await?;
        if b[0] <= PLAIN_FEE {
            continue;
        }
        let amount = b[0] - PLAIN_FEE;
        let msg = legacy::transfer_message(&p.pubkey(), to, amount, &[0; 32]);
        send_tracked(rpc, ledger, auth, "sweep", "setup", run_id, &msg, p, vec![(p.pubkey_b58(), tx::b58(to), amount)])
            .await?;
    }
    let r = settle(rpc, ledger, Duration::from_secs(300)).await?;
    if !r.clean {
        bail!("ledger and chain disagree after the sweep");
    }
    Ok(())
}

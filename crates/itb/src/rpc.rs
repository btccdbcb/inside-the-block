//! JSON-RPC over a provider endpoint (ITB_RPC_URL, key in the URL, from the environment only) with the keyless public
//! endpoints as fallback, or over the public pool alone: per-endpoint and per-method pacing, failover, no custom
//! headers, chunked status queries. Sending a transaction needs a `LiveAuthorization`.

use crate::auth::LiveAuthorization;
use crate::tx::Pubkey;
use crate::util::{log, redact_url};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

pub const DEFAULT_RPC: &[&str] = &[
    "https://api.mainnet-beta.solana.com",
    "https://solana-rpc.publicnode.com",
    "https://public.rpc.solanavibestation.com",
];

const GAP_ANY: Duration = Duration::from_millis(120);
const GAP_METHOD: Duration = Duration::from_millis(300);

#[derive(Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}
impl std::error::Error for RpcError {}

#[derive(Default)]
struct EpState {
    next_any: Option<Instant>,
    next_by_method: HashMap<String, Instant>,
    backoff_until: Option<Instant>,
    failures: u32,
}

struct Endpoint {
    url: String,
    state: Mutex<EpState>,
}

pub struct Rpc {
    client: reqwest::Client,
    endpoints: Vec<Endpoint>,
    rr: AtomicUsize,
    /// the first endpoint is the provider: every call starts there and falls back in order
    ordered: bool,
}

fn is_throttle(code: i64, msg: &str) -> bool {
    code == 429 || code == -32429 || code == -32005 || msg.to_lowercase().contains("too many requests")
}

impl Rpc {
    pub fn new(urls: &[String]) -> Result<Self> {
        if urls.is_empty() {
            bail!("no RPC endpoints");
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(30))
            .build()?;
        let endpoints =
            urls.iter().map(|u| Endpoint { url: u.clone(), state: Mutex::new(EpState::default()) }).collect();
        Ok(Rpc { client, endpoints, rr: AtomicUsize::new(0), ordered: false })
    }

    /// ITB_RPC_URL (the provider, tried first) followed by ITB_RPC_URLS (comma separated) or the keyless pool.
    pub fn from_env() -> Result<Self> {
        let mut urls: Vec<String> = match std::env::var("ITB_RPC_URLS") {
            Ok(v) if !v.trim().is_empty() => v.split(',').map(|s| s.trim().to_string()).collect(),
            _ => DEFAULT_RPC.iter().map(|s| s.to_string()).collect(),
        };
        let provider = std::env::var("ITB_RPC_URL").unwrap_or_default();
        let ordered = !provider.trim().is_empty();
        if ordered {
            urls.insert(0, provider.trim().to_string());
        }
        let mut rpc = Rpc::new(&urls)?;
        rpc.ordered = ordered;
        Ok(rpc)
    }

    pub fn hosts(&self) -> Vec<String> {
        self.endpoints.iter().map(|e| redact_url(&e.url)).collect()
    }

    async fn pace(&self, i: usize, method: &str) {
        let wait_until = {
            let mut st = self.endpoints[i].state.lock().await;
            let now = Instant::now();
            let mut t = now;
            if let Some(x) = st.next_any {
                t = t.max(x);
            }
            if let Some(x) = st.next_by_method.get(method) {
                t = t.max(*x);
            }
            if let Some(x) = st.backoff_until {
                t = t.max(x);
            }
            st.next_any = Some(t + GAP_ANY);
            st.next_by_method.insert(method.to_string(), t + GAP_METHOD);
            t
        };
        tokio::time::sleep_until(wait_until).await;
    }

    async fn penalise(&self, i: usize) {
        let mut st = self.endpoints[i].state.lock().await;
        st.failures += 1;
        let secs = (2u64 << st.failures.min(4)).min(30);
        st.backoff_until = Some(Instant::now() + Duration::from_secs(secs));
    }

    async fn reward(&self, i: usize) {
        let mut st = self.endpoints[i].state.lock().await;
        st.failures = 0;
    }

    /// Call one endpoint once.
    pub async fn call_on(&self, i: usize, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.pace(i, method).await;
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let resp = self.client.post(&self.endpoints[i].url).json(&body).timeout(timeout).send().await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                self.penalise(i).await;
                bail!("{} {method}: {}", redact_url(&self.endpoints[i].url), e.without_url());
            }
        };
        let status = resp.status();
        if status.as_u16() == 429 || status.is_server_error() {
            self.penalise(i).await;
            return Err(RpcError { code: status.as_u16() as i64, message: format!("HTTP {status}") }.into());
        }
        let v: Value = resp.json().await.map_err(|e| anyhow!("{method}: bad JSON: {}", e.without_url()))?;
        if let Some(err) = v.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
            if is_throttle(code, &message) {
                self.penalise(i).await;
            }
            return Err(RpcError { code, message }.into());
        }
        self.reward(i).await;
        v.get("result").cloned().ok_or_else(|| anyhow!("{method}: no result"))
    }

    /// Call with failover across endpoints. Only JSON-RPC errors that are not throttling are returned
    /// immediately (they are answers, not failures).
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let timeout = match method {
            "getBlock" | "getClusterNodes" | "getVoteAccounts" | "getLeaderSchedule" => Duration::from_secs(30),
            _ => Duration::from_secs(10),
        };
        let n = self.endpoints.len();
        let mut last = anyhow!("no attempt");
        for attempt in 0..(3 * n) {
            let i = if self.ordered { attempt % n } else { self.rr.fetch_add(1, Ordering::Relaxed) % n };
            match self.call_on(i, method, params.clone(), timeout).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if let Some(re) = e.downcast_ref::<RpcError>() {
                        if !is_throttle(re.code, &re.message) && re.code != 503 && re.code != 502 && re.code != 500 {
                            return Err(e);
                        }
                    }
                    if attempt + 1 == 3 * n {
                        log("rpc_give_up", json!({"method": method, "error": e.to_string()}));
                    }
                    last = e;
                }
            }
        }
        Err(last)
    }

    pub async fn genesis_hash(&self) -> Result<String> {
        Ok(self.call("getGenesisHash", json!([])).await?.as_str().unwrap_or_default().to_string())
    }

    pub async fn slot(&self, commitment: &str) -> Result<u64> {
        self.call("getSlot", json!([{"commitment": commitment}])).await?.as_u64().ok_or_else(|| anyhow!("getSlot"))
    }

    /// Current block height, capped by the newest blockhash's lastValidBlockHeight − 150 from a second call. One public
    /// endpoint answers getBlockHeight with the slot number (seen 2026-11-03), which made every pending transaction look
    /// expired at once (REPORT §8). A low value only delays an expiry; a high one releases a transaction that can still land.
    pub async fn block_height(&self) -> Result<u64> {
        let h = self
            .call("getBlockHeight", json!([{"commitment": "confirmed"}]))
            .await?
            .as_u64()
            .ok_or_else(|| anyhow!("getBlockHeight"))?;
        let (_, lvh, _) = self.latest_blockhash().await?;
        capped_height(h, lvh)
    }

    pub async fn epoch_info(&self) -> Result<Value> {
        self.call("getEpochInfo", json!([{"commitment": "confirmed"}])).await
    }

    /// (blockhash bytes, last valid block height, context slot)
    pub async fn latest_blockhash(&self) -> Result<([u8; 32], u64, u64)> {
        let v = self.call("getLatestBlockhash", json!([{"commitment": "confirmed"}])).await?;
        let bh = v["value"]["blockhash"].as_str().ok_or_else(|| anyhow!("no blockhash"))?;
        let lvh = v["value"]["lastValidBlockHeight"].as_u64().unwrap_or(0);
        let slot = v["context"]["slot"].as_u64().unwrap_or(0);
        Ok((crate::tx::pubkey_from_b58(bh)?, lvh, slot))
    }

    /// Leader slots (absolute) of one identity in the epoch that contains `slot`.
    pub async fn leader_slots(&self, slot: u64, identity: &str, epoch_first_slot: u64) -> Result<Vec<u64>> {
        let v = self.call("getLeaderSchedule", json!([slot, {"identity": identity}])).await?;
        if v.is_null() {
            return Ok(vec![]);
        }
        let mut out: Vec<u64> = v
            .get(identity)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_u64()).map(|i| i + epoch_first_slot).collect())
            .unwrap_or_default();
        out.sort_unstable();
        Ok(out)
    }

    pub async fn slot_leaders(&self, start: u64, limit: u64) -> Result<Vec<String>> {
        let v = self.call("getSlotLeaders", json!([start, limit])).await?;
        Ok(v.as_array()
            .ok_or_else(|| anyhow!("getSlotLeaders"))?
            .iter()
            .map(|s| s.as_str().unwrap_or("").to_string())
            .collect())
    }

    /// Lamport balances via getMultipleAccounts (None = account does not exist). Returns (context slot, balances).
    pub async fn balances(&self, keys: &[Pubkey]) -> Result<(u64, Vec<u64>)> {
        let mut out = Vec::with_capacity(keys.len());
        let mut slot = 0;
        for chunk in keys.chunks(100) {
            let ks: Vec<String> = chunk.iter().map(|k| crate::tx::b58(k)).collect();
            let v = self.call("getMultipleAccounts", json!([ks, {"commitment": "confirmed", "encoding": "base64", "dataSlice": {"offset": 0, "length": 0}}])).await?;
            slot = slot.max(v["context"]["slot"].as_u64().unwrap_or(0));
            for a in v["value"].as_array().ok_or_else(|| anyhow!("getMultipleAccounts"))? {
                out.push(if a.is_null() { 0 } else { a["lamports"].as_u64().unwrap_or(0) });
            }
        }
        Ok((slot, out))
    }

    /// Signature statuses in chunks of 32. `None` = unknown to the node.
    pub async fn signature_statuses(&self, sigs: &[String]) -> Result<Vec<Option<SigStatus>>> {
        let mut out = Vec::with_capacity(sigs.len());
        for chunk in sigs.chunks(32) {
            let v = self.call("getSignatureStatuses", json!([chunk, {"searchTransactionHistory": true}])).await?;
            for s in v["value"].as_array().ok_or_else(|| anyhow!("getSignatureStatuses"))? {
                if s.is_null() {
                    out.push(None);
                } else {
                    out.push(Some(SigStatus {
                        slot: s["slot"].as_u64().unwrap_or(0),
                        err: if s["err"].is_null() { None } else { Some(s["err"].to_string()) },
                        confirmation: s["confirmationStatus"].as_str().unwrap_or("").to_string(),
                    }));
                }
            }
        }
        Ok(out)
    }

    pub async fn simulate(&self, tx_b64: &str, sig_verify: bool) -> Result<Value> {
        self.call(
            "simulateTransaction",
            json!([tx_b64, {"encoding": "base64", "sigVerify": sig_verify,
            "replaceRecentBlockhash": !sig_verify, "commitment": "confirmed"}]),
        )
        .await
    }

    pub async fn min_rent_exempt(&self, data_len: u64) -> Result<u64> {
        self.call("getMinimumBalanceForRentExemption", json!([data_len])).await?.as_u64().ok_or_else(|| anyhow!("rent"))
    }

    pub async fn get_transaction(&self, sig: &str) -> Result<Value> {
        self.call(
            "getTransaction",
            json!([sig, {"encoding": "base64", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]),
        )
        .await
    }

    /// Signatures in block order plus the Fee-reward recipient (the leader that produced the block).
    pub async fn block_signatures(&self, slot: u64) -> Result<Option<(Vec<String>, String)>> {
        let r = self
            .call(
                "getBlock",
                json!([slot, {"encoding": "base64", "transactionDetails": "signatures",
            "rewards": true, "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]),
            )
            .await;
        match r {
            Ok(v) => {
                let sigs = v["signatures"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let leader = v["rewards"]
                    .as_array()
                    .and_then(|a| a.iter().find(|r| r["rewardType"] == "Fee"))
                    .and_then(|r| r["pubkey"].as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(Some((sigs, leader)))
            }
            Err(e) => match e.downcast_ref::<RpcError>() {
                // -32007 slot skipped, -32009 missing in long-term storage, -32004 block not available
                Some(re) if [-32007, -32009, -32004].contains(&re.code) => Ok(None),
                _ => Err(e),
            },
        }
    }

    /// Submit a signed transaction through RPC (funding, Memo, sweep). Needs a live authorization.
    pub async fn send_transaction(&self, _auth: &LiveAuthorization, tx_b64: &str) -> Result<String> {
        let params = json!([tx_b64, {"encoding": "base64", "skipPreflight": false, "preflightCommitment": "confirmed", "maxRetries": 0}]);
        let mut last = anyhow!("not sent");
        for i in 0..self.endpoints.len() {
            match self.call_on(i, "sendTransaction", params.clone(), Duration::from_secs(15)).await {
                Ok(v) => return v.as_str().map(String::from).ok_or_else(|| anyhow!("sendTransaction: no signature")),
                Err(e) => last = e,
            }
        }
        Err(last).context("sendTransaction failed on every endpoint")
    }
}

#[derive(Clone, Debug)]
pub struct SigStatus {
    pub slot: u64,
    pub err: Option<String>,
    pub confirmation: String,
}

/// Blocks a blockhash stays valid for (Agave MAX_PROCESSING_AGE): getLatestBlockhash returns height + this.
pub const MAX_PROCESSING_AGE: u64 = 150;

/// `height` from getBlockHeight, never above what the newest blockhash's `lvh` implies.
pub fn capped_height(height: u64, lvh: u64) -> Result<u64> {
    if lvh == 0 {
        bail!("getLatestBlockhash without lastValidBlockHeight");
    }
    Ok(height.min(lvh.saturating_sub(MAX_PROCESSING_AGE)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_is_capped_by_the_newest_blockhash() {
        // 2026-11-03: one endpoint answered getBlockHeight with the slot (464942124) while the others said 442962886
        assert_eq!(capped_height(464_942_124, 442_963_034).unwrap(), 442_962_884);
        assert_eq!(capped_height(442_962_886, 442_963_038).unwrap(), 442_962_886);
        assert!(capped_height(442_962_886, 0).is_err());
    }
}

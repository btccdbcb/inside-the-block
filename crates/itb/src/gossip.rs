//! Who is who at measurement time: gossip (clientId, version, tpuQuic), stake, ingress kind,
//! Jito Kobe `running_bam`, and validators.app `software_client` when a token is set.

use crate::rpc::Rpc;
use crate::util::{log, utc_now};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub client_id: String,
    pub version: String,
    pub tpu_quic: Option<String>,
    pub gossip: Option<String>,
    pub stake: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub utc: String,
    pub nodes: HashMap<String, Node>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TargetView {
    pub identity: String,
    pub client_id: Option<String>,
    pub version: Option<String>,
    pub tpu_quic: Option<String>,
    pub stake: u64,
    /// Number of staked nodes advertising the same TPU host IP.
    pub host_share: usize,
    /// direct | shared | missing
    pub ingress_kind: String,
}

fn host(addr: &str) -> &str {
    addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr)
}

pub async fn snapshot(rpc: &Rpc) -> Result<Snapshot> {
    let cn = rpc.call("getClusterNodes", json!([])).await?;
    let va = rpc.call("getVoteAccounts", json!([{"commitment": "confirmed"}])).await?;
    let mut stake: HashMap<String, u64> = HashMap::new();
    for key in ["current", "delinquent"] {
        for v in va[key].as_array().into_iter().flatten() {
            if let Some(id) = v["nodePubkey"].as_str() {
                *stake.entry(id.to_string()).or_default() += v["activatedStake"].as_u64().unwrap_or(0);
            }
        }
    }
    let mut nodes = HashMap::new();
    for n in cn.as_array().ok_or_else(|| anyhow!("getClusterNodes"))? {
        let Some(pk) = n["pubkey"].as_str() else {
            continue;
        };
        nodes.insert(
            pk.to_string(),
            Node {
                client_id: n["clientId"].as_str().unwrap_or("").to_string(),
                version: n["version"].as_str().unwrap_or("").to_string(),
                tpu_quic: n["tpuQuic"].as_str().map(String::from),
                gossip: n["gossip"].as_str().map(String::from),
                stake: stake.get(pk).copied().unwrap_or(0),
            },
        );
    }
    Ok(Snapshot { utc: utc_now(), nodes })
}

impl Snapshot {
    pub fn view(&self, identity: &str) -> TargetView {
        let Some(n) = self.nodes.get(identity) else {
            return TargetView {
                identity: identity.into(),
                client_id: None,
                version: None,
                tpu_quic: None,
                stake: 0,
                host_share: 0,
                ingress_kind: "missing".into(),
            };
        };
        let share = n
            .tpu_quic
            .as_deref()
            .map(|tq| {
                let h = host(tq);
                self.nodes.values().filter(|m| m.stake > 0 && m.tpu_quic.as_deref().map(host) == Some(h)).count()
            })
            .unwrap_or(0);
        let own_host = n.tpu_quic.as_deref().map(host) == n.gossip.as_deref().map(host);
        let kind = match (&n.tpu_quic, share) {
            (None, _) => "missing",
            (Some(_), s) if s <= 1 && own_host => "direct",
            (Some(_), s) if s <= 1 => "direct_other_host",
            _ => "shared",
        };
        TargetView {
            identity: identity.into(),
            client_id: Some(n.client_id.clone()),
            version: Some(n.version.clone()),
            tpu_quic: n.tpu_quic.clone(),
            stake: n.stake,
            host_share: share,
            ingress_kind: kind.into(),
        }
    }

    /// Compact table of every node (for the archived snapshot).
    pub fn compact(&self) -> Value {
        let mut rows: Vec<_> = self.nodes.iter().collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        json!({"utc": self.utc, "columns": ["identity", "client_id", "version", "tpu_quic", "gossip", "stake"],
               "rows": rows.iter().map(|(k, n)| json!([k, n.client_id, n.version, n.tpu_quic, n.gossip, n.stake])).collect::<Vec<_>>()})
    }
}

/// Jito Kobe: identity -> running_bam.
pub async fn kobe_running_bam() -> Result<HashMap<String, bool>> {
    let c = reqwest::Client::builder().user_agent(crate::rpc::USER_AGENT).timeout(Duration::from_secs(30)).build()?;
    let v: Value = c.get("https://kobe.mainnet.jito.network/api/v1/validators").send().await?.json().await?;
    let mut out = HashMap::new();
    for x in v["validators"].as_array().into_iter().flatten() {
        if let Some(id) = x["identity_account"].as_str() {
            out.insert(id.to_string(), x["running_bam"].as_bool().unwrap_or(false));
        }
    }
    Ok(out)
}

/// validators.app: identity -> (software_client, software_version). Token from VALIDATORS_APP_TOKEN; skipped if empty.
pub async fn validators_app() -> Result<Option<HashMap<String, (String, String)>>> {
    let token = std::env::var("VALIDATORS_APP_TOKEN").unwrap_or_default();
    if token.trim().is_empty() {
        return Ok(None);
    }
    let c = reqwest::Client::builder().user_agent(crate::rpc::USER_AGENT).timeout(Duration::from_secs(60)).build()?;
    let resp = c
        .get("https://www.validators.app/api/v1/validators/mainnet.json?limit=9999&active_only=true")
        .header("Token", token.trim())
        .send()
        .await?;
    if !resp.status().is_success() {
        log("validators_app_error", json!({"status": resp.status().as_u16()}));
        return Ok(None);
    }
    let v: Value = resp.json().await?;
    let mut out = HashMap::new();
    for x in v.as_array().into_iter().flatten() {
        if let Some(id) = x["account"].as_str() {
            out.insert(
                id.to_string(),
                (
                    x["software_client"].as_str().unwrap_or("").to_string(),
                    x["software_version"].as_str().unwrap_or("").to_string(),
                ),
            );
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_kind_from_host_sharing() {
        let mut nodes = HashMap::new();
        let n = |c: &str, tq: &str, g: &str, s: u64| Node {
            client_id: c.into(),
            version: "4.3.2".into(),
            tpu_quic: Some(tq.into()),
            gossip: Some(g.into()),
            stake: s,
        };
        nodes.insert("D".into(), n("JitoLabs", "1.1.1.1:8002", "1.1.1.1:8001", 5));
        nodes.insert("S1".into(), n("AgaveBam", "9.9.9.9:5010", "2.2.2.2:8001", 5));
        nodes.insert("S2".into(), n("AgaveBam", "9.9.9.9:5010", "3.3.3.3:8001", 5));
        let s = Snapshot { utc: "t".into(), nodes };
        assert_eq!(s.view("D").ingress_kind, "direct");
        assert_eq!(s.view("S1").ingress_kind, "shared");
        assert_eq!(s.view("S1").host_share, 2);
        assert_eq!(s.view("nope").ingress_kind, "missing");
    }
}

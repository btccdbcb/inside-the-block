//! The frozen design: `design/campaign.json` (sessions, caps, sweep address, hashes), `design/targets.json`
//! and `design/design.jsonl` (one row per planned quad with every randomised factor).

use crate::tx::library::{FeeScale, PairType};
use crate::util::sha256_file;
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesignRow {
    pub quad_id: String,
    /// smoke | pilot | A | B | C | D
    pub session: String,
    pub target_slot: String,
    pub seq: u32,
    /// send offset τ from the first shred of slot0 − 1 (D-023)
    pub tau_ms: i32,
    pub main_type: String,
    pub dt_ms: u32,
    /// "pref" (the member reward/cost puts first) or "other" is sent first
    pub main_first: String,
    pub control_type: String,
    pub control_gap_ms: u32,
    pub control_first: String,
    /// payer index within the quad's pool of 4, for roles [main pref, main other, control pref, control other]
    pub payer_perm: [u8; 4],
    /// pilot rows fix the fee scale; campaign rows use the pilot decision
    #[serde(default)]
    pub fee_scale: Option<FeeScale>,
}

impl DesignRow {
    pub fn main(&self) -> Result<PairType> {
        let p = PairType::parse(&self.main_type)?;
        ensure!(p.is_main(), "{} is not a main pair type", self.main_type);
        Ok(p)
    }

    pub fn control(&self) -> Result<PairType> {
        let p = PairType::parse(&self.control_type)?;
        ensure!(!p.is_main(), "{} is not a control pair type", self.control_type);
        Ok(p)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionWindow {
    pub id: String,
    pub window: String,
    /// ISO weekday numbers 1 = Monday .. 7 = Sunday
    pub weekdays: Vec<u32>,
    pub start_utc: String,
    pub end_utc: String,
    /// minimum minutes left in the window to start it late
    pub min_minutes: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub study: String,
    pub genesis_hash: String,
    pub cap_lamports: u64,
    pub sweep_to: String,
    pub funder: String,
    pub campaign_end_utc: String,
    pub sessions: Vec<SessionWindow>,
    /// payers are filled to this at setup and at batch boundaries (rent-exempt minimum + spending float)
    pub payer_float_lamports: u64,
    /// a payer below this is topped up at the next batch boundary
    pub payer_low_lamports: u64,
    pub batch_quads: usize,
    pub design_sha256: String,
    pub targets_sha256: String,
    #[serde(default)]
    pub prereg_sha256: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl Campaign {
    /// The freeze's second pass (`itb-analysis design --prereg`) ran: the pre-registration and the confirmatory code
    /// are hashed into the campaign file, so the Memo commits to them.
    pub fn prereg_complete(&self) -> Result<()> {
        let hex64 = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        ensure!(hex64(&self.prereg_sha256), "campaign.json has no prereg_sha256 (run `itb-analysis design --prereg`)");
        let code = self.rest.get("confirmatory_code_sha256").and_then(|v| v.as_object());
        ensure!(
            code.is_some_and(|m| !m.is_empty() && m.values().all(|v| v.as_str().is_some_and(hex64))),
            "campaign.json has no confirmatory_code_sha256 (run `itb-analysis design --prereg`)"
        );
        Ok(())
    }
}

pub struct Design {
    pub dir: PathBuf,
    pub campaign: Campaign,
    pub rows: Vec<DesignRow>,
    pub targets: Vec<crate::TargetEntry>,
    pub campaign_sha256: String,
    pub design_sha256: String,
}

impl Design {
    /// Load and check the hashes recorded in campaign.json against the files.
    pub fn load(dir: &Path) -> Result<Design> {
        let cpath = dir.join("campaign.json");
        let campaign: Campaign = serde_json::from_str(
            &std::fs::read_to_string(&cpath).with_context(|| format!("read {}", cpath.display()))?,
        )?;
        let dpath = dir.join("design.jsonl");
        let design_sha256 = sha256_file(&dpath)?;
        if design_sha256 != campaign.design_sha256 {
            bail!("design.jsonl sha256 {design_sha256} does not match campaign.json {}", campaign.design_sha256);
        }
        let tpath = dir.join("targets.json");
        let tsha = sha256_file(&tpath)?;
        if tsha != campaign.targets_sha256 {
            bail!("targets.json sha256 {tsha} does not match campaign.json {}", campaign.targets_sha256);
        }
        let mut rows = Vec::new();
        for (i, line) in std::fs::read_to_string(&dpath)?.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let r: DesignRow = serde_json::from_str(line).with_context(|| format!("design.jsonl line {}", i + 1))?;
            r.main()?;
            r.control()?;
            rows.push(r);
        }
        let targets = crate::read_targets(&tpath)?;
        Ok(Design {
            dir: dir.to_path_buf(),
            campaign,
            rows,
            targets,
            campaign_sha256: sha256_file(&cpath)?,
            design_sha256,
        })
    }

    pub fn rows_for(&self, session: &str) -> Vec<DesignRow> {
        self.rows.iter().filter(|r| r.session == session).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign(extra: serde_json::Value) -> Campaign {
        let mut v = json_base();
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    fn json_base() -> serde_json::Value {
        serde_json::json!({"study": "s", "genesis_hash": "g", "cap_lamports": 1, "sweep_to": "t", "funder": "f",
            "campaign_end_utc": "e", "sessions": [], "payer_float_lamports": 1, "payer_low_lamports": 1,
            "batch_quads": 1, "design_sha256": "d", "targets_sha256": "t"})
    }

    #[test]
    fn prereg_complete_needs_both_hashes() {
        let h = "a".repeat(64);
        assert!(campaign(serde_json::json!({})).prereg_complete().is_err());
        assert!(campaign(serde_json::json!({"prereg_sha256": ""})).prereg_complete().is_err());
        assert!(campaign(serde_json::json!({"prereg_sha256": h})).prereg_complete().is_err());
        assert!(campaign(serde_json::json!({"prereg_sha256": h, "confirmatory_code_sha256": {}}))
            .prereg_complete()
            .is_err());
        assert!(campaign(serde_json::json!({"prereg_sha256": "A".repeat(64), "confirmatory_code_sha256": {"x": h}}))
            .prereg_complete()
            .is_err());
        assert!(campaign(serde_json::json!({"prereg_sha256": h, "confirmatory_code_sha256": {"x": "short"}}))
            .prereg_complete()
            .is_err());
        campaign(serde_json::json!({"prereg_sha256": h, "confirmatory_code_sha256": {"x": h}}))
            .prereg_complete()
            .unwrap();
    }
}

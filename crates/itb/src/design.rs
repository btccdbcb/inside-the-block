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
    pub phi_ms: i32,
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
    pub payer_float_lamports: u64,
    pub batch_quads: usize,
    pub design_sha256: String,
    pub targets_sha256: String,
    #[serde(default)]
    pub prereg_sha256: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
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

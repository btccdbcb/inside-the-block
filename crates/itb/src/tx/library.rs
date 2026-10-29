//! The probe parameter library and the pair contrasts.
//!
//! Each contrast is built so that the candidate ordering keys disagree about which member should go first.
//! The tests check the predicted winner of every contrast under every key, at both fee scales.

use super::cost::{probe_cost, probe_fee, probe_priority};
use super::{Format, ProbeParams};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const KIB32: u32 = 32 * 1024;
pub const MIB64: u32 = 64 * 1024 * 1024;

/// Fee scale chosen by the pilot: s1 = the base prices, s10 = ten times the price (same L and D).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeeScale {
    S1,
    S10,
}

impl FeeScale {
    pub fn factor(self) -> u64 {
        match self {
            FeeScale::S1 => 1,
            FeeScale::S10 => 10,
        }
    }
}

/// Named probe shapes. `A1` is A in v1 format with the same cost and reward; `Hi1` is HI in v1 format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Probe {
    A,
    B,
    #[serde(rename = "Aprime")]
    APrime,
    #[serde(rename = "HI")]
    Hi,
    X,
    Z,
    A1,
    #[serde(rename = "HI1")]
    Hi1,
}

impl Probe {
    pub fn name(self) -> &'static str {
        match self {
            Probe::A => "A",
            Probe::B => "B",
            Probe::APrime => "Aprime",
            Probe::Hi => "HI",
            Probe::X => "X",
            Probe::Z => "Z",
            Probe::A1 => "A1",
            Probe::Hi1 => "HI1",
        }
    }

    pub fn params(self, scale: FeeScale) -> ProbeParams {
        let f = scale.factor();
        match self {
            Probe::A => ProbeParams::legacy(1_000, 100_000 * f, KIB32),
            Probe::B => ProbeParams::legacy(3_000, 50_000 * f, KIB32),
            Probe::APrime => ProbeParams::legacy(1_000, 100_000 * f, MIB64),
            Probe::Hi => ProbeParams::legacy(1_000, 300_000 * f, KIB32),
            Probe::X => ProbeParams::legacy(1_000, 10_000 * f, KIB32),
            Probe::Z => ProbeParams::legacy(1_200, 500_000 * f, KIB32),
            // v1 has 4 fewer instruction-data bytes' worth of cost (3 CU vs 7 CU), so L is 4 higher
            // to keep cost equal; the priority fee is the same number of lamports.
            Probe::A1 => ProbeParams::v1(1_004, Probe::A.params(scale).priority_fee, KIB32),
            Probe::Hi1 => ProbeParams::v1(1_004, Probe::Hi.params(scale).priority_fee, KIB32),
        }
    }

    pub fn format(self) -> Format {
        match self {
            Probe::A1 | Probe::Hi1 => Format::V1,
            _ => Format::Legacy,
        }
    }
}

/// Pair types. Main pairs: R (race), C1, C2, C3, RInd (race without a shared lock). Control pairs: EQ, VEq, VHi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum PairType {
    R,
    C1,
    C2,
    C3,
    #[serde(rename = "R_IND")]
    RInd,
    EQ,
    #[serde(rename = "V_EQ")]
    VEq,
    #[serde(rename = "V_HI")]
    VHi,
}

impl PairType {
    /// (preferred by reward/cost, other). For EQ and V_EQ the two have equal keys.
    pub fn members(self) -> (Probe, Probe) {
        match self {
            PairType::R | PairType::RInd => (Probe::Hi, Probe::APrime),
            PairType::C1 => (Probe::A, Probe::B),
            PairType::C2 => (Probe::B, Probe::APrime),
            PairType::C3 => (Probe::Z, Probe::X),
            PairType::EQ => (Probe::A, Probe::A),
            PairType::VEq => (Probe::A1, Probe::A),
            PairType::VHi => (Probe::Hi1, Probe::APrime),
        }
    }

    pub fn shares_lock(self) -> bool {
        !matches!(self, PairType::RInd)
    }

    pub fn is_main(self) -> bool {
        matches!(self, PairType::R | PairType::C1 | PairType::C2 | PairType::C3 | PairType::RInd)
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "R" => PairType::R,
            "C1" => PairType::C1,
            "C2" => PairType::C2,
            "C3" => PairType::C3,
            "R_IND" => PairType::RInd,
            "EQ" => PairType::EQ,
            "V_EQ" => PairType::VEq,
            "V_HI" => PairType::VHi,
            _ => bail!("unknown pair type {s}"),
        })
    }
}

/// Candidate ordering keys. Higher key goes first, except `Cost` (lower cost first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    CuPrice,
    PriorityFee,
    Cost,
    RewardPerCost,
}

/// Which member each key puts first: Some(true) = first member, Some(false) = second, None = tie.
pub fn key_prefers_first(key: Key, a: &ProbeParams, b: &ProbeParams) -> Option<bool> {
    let (x, y) = match key {
        Key::CuPrice => (a.cu_price_micro as i128, b.cu_price_micro as i128),
        Key::PriorityFee => (a.priority_fee as i128, b.priority_fee as i128),
        Key::Cost => (-(probe_cost(a) as i128), -(probe_cost(b) as i128)),
        Key::RewardPerCost => (probe_priority(a) as i128, probe_priority(b) as i128),
    };
    match x.cmp(&y) {
        std::cmp::Ordering::Greater => Some(true),
        std::cmp::Ordering::Less => Some(false),
        std::cmp::Ordering::Equal => None,
    }
}

/// Worst-case fee of a quad (all four probes land), at a fee scale.
pub fn quad_worst_case_fee(main: PairType, control: PairType, scale: FeeScale) -> u64 {
    let (a, b) = main.members();
    let (c, d) = control.members();
    [a, b, c, d].iter().map(|p| probe_fee(&p.params(scale))).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn winner(pair: PairType, key: Key, scale: FeeScale) -> Option<Probe> {
        let (a, b) = pair.members();
        key_prefers_first(key, &a.params(scale), &b.params(scale)).map(|first| if first { a } else { b })
    }

    #[test]
    fn contrasts_rank_as_designed_under_every_key() {
        use Key::*;
        use Probe::*;
        for scale in [FeeScale::S1, FeeScale::S10] {
            // R: every key agrees HI goes first.
            for k in [CuPrice, PriorityFee, Cost, RewardPerCost] {
                assert_eq!(winner(PairType::R, k, scale), Some(Hi), "{k:?} {scale:?}");
            }
            // C1 (A vs B): only priority fee prefers B.
            assert_eq!(winner(PairType::C1, CuPrice, scale), Some(A));
            assert_eq!(winner(PairType::C1, PriorityFee, scale), Some(B));
            assert_eq!(winner(PairType::C1, Cost, scale), Some(A));
            assert_eq!(winner(PairType::C1, RewardPerCost, scale), Some(A));
            // C2 (B vs A'): only CU price prefers A'.
            assert_eq!(winner(PairType::C2, CuPrice, scale), Some(APrime));
            assert_eq!(winner(PairType::C2, PriorityFee, scale), Some(B));
            assert_eq!(winner(PairType::C2, Cost, scale), Some(B));
            assert_eq!(winner(PairType::C2, RewardPerCost, scale), Some(B));
            // C3 (Z vs X): only cost alone prefers X.
            assert_eq!(winner(PairType::C3, CuPrice, scale), Some(Z));
            assert_eq!(winner(PairType::C3, PriorityFee, scale), Some(Z));
            assert_eq!(winner(PairType::C3, Cost, scale), Some(X));
            assert_eq!(winner(PairType::C3, RewardPerCost, scale), Some(Z));
            // EQ: tie under every key.
            for k in [CuPrice, PriorityFee, Cost, RewardPerCost] {
                assert_eq!(winner(PairType::EQ, k, scale), None);
            }
            // V_EQ: same reward and cost -> tie under the Agave key and under cost.
            assert_eq!(winner(PairType::VEq, RewardPerCost, scale), None);
            assert_eq!(winner(PairType::VEq, Cost, scale), None);
            assert_eq!(winner(PairType::VEq, PriorityFee, scale), None);
            // V_HI: the v1 HI beats the legacy A' under reward/cost.
            assert_eq!(winner(PairType::VHi, RewardPerCost, scale), Some(Hi1));
        }
    }

    #[test]
    fn only_reward_per_cost_gives_the_a_b_z_pattern() {
        use Key::*;
        let pattern = |k| {
            [PairType::C1, PairType::C2, PairType::C3].map(|p| winner(p, k, FeeScale::S1))
        };
        let rc = pattern(RewardPerCost);
        for k in [CuPrice, PriorityFee, Cost] {
            assert_ne!(pattern(k), rc, "{k:?} gives the same pattern as reward/cost");
        }
    }

    #[test]
    fn worst_case_quad_fees() {
        assert_eq!(quad_worst_case_fee(PairType::C3, PairType::EQ, FeeScale::S1), 5_600 + 5_010 + 5_100 + 5_100);
        assert!(quad_worst_case_fee(PairType::C3, PairType::EQ, FeeScale::S10) < 30_000);
    }
}

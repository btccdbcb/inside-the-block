//! The probe parameter library and the pair contrasts.
//!
//! Each contrast is built so that the candidate ordering keys disagree about which member should go first.
//! The tests check the predicted winner of every contrast under every key, at both fee scales.

use super::cost::{probe_cost, probe_fee, probe_priority};
use super::{Format, ProbeParams, Shape};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Fee scale chosen by the pilot: s1 = the base prices, s10 = ten times the price (same L).
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

/// Named probes (PROBES.md). `A1` is A in v1 format with the same cost and reward; `HI1` is HI in v1 format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Probe {
    A,
    B,
    #[serde(rename = "HI")]
    Hi,
    #[serde(rename = "LO")]
    Lo,
    P,
    Q,
    X,
    Z,
    A1,
    #[serde(rename = "HI1")]
    Hi1,
}

impl Probe {
    pub const ALL: [Probe; 10] =
        [Probe::A, Probe::B, Probe::Hi, Probe::Lo, Probe::P, Probe::Q, Probe::X, Probe::Z, Probe::A1, Probe::Hi1];

    pub fn name(self) -> &'static str {
        match self {
            Probe::A => "A",
            Probe::B => "B",
            Probe::Hi => "HI",
            Probe::Lo => "LO",
            Probe::P => "P",
            Probe::Q => "Q",
            Probe::X => "X",
            Probe::Z => "Z",
            Probe::A1 => "A1",
            Probe::Hi1 => "HI1",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match Probe::ALL.iter().find(|p| p.name().eq_ignore_ascii_case(s)) {
            Some(p) => Ok(*p),
            None => bail!("unknown probe {s}"),
        }
    }

    pub fn params(self, scale: FeeScale) -> ProbeParams {
        let f = scale.factor();
        match self {
            Probe::A => ProbeParams::legacy(1_000, 100_000 * f),
            Probe::B => ProbeParams::legacy(6_000, 25_000 * f),
            Probe::Hi => ProbeParams::legacy(1_000, 300_000 * f),
            Probe::Lo => ProbeParams::legacy(1_200, 80_000 * f),
            Probe::P => ProbeParams::legacy(1_000, 200_000 * f),
            Probe::Q => ProbeParams::legacy(4_000, 190_000 * f),
            Probe::X => ProbeParams::legacy(1_000, 10_000 * f),
            Probe::Z => ProbeParams::legacy(1_200, 500_000 * f),
            // v1 carries 3 CU less instruction data (3 vs 6), so L is 3 higher to keep cost equal; the priority
            // fee is the same number of lamports.
            Probe::A1 => ProbeParams::v1(1_003, Probe::A.params(scale).priority_fee),
            Probe::Hi1 => ProbeParams::v1(1_003, Probe::Hi.params(scale).priority_fee),
        }
    }

    pub fn format(self) -> Format {
        match self {
            Probe::A1 | Probe::Hi1 => Format::V1,
            _ => Format::Legacy,
        }
    }
}

/// Pair types. Main pairs: R (race), C1, C2, C3, RInd (race without a shared write lock). Control pairs: EQ, VEq,
/// VHi.
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
            PairType::R | PairType::RInd => (Probe::Hi, Probe::Lo),
            PairType::C1 => (Probe::A, Probe::B),
            PairType::C2 => (Probe::Q, Probe::P),
            PairType::C3 => (Probe::Z, Probe::X),
            PairType::EQ => (Probe::A, Probe::A),
            PairType::VEq => (Probe::A1, Probe::A),
            PairType::VHi => (Probe::Hi1, Probe::Lo),
        }
    }

    /// R_IND members transfer to themselves; every other pair's members transfer to each other.
    pub fn shape(self) -> Shape {
        match self {
            PairType::RInd => Shape::SelfTransfer,
            _ => Shape::Partner,
        }
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

/// Which member each key puts first: Some(true) = first member, Some(false) = second, None = tie. Both members
/// of a pair have the same shape.
pub fn key_prefers_first(key: Key, a: &ProbeParams, b: &ProbeParams, shape: Shape) -> Option<bool> {
    let (x, y) = match key {
        Key::CuPrice => (a.cu_price_micro as i128, b.cu_price_micro as i128),
        Key::PriorityFee => (a.priority_fee as i128, b.priority_fee as i128),
        Key::Cost => (-(probe_cost(a, shape) as i128), -(probe_cost(b, shape) as i128)),
        Key::RewardPerCost => (probe_priority(a, shape) as i128, probe_priority(b, shape) as i128),
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
        key_prefers_first(key, &a.params(scale), &b.params(scale), pair.shape()).map(|first| if first { a } else { b })
    }

    #[test]
    fn contrasts_rank_as_designed_under_every_key() {
        use Key::*;
        use Probe::*;
        for scale in [FeeScale::S1, FeeScale::S10] {
            // R and R_IND: every key agrees HI goes first.
            for k in [CuPrice, PriorityFee, Cost, RewardPerCost] {
                assert_eq!(winner(PairType::R, k, scale), Some(Hi), "{k:?} {scale:?}");
                assert_eq!(winner(PairType::RInd, k, scale), Some(Hi), "{k:?} {scale:?}");
            }
            // C1 (A vs B): only priority fee prefers B.
            assert_eq!(winner(PairType::C1, CuPrice, scale), Some(A));
            assert_eq!(winner(PairType::C1, PriorityFee, scale), Some(B));
            assert_eq!(winner(PairType::C1, Cost, scale), Some(A));
            assert_eq!(winner(PairType::C1, RewardPerCost, scale), Some(A));
            // C2 (Q vs P): CU price and cost alone prefer P.
            assert_eq!(winner(PairType::C2, CuPrice, scale), Some(P));
            assert_eq!(winner(PairType::C2, PriorityFee, scale), Some(Q));
            assert_eq!(winner(PairType::C2, Cost, scale), Some(P));
            assert_eq!(winner(PairType::C2, RewardPerCost, scale), Some(Q));
            // C3 (Z vs X): only cost alone prefers X.
            assert_eq!(winner(PairType::C3, CuPrice, scale), Some(Z));
            assert_eq!(winner(PairType::C3, PriorityFee, scale), Some(Z));
            assert_eq!(winner(PairType::C3, Cost, scale), Some(X));
            assert_eq!(winner(PairType::C3, RewardPerCost, scale), Some(Z));
            // EQ: tie under every key.
            for k in [CuPrice, PriorityFee, Cost, RewardPerCost] {
                assert_eq!(winner(PairType::EQ, k, scale), None);
            }
            // V_EQ: same reward and cost -> tie under the Agave key, cost and fee.
            assert_eq!(winner(PairType::VEq, RewardPerCost, scale), None);
            assert_eq!(winner(PairType::VEq, Cost, scale), None);
            assert_eq!(winner(PairType::VEq, PriorityFee, scale), None);
            // V_HI: the v1 HI beats the legacy LO under reward/cost.
            assert_eq!(winner(PairType::VHi, RewardPerCost, scale), Some(Hi1));
        }
    }

    #[test]
    fn every_key_gives_a_distinct_pattern() {
        use Key::*;
        for scale in [FeeScale::S1, FeeScale::S10] {
            let pattern = |k| [PairType::C1, PairType::C2, PairType::C3].map(|p| winner(p, k, scale));
            let all: Vec<_> = [CuPrice, PriorityFee, Cost, RewardPerCost].map(pattern).to_vec();
            for i in 0..all.len() {
                for j in i + 1..all.len() {
                    assert_ne!(all[i], all[j], "keys {i} and {j} give the same pattern at {scale:?}");
                }
            }
        }
    }

    #[test]
    fn names_roundtrip() {
        for p in Probe::ALL {
            assert_eq!(Probe::parse(p.name()).unwrap(), p);
            assert_eq!(serde_json::to_value(p).unwrap(), serde_json::json!(p.name()));
        }
    }

    #[test]
    fn worst_case_quad_fees() {
        assert_eq!(quad_worst_case_fee(PairType::C3, PairType::EQ, FeeScale::S1), 5_600 + 5_010 + 5_100 + 5_100);
        let worst = [PairType::R, PairType::C1, PairType::C2, PairType::C3, PairType::RInd]
            .iter()
            .flat_map(|m| {
                [PairType::EQ, PairType::VEq, PairType::VHi].map(|c| quad_worst_case_fee(*m, c, FeeScale::S10))
            })
            .max()
            .unwrap();
        // C2 (Q, P) with V_HI (HI1, LO) at s10
        assert_eq!(worst, 12_600 + 7_000 + 8_000 + 5_960);
    }
}

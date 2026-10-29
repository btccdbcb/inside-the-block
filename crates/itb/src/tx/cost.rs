//! Cost, fee, reward and priority of a probe, following Agave v4.3.2.
//!
//! * cost-model/src/block_cost_limits.rs: 720 CU per signature, 300 CU per write lock,
//!   1 CU per 4 bytes of instruction data, 8 CU per 32 KiB page of requested loaded data.
//! * The requested compute-unit limit L is charged in full.
//! * core/src/transaction_priority.rs: priority = reward * 1,000,000 / (cost + 1).
//! * reward = priority fee + the half of the signature fee that is not burned.
//!
//! These are predictions. The pilot compares them with meta.fee and meta.costUnits on chain.

use super::{Format, ProbeParams};

pub const SIGNATURE_COST: u64 = 720;
pub const WRITE_LOCK_COST: u64 = 300;
pub const INSTRUCTION_DATA_BYTES_PER_CU: u64 = 4;
pub const LOADED_DATA_PAGE_COST: u64 = 8;
pub const LOADED_DATA_PAGE_BYTES: u64 = 32 * 1024;
pub const LAMPORTS_PER_SIGNATURE: u64 = 5_000;
pub const BURN_PERCENT: u64 = 50;
pub const PRIORITY_MULTIPLIER: u64 = 1_000_000;

/// Every probe has one signature and two write-locked accounts (payer and lock).
pub const PROBE_SIGNATURES: u64 = 1;
pub const PROBE_WRITE_LOCKS: u64 = 2;

/// Instruction data bytes of the legacy probe: SetComputeUnitLimit (5) + SetComputeUnitPrice (9)
/// + SetLoadedAccountsDataSizeLimit (5) + System transfer (12).
pub const LEGACY_IX_DATA_BYTES: u64 = 31;
/// The v1 probe carries only the System transfer (12 bytes); limits sit in the config mask.
pub const V1_IX_DATA_BYTES: u64 = 12;

pub fn loaded_data_pages(bytes: u32) -> u64 {
    // SIMD-0385: a request of 0 is costed as one page.
    (bytes as u64).div_ceil(LOADED_DATA_PAGE_BYTES).max(1)
}

pub fn data_bytes_cost(bytes: u64) -> u64 {
    bytes / INSTRUCTION_DATA_BYTES_PER_CU
}

pub fn probe_cost(p: &ProbeParams) -> u64 {
    let data = match p.format {
        Format::Legacy => LEGACY_IX_DATA_BYTES,
        Format::V1 => V1_IX_DATA_BYTES,
    };
    SIGNATURE_COST * PROBE_SIGNATURES
        + WRITE_LOCK_COST * PROBE_WRITE_LOCKS
        + data_bytes_cost(data)
        + p.cu_limit as u64
        + LOADED_DATA_PAGE_COST * loaded_data_pages(p.loaded_data)
}

pub fn probe_fee(p: &ProbeParams) -> u64 {
    LAMPORTS_PER_SIGNATURE * PROBE_SIGNATURES + p.priority_fee
}

pub fn probe_reward(p: &ProbeParams) -> u64 {
    let base = LAMPORTS_PER_SIGNATURE * PROBE_SIGNATURES;
    p.priority_fee + base - base * BURN_PERCENT / 100
}

pub fn priority(reward: u64, cost: u64) -> u64 {
    reward.saturating_mul(PRIORITY_MULTIPLIER) / (cost + 1)
}

pub fn probe_priority(p: &ProbeParams) -> u64 {
    priority(probe_reward(p), probe_cost(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_from_the_design() {
        // (L, p, D, priority fee, fee, cost, priority) for A, B, A', HI, X, Z at fee scale s1.
        let rows = [
            (1_000, 100_000, 32 * 1024, 100, 5_100, 2_335, 1_113_013),
            (3_000, 50_000, 32 * 1024, 150, 5_150, 4_335, 611_162),
            (1_000, 100_000, 64 * 1024 * 1024, 100, 5_100, 18_711, 138_948),
            (1_000, 300_000, 32 * 1024, 300, 5_300, 2_335, 1_198_630),
            (1_000, 10_000, 32 * 1024, 10, 5_010, 2_335, 1_074_486),
            (1_200, 500_000, 32 * 1024, 600, 5_600, 2_535, 1_222_397),
        ];
        for (l, price, d, pf, fee, cost, prio) in rows {
            let p = ProbeParams::legacy(l, price, d);
            assert_eq!(p.priority_fee, pf);
            assert_eq!(probe_fee(&p), fee);
            assert_eq!(probe_cost(&p), cost);
            assert_eq!(probe_priority(&p), prio);
        }
    }

    #[test]
    fn matched_v1_has_same_cost_and_reward() {
        let legacy = ProbeParams::legacy(1_000, 100_000, 32 * 1024);
        let v1 = ProbeParams::v1(1_004, 100, 32 * 1024);
        assert_eq!(probe_cost(&legacy), probe_cost(&v1));
        assert_eq!(probe_reward(&legacy), probe_reward(&v1));
        assert_eq!(probe_fee(&legacy), probe_fee(&v1));
    }

    #[test]
    fn zero_loaded_data_is_one_page() {
        assert_eq!(loaded_data_pages(0), 1);
        assert_eq!(loaded_data_pages(1), 1);
        assert_eq!(loaded_data_pages(32 * 1024), 1);
        assert_eq!(loaded_data_pages(32 * 1024 + 1), 2);
        assert_eq!(loaded_data_pages(64 * 1024 * 1024), 2_048);
    }
}

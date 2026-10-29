//! Cost, fee, reward and priority of a probe, following Agave v4.3.2.
//!
//! * cost-model/src/block_cost_limits.rs: 720 CU per signature, 300 CU per write lock,
//!   1 CU per 4 bytes of instruction data, 8 CU per 32 KiB page of requested loaded data.
//! * The requested compute-unit limit L is charged in full. No probe requests a loaded-data limit, so the
//!   default 64 MiB (2,048 pages = 16,384 CU) is charged in full too.
//! * core/src/transaction_priority.rs: priority = reward * 1,000,000 / (cost + 1).
//! * reward = priority fee + the half of the signature fee that is not burned.
//!
//! These are predictions. The pilot compares them with meta.fee and meta.costUnits on chain.

use super::{Format, ProbeParams, Shape};

pub const SIGNATURE_COST: u64 = 720;
pub const WRITE_LOCK_COST: u64 = 300;
pub const INSTRUCTION_DATA_BYTES_PER_CU: u64 = 4;
pub const LOADED_DATA_PAGE_COST: u64 = 8;
pub const LOADED_DATA_PAGE_BYTES: u64 = 32 * 1024;
/// MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES, used when a transaction requests no limit.
pub const DEFAULT_LOADED_DATA_BYTES: u64 = 64 * 1024 * 1024;
pub const LAMPORTS_PER_SIGNATURE: u64 = 5_000;
pub const BURN_PERCENT: u64 = 50;
pub const PRIORITY_MULTIPLIER: u64 = 1_000_000;
/// Builtin cost of each instruction actually executed (ComputeBudget and System).
pub const BUILTIN_IX_CU: u64 = 150;

pub const PROBE_SIGNATURES: u64 = 1;

/// Instruction data bytes of the legacy probe: SetComputeUnitLimit (5) + SetComputeUnitPrice (9)
/// + System transfer (12).
pub const LEGACY_IX_DATA_BYTES: u64 = 26;
/// The v1 probe carries only the System transfer (12 bytes); the limits sit in the config mask.
pub const V1_IX_DATA_BYTES: u64 = 12;

pub fn loaded_data_pages(bytes: u64) -> u64 {
    // SIMD-0385: a request of 0 is costed as one page.
    bytes.div_ceil(LOADED_DATA_PAGE_BYTES).max(1)
}

pub fn data_bytes_cost(bytes: u64) -> u64 {
    bytes / INSTRUCTION_DATA_BYTES_PER_CU
}

fn ix_data_bytes(format: Format) -> u64 {
    match format {
        Format::Legacy => LEGACY_IX_DATA_BYTES,
        Format::V1 => V1_IX_DATA_BYTES,
    }
}

/// The cost the scheduler sees before execution (requested L and loaded-data limit).
pub fn probe_cost(p: &ProbeParams, shape: Shape) -> u64 {
    SIGNATURE_COST * PROBE_SIGNATURES
        + WRITE_LOCK_COST * shape.write_locks()
        + data_bytes_cost(ix_data_bytes(p.format))
        + p.cu_limit as u64
        + LOADED_DATA_PAGE_COST * loaded_data_pages(DEFAULT_LOADED_DATA_BYTES)
}

/// Expected block `meta.costUnits` once executed: CU actually consumed (150 per builtin instruction) and the
/// actually loaded accounts (well under one page).
pub fn probe_executed_cost(p: &ProbeParams, shape: Shape) -> u64 {
    let instructions = match p.format {
        Format::Legacy => 3,
        Format::V1 => 1,
    };
    SIGNATURE_COST * PROBE_SIGNATURES
        + WRITE_LOCK_COST * shape.write_locks()
        + data_bytes_cost(ix_data_bytes(p.format))
        + BUILTIN_IX_CU * instructions
        + LOADED_DATA_PAGE_COST
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

pub fn probe_priority(p: &ProbeParams, shape: Shape) -> u64 {
    priority(probe_reward(p), probe_cost(p, shape))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_from_the_design() {
        // (L, p, priority fee, fee, cost, priority) for A, B, HI, LO, P, Q, X, Z at fee scale s1 (PROBES.md).
        let rows = [
            (1_000, 100_000, 100, 5_100, 18_710, 138_955),
            (6_000, 25_000, 150, 5_150, 23_710, 111_762),
            (1_000, 300_000, 300, 5_300, 18_710, 149_644),
            (1_200, 80_000, 96, 5_096, 18_910, 137_274),
            (1_000, 200_000, 200, 5_200, 18_710, 144_300),
            (4_000, 190_000, 760, 5_760, 21_710, 150_154),
            (1_000, 10_000, 10, 5_010, 18_710, 134_145),
            (1_200, 500_000, 600, 5_600, 18_910, 163_925),
        ];
        for (l, price, pf, fee, cost, prio) in rows {
            let p = ProbeParams::legacy(l, price);
            assert_eq!(p.priority_fee, pf);
            assert_eq!(probe_fee(&p), fee);
            assert_eq!(probe_cost(&p, Shape::Partner), cost);
            assert_eq!(probe_priority(&p, Shape::Partner), prio);
        }
        assert_eq!(probe_cost(&ProbeParams::legacy(1_000, 1), Shape::SelfTransfer), 17_410 + 1_000);
    }

    #[test]
    fn matched_v1_has_same_cost_and_reward() {
        let legacy = ProbeParams::legacy(1_000, 100_000);
        let v1 = ProbeParams::v1(1_003, 100);
        assert_eq!(probe_cost(&legacy, Shape::Partner), probe_cost(&v1, Shape::Partner));
        assert_eq!(probe_reward(&legacy), probe_reward(&v1));
        assert_eq!(probe_fee(&legacy), probe_fee(&v1));
    }

    #[test]
    fn executed_cost() {
        let p = ProbeParams::legacy(1_000, 100_000);
        assert_eq!(probe_executed_cost(&p, Shape::Partner), 1_784);
        assert_eq!(probe_executed_cost(&p, Shape::SelfTransfer), 1_484);
        assert_eq!(probe_executed_cost(&ProbeParams::v1(1_003, 100), Shape::Partner), 1_481);
    }

    #[test]
    fn loaded_data_pages_rounding() {
        assert_eq!(loaded_data_pages(0), 1);
        assert_eq!(loaded_data_pages(1), 1);
        assert_eq!(loaded_data_pages(32 * 1024), 1);
        assert_eq!(loaded_data_pages(32 * 1024 + 1), 2);
        assert_eq!(loaded_data_pages(DEFAULT_LOADED_DATA_BYTES), 2_048);
    }
}

//! The live gate. A `LiveAuthorization` is the only thing that lets bytes of a transaction reach a socket
//! (QUIC `send` and RPC `send_transaction` both take `&LiveAuthorization`). It can only be built by
//! `LiveAuthorization::grant`, which checks every condition below. Dry runs never hold one.

use anyhow::{bail, Result};

/// Hard cap on fees over every live command ever run with these keys. `--max-spend-sol` can only lower it.
pub const MAX_SPEND_LAMPORTS: u64 = 30_000_000;
pub const MAINNET_GENESIS: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
/// First slot after the Alpenglow switch-over without vote transactions in blocks.
pub const ALPENGLOW_FIRST_SLOT: u64 = 459_653_008;
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Sub-budgets (lamports of fees). They sum to the compiled cap.
pub const SUB_BUDGETS: &[(&str, u64)] =
    &[("setup", 300_000), ("smoke", 100_000), ("pilot", 1_200_000), ("campaign", 28_400_000)];

pub fn sub_budget_cap(name: &str) -> Result<u64> {
    SUB_BUDGETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
        .ok_or_else(|| anyhow::anyhow!("unknown sub-budget {name}"))
}

pub struct LiveChecks<'a> {
    pub live_flag: bool,
    pub allow_non_mainnet: bool,
    pub max_spend_lamports: u64,
    pub genesis_hash: &'a str,
    pub current_slot: u64,
    pub sub_budget: &'a str,
    pub key_present: bool,
    pub reconciliation_clean: bool,
    /// Lamports already charged in the ledger (all live commands so far).
    pub charged_total: u64,
    pub charged_sub_budget: u64,
    /// Worst case of what this command plans to send.
    pub worst_case: u64,
}

#[derive(Debug)]
pub struct LiveAuthorization {
    sub_budget: String,
    limit_lamports: u64,
    mainnet: bool,
    _private: (),
}

impl LiveAuthorization {
    pub fn grant(c: &LiveChecks) -> Result<LiveAuthorization> {
        if !c.live_flag {
            bail!("dry run: no --live flag");
        }
        if c.max_spend_lamports > MAX_SPEND_LAMPORTS {
            bail!("--max-spend-sol above the compiled cap of {MAX_SPEND_LAMPORTS} lamports");
        }
        let mainnet = c.genesis_hash == MAINNET_GENESIS;
        if !mainnet && !c.allow_non_mainnet {
            bail!("genesis {} is not mainnet (use --allow-non-mainnet for localnet tests only)", c.genesis_hash);
        }
        if mainnet && c.current_slot < ALPENGLOW_FIRST_SLOT {
            bail!("slot {} is before the Alpenglow gate {ALPENGLOW_FIRST_SLOT}", c.current_slot);
        }
        if !c.key_present {
            bail!("no signing key");
        }
        if !c.reconciliation_clean {
            bail!("ledger and chain disagree: reconcile first");
        }
        let sub_cap = sub_budget_cap(c.sub_budget)?;
        let limit = c.max_spend_lamports.min(MAX_SPEND_LAMPORTS);
        if c.charged_total + c.worst_case > limit {
            bail!("cap: charged {} + worst case {} > limit {limit}", c.charged_total, c.worst_case);
        }
        if c.charged_sub_budget + c.worst_case > sub_cap {
            bail!(
                "sub-budget {}: charged {} + worst case {} > {sub_cap}",
                c.sub_budget,
                c.charged_sub_budget,
                c.worst_case
            );
        }
        Ok(LiveAuthorization { sub_budget: c.sub_budget.to_string(), limit_lamports: limit, mainnet, _private: () })
    }

    pub fn sub_budget(&self) -> &str {
        &self.sub_budget
    }

    pub fn limit(&self) -> u64 {
        self.limit_lamports
    }

    pub fn is_mainnet(&self) -> bool {
        self.mainnet
    }
}

pub fn sol_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SOL as f64).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> LiveChecks<'static> {
        LiveChecks {
            live_flag: true,
            allow_non_mainnet: false,
            max_spend_lamports: MAX_SPEND_LAMPORTS,
            genesis_hash: MAINNET_GENESIS,
            current_slot: ALPENGLOW_FIRST_SLOT + 1,
            sub_budget: "pilot",
            key_present: true,
            reconciliation_clean: true,
            charged_total: 0,
            charged_sub_budget: 0,
            worst_case: 100_000,
        }
    }

    #[test]
    fn grants_only_when_every_check_passes() {
        assert!(LiveAuthorization::grant(&ok()).is_ok());
        let mut c = ok();
        c.live_flag = false;
        assert!(LiveAuthorization::grant(&c).is_err(), "dry run must never get an authorization");
        let mut c = ok();
        c.max_spend_lamports = MAX_SPEND_LAMPORTS + 1;
        assert!(LiveAuthorization::grant(&c).is_err());
        let mut c = ok();
        c.genesis_hash = "localnet";
        assert!(LiveAuthorization::grant(&c).is_err());
        c.allow_non_mainnet = true;
        assert!(LiveAuthorization::grant(&c).is_ok());
        let mut c = ok();
        c.current_slot = ALPENGLOW_FIRST_SLOT - 1;
        assert!(LiveAuthorization::grant(&c).is_err());
        let mut c = ok();
        c.reconciliation_clean = false;
        assert!(LiveAuthorization::grant(&c).is_err());
        let mut c = ok();
        c.charged_total = MAX_SPEND_LAMPORTS - 50_000;
        assert!(LiveAuthorization::grant(&c).is_err());
        let mut c = ok();
        c.charged_sub_budget = 1_150_000;
        assert!(LiveAuthorization::grant(&c).is_err());
    }

    #[test]
    fn sub_budgets_sum_to_cap() {
        assert_eq!(SUB_BUDGETS.iter().map(|(_, c)| c).sum::<u64>(), MAX_SPEND_LAMPORTS);
    }
}

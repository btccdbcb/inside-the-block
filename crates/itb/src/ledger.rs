//! The spend ledger (SQLite, WAL, synchronous=FULL), next to the keys' state.
//!
//! Rules:
//! * Every transaction's fee is reserved *before* its bytes can leave the process (`reserve*`).
//! * A reservation is released (status `never_sent`) only when the bytes provably never left.
//! * `sent` stays charged until reconciliation proves it landed (charged at its exact fee) or expired
//!   (blockhash too old and unknown to the chain: released).
//! * Charged total = sum of fees with status reserved | sent | landed | unexplained.
//! * One process at a time: an exclusive OS lock on `ledger.lock`.

use crate::auth::{sub_budget_cap, MAX_SPEND_LAMPORTS};
use crate::util::utc_now;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS txs (
  id INTEGER PRIMARY KEY,
  created_utc TEXT NOT NULL,
  updated_utc TEXT NOT NULL,
  kind TEXT NOT NULL,
  sub_budget TEXT NOT NULL,
  run_id TEXT NOT NULL,
  signature TEXT NOT NULL UNIQUE,
  fee_payer TEXT NOT NULL,
  blockhash TEXT NOT NULL,
  last_valid_height INTEGER NOT NULL,
  fee INTEGER NOT NULL,
  transfers TEXT NOT NULL,
  status TEXT NOT NULL,
  slot INTEGER,
  err TEXT
);
CREATE INDEX IF NOT EXISTS txs_status ON txs(status);
CREATE TABLE IF NOT EXISTS accounts (pubkey TEXT PRIMARY KEY, role TEXT NOT NULL, baseline INTEGER NOT NULL,
  baseline_slot INTEGER NOT NULL, added_utc TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS foreign_deposits (id INTEGER PRIMARY KEY, utc TEXT NOT NULL, pubkey TEXT NOT NULL,
  lamports INTEGER NOT NULL, slot INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS reconciliations (id INTEGER PRIMARY KEY, utc TEXT NOT NULL, slot INTEGER NOT NULL,
  clean INTEGER NOT NULL, detail TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY, utc TEXT NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL);
";

pub const CHARGED: &str = "status IN ('reserved','sent','landed','unexplained')";

#[derive(Clone, Debug, Serialize)]
pub struct Reservation {
    pub kind: String,
    pub sub_budget: String,
    pub run_id: String,
    pub signature: String,
    pub fee_payer: String,
    pub blockhash: String,
    pub last_valid_height: u64,
    pub fee: u64,
    /// (from, to, lamports) moved between accounts if the transaction lands.
    pub transfers: Vec<(String, String, u64)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TxRow {
    pub id: i64,
    pub kind: String,
    pub sub_budget: String,
    pub run_id: String,
    pub signature: String,
    pub fee_payer: String,
    pub last_valid_height: u64,
    pub fee: u64,
    pub status: String,
    pub slot: Option<u64>,
    pub created_utc: String,
}

pub struct Ledger {
    conn: Connection,
    _lock: Option<File>,
    pub path: PathBuf,
}

impl Ledger {
    /// Open for writing: takes the exclusive process lock.
    pub fn open(state_dir: &Path) -> Result<Ledger> {
        std::fs::create_dir_all(state_dir)?;
        let lock = File::create(state_dir.join("ledger.lock"))?;
        if lock.try_lock().is_err() {
            bail!("another itb process holds {}/ledger.lock", state_dir.display());
        }
        let mut l = Self::open_inner(state_dir)?;
        l._lock = Some(lock);
        Ok(l)
    }

    /// Open without the process lock, for read-only reports.
    pub fn open_readonly(state_dir: &Path) -> Result<Ledger> {
        let path = state_dir.join("ledger.sqlite");
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open {}", path.display()))?;
        Ok(Ledger { conn, _lock: None, path })
    }

    fn open_inner(state_dir: &Path) -> Result<Ledger> {
        let path = state_dir.join("ledger.sqlite");
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(SCHEMA)?;
        let l = Ledger { conn, _lock: None, path };
        if l.meta("cap_lamports")?.is_none() {
            l.set_meta("cap_lamports", &MAX_SPEND_LAMPORTS.to_string())?;
            l.set_meta("created_utc", &utc_now())?;
        }
        Ok(l)
    }

    pub fn meta(&self, k: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT v FROM meta WHERE k=?1", [k], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&self, k: &str, v: &str) -> Result<()> {
        self.conn.execute("INSERT INTO meta(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [k, v])?;
        Ok(())
    }

    pub fn charged_total(&self) -> Result<u64> {
        let v: i64 =
            self.conn.query_row(&format!("SELECT COALESCE(SUM(fee),0) FROM txs WHERE {CHARGED}"), [], |r| r.get(0))?;
        Ok(v as u64)
    }

    pub fn charged_sub(&self, sub: &str) -> Result<u64> {
        let v: i64 = self.conn.query_row(
            &format!("SELECT COALESCE(SUM(fee),0) FROM txs WHERE {CHARGED} AND sub_budget=?1"),
            [sub],
            |r| r.get(0),
        )?;
        Ok(v as u64)
    }

    pub fn landed_total(&self) -> Result<u64> {
        let v: i64 =
            self.conn.query_row("SELECT COALESCE(SUM(fee),0) FROM txs WHERE status='landed'", [], |r| r.get(0))?;
        Ok(v as u64)
    }

    /// Reserve several transactions at once (all or nothing), checking the wallet rule, the cap, the run limit
    /// and the sub-budget.
    pub fn reserve_many(&mut self, rs: &[Reservation], limit: u64) -> Result<()> {
        self.check_wallet_rule(rs)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let total: i64 =
            tx.query_row(&format!("SELECT COALESCE(SUM(fee),0) FROM txs WHERE {CHARGED}"), [], |r| r.get(0))?;
        let add: u64 = rs.iter().map(|r| r.fee).sum();
        let limit = limit.min(MAX_SPEND_LAMPORTS);
        if total as u64 + add > limit {
            bail!("cap reached: charged {total} + {add} > {limit}");
        }
        let mut per_sub: HashMap<&str, u64> = HashMap::new();
        for r in rs {
            *per_sub.entry(r.sub_budget.as_str()).or_default() += r.fee;
        }
        for (sub, add) in per_sub {
            let used: i64 = tx.query_row(
                &format!("SELECT COALESCE(SUM(fee),0) FROM txs WHERE {CHARGED} AND sub_budget=?1"),
                [sub],
                |r| r.get(0),
            )?;
            let cap = sub_budget_cap(sub)?;
            if used as u64 + add > cap {
                bail!("sub-budget {sub} reached: {used} + {add} > {cap}");
            }
        }
        let now = utc_now();
        for r in rs {
            tx.execute(
                "INSERT INTO txs(created_utc,updated_utc,kind,sub_budget,run_id,signature,fee_payer,blockhash,last_valid_height,fee,transfers,status)
                 VALUES(?1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'reserved')",
                params![now, r.kind, r.sub_budget, r.run_id, r.signature, r.fee_payer, r.blockhash,
                        r.last_valid_height as i64, r.fee as i64, serde_json::to_string(&r.transfers)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Wallet rule (D-023): SOL only moves out of our own accounts (funder, payers) and only into our own
    /// accounts or the sweep address recorded in meta `sweep_to`; fees are paid by our own accounts.
    fn check_wallet_rule(&self, rs: &[Reservation]) -> Result<()> {
        let owned: std::collections::HashSet<String> = self.accounts()?.into_iter().map(|(pk, _, _)| pk).collect();
        let sweep_to = self.meta("sweep_to")?;
        for r in rs {
            if !owned.contains(&r.fee_payer) {
                bail!("wallet rule: fee payer {} of {} is not one of our accounts", r.fee_payer, r.signature);
            }
            for (from, to, _) in &r.transfers {
                if !owned.contains(from) {
                    bail!("wallet rule: transfer from {from} in {}: not one of our accounts", r.signature);
                }
                if !owned.contains(to) && sweep_to.as_deref() != Some(to.as_str()) {
                    bail!("wallet rule: transfer to {to} in {}: only our accounts or the sweep address", r.signature);
                }
                if sweep_to.as_deref() == Some(to.as_str()) && r.kind != "sweep" {
                    bail!("wallet rule: only the sweep may pay the sweep address ({})", r.signature);
                }
            }
        }
        Ok(())
    }

    pub fn set_status(&self, sig: &str, status: &str) -> Result<()> {
        let n = self
            .conn
            .execute("UPDATE txs SET status=?1, updated_utc=?2 WHERE signature=?3", params![status, utc_now(), sig])?;
        if n != 1 {
            bail!("ledger: no row for {sig}");
        }
        Ok(())
    }

    /// Release a reservation whose bytes never left the process.
    pub fn release_never_sent(&self, sig: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE txs SET status='never_sent', updated_utc=?1 WHERE signature=?2 AND status='reserved'",
            params![utc_now(), sig],
        )?;
        if n != 1 {
            bail!("ledger: {sig} is not a fresh reservation");
        }
        Ok(())
    }

    pub fn set_landed(&self, sig: &str, slot: u64, err: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE txs SET status='landed', slot=?1, err=?2, updated_utc=?3 WHERE signature=?4",
            params![slot as i64, err, utc_now(), sig],
        )?;
        Ok(())
    }

    pub fn set_expired(&self, sig: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE txs SET status='expired', updated_utc=?1 WHERE signature=?2 AND status IN ('reserved','sent')",
            params![utc_now(), sig],
        )?;
        Ok(())
    }

    fn rows(&self, where_: &str) -> Result<Vec<TxRow>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT id,kind,sub_budget,run_id,signature,fee_payer,last_valid_height,fee,status,slot,created_utc FROM txs WHERE {where_} ORDER BY id"))?;
        let rows = st.query_map([], |r| {
            Ok(TxRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                sub_budget: r.get(2)?,
                run_id: r.get(3)?,
                signature: r.get(4)?,
                fee_payer: r.get(5)?,
                last_valid_height: r.get::<_, i64>(6)? as u64,
                fee: r.get::<_, i64>(7)? as u64,
                status: r.get(8)?,
                slot: r.get::<_, Option<i64>>(9)?.map(|s| s as u64),
                created_utc: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn pending(&self) -> Result<Vec<TxRow>> {
        self.rows("status IN ('reserved','sent')")
    }

    pub fn all_rows(&self) -> Result<Vec<TxRow>> {
        self.rows("1=1")
    }

    /// Quad ids (the part of `run_id` after the last '/') whose probes were reserved and not released.
    pub fn attempted_quads(&self) -> Result<std::collections::HashSet<String>> {
        let mut st =
            self.conn.prepare("SELECT DISTINCT run_id FROM txs WHERE kind='probe' AND status != 'never_sent'")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = std::collections::HashSet::new();
        for r in rows {
            let r = r?;
            out.insert(r.rsplit('/').next().unwrap_or(&r).to_string());
        }
        Ok(out)
    }

    pub fn add_account(&self, pubkey: &str, role: &str, baseline: u64, slot: u64) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO accounts(pubkey,role,baseline,baseline_slot,added_utc) VALUES(?1,?2,?3,?4,?5)",
            params![pubkey, role, baseline as i64, slot as i64, utc_now()],
        )?;
        Ok(n == 1)
    }

    /// (pubkey, role, baseline)
    pub fn accounts(&self) -> Result<Vec<(String, String, u64)>> {
        let mut st = self.conn.prepare("SELECT pubkey, role, baseline FROM accounts ORDER BY rowid")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Balance each owned account should have if the ledger is right: baseline + transfers in − transfers out
    /// − fees paid, over landed transactions, + recorded foreign deposits.
    pub fn expected_balances(&self) -> Result<HashMap<String, i128>> {
        let mut exp: HashMap<String, i128> = HashMap::new();
        for (pk, _, base) in self.accounts()? {
            exp.insert(pk, base as i128);
        }
        let mut st = self
            .conn
            .prepare("SELECT fee_payer, fee, transfers, err FROM txs WHERE status IN ('landed','unexplained')")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?))
        })?;
        for row in rows {
            let (payer, fee, transfers, err) = row?;
            if let Some(b) = exp.get_mut(&payer) {
                *b -= fee as i128;
            }
            // a transaction that landed with an error pays its fee but moves nothing
            if err.is_some() {
                continue;
            }
            let ts: Vec<(String, String, u64)> = serde_json::from_str(&transfers)?;
            for (from, to, lamports) in ts {
                if let Some(b) = exp.get_mut(&from) {
                    *b -= lamports as i128;
                }
                if let Some(b) = exp.get_mut(&to) {
                    *b += lamports as i128;
                }
            }
        }
        let mut st = self.conn.prepare("SELECT pubkey, SUM(lamports) FROM foreign_deposits GROUP BY pubkey")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (pk, l) = row?;
            if let Some(b) = exp.get_mut(&pk) {
                *b += l as i128;
            }
        }
        Ok(exp)
    }

    pub fn record_foreign(&self, pubkey: &str, lamports: u64, slot: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO foreign_deposits(utc,pubkey,lamports,slot) VALUES(?1,?2,?3,?4)",
            params![utc_now(), pubkey, lamports as i64, slot as i64],
        )?;
        Ok(())
    }

    /// Charge an unexplained balance drop so the cap stays conservative.
    pub fn charge_unexplained(&self, pubkey: &str, lamports: u64, slot: u64) -> Result<()> {
        let now = utc_now();
        self.conn.execute(
            "INSERT INTO txs(created_utc,updated_utc,kind,sub_budget,run_id,signature,fee_payer,blockhash,last_valid_height,fee,transfers,status,slot)
             VALUES(?1,?1,'unexplained','setup','reconcile',?2,?3,'',0,?4,'[]','unexplained',?5)",
            params![now, format!("unexplained-{pubkey}-{slot}"), pubkey, lamports as i64, slot as i64])?;
        Ok(())
    }

    pub fn record_reconciliation(&self, slot: u64, clean: bool, detail: &serde_json::Value) -> Result<()> {
        self.conn.execute(
            "INSERT INTO reconciliations(utc,slot,clean,detail) VALUES(?1,?2,?3,?4)",
            params![utc_now(), slot as i64, clean as i64, detail.to_string()],
        )?;
        Ok(())
    }

    pub fn last_reconciliation_clean(&self) -> Result<bool> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT clean FROM reconciliations ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .optional()?;
        Ok(v.unwrap_or(1) == 1)
    }

    pub fn event(&self, kind: &str, detail: &serde_json::Value) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events(utc,kind,detail) VALUES(?1,?2,?3)",
            params![utc_now(), kind, detail.to_string()],
        )?;
        Ok(())
    }

    /// CSV export: one row per transaction ever reserved.
    pub fn export_csv(&self, path: &Path) -> Result<()> {
        let mut out =
            String::from("id,created_utc,kind,sub_budget,run_id,signature,fee_payer,fee_lamports,status,slot\n");
        for r in self.all_rows()? {
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{}\n",
                r.id,
                r.created_utc,
                r.kind,
                r.sub_budget,
                r.run_id,
                r.signature,
                r.fee_payer,
                r.fee,
                r.status,
                r.slot.map(|s| s.to_string()).unwrap_or_default()
            ));
        }
        std::fs::write(path, out)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("itb-ledger-{tag}-{}", crate::util::wall_ns()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn res(sig: &str, sub: &str, fee: u64) -> Reservation {
        Reservation {
            kind: "probe".into(),
            sub_budget: sub.into(),
            run_id: "t".into(),
            signature: sig.into(),
            fee_payer: "P".into(),
            blockhash: "h".into(),
            last_valid_height: 100,
            fee,
            transfers: vec![],
        }
    }

    #[test]
    fn reserve_charges_and_cap_holds() {
        let d = dir("cap");
        let mut l = Ledger::open(&d).unwrap();
        l.add_account("P", "payer-00", 0, 1).unwrap();
        l.reserve_many(&[res("a", "pilot", 600_000)], MAX_SPEND_LAMPORTS).unwrap();
        assert_eq!(l.charged_total().unwrap(), 600_000);
        // sub-budget pilot is 1,200,000
        assert!(l.reserve_many(&[res("b", "pilot", 700_000)], MAX_SPEND_LAMPORTS).is_err());
        // run limit lower than the cap
        assert!(l.reserve_many(&[res("c", "campaign", 500_000)], 1_000_000).is_err());
        // all-or-nothing: a failing batch leaves nothing behind
        assert_eq!(l.charged_total().unwrap(), 600_000);
        l.release_never_sent("a").unwrap();
        assert_eq!(l.charged_total().unwrap(), 0);
    }

    #[test]
    fn crash_between_reserve_and_send_stays_charged_after_restart() {
        let d = dir("crash");
        {
            let mut l = Ledger::open(&d).unwrap();
            l.add_account("P", "payer-00", 0, 1).unwrap();
            l.reserve_many(&[res("x", "campaign", 5_100), res("y", "campaign", 5_300)], MAX_SPEND_LAMPORTS).unwrap();
            l.set_status("x", "sent").unwrap();
            // process dies here: drop without any cleanup
        }
        let l = Ledger::open(&d).unwrap();
        assert_eq!(l.charged_total().unwrap(), 10_400);
        assert_eq!(l.pending().unwrap().len(), 2);
        // a sent transaction can't be released as never-sent
        assert!(l.release_never_sent("x").is_err());
        l.set_expired("x").unwrap();
        l.set_landed("y", 42, None).unwrap();
        assert_eq!(l.charged_total().unwrap(), 5_300);
        assert_eq!(l.landed_total().unwrap(), 5_300);
    }

    #[test]
    fn second_process_is_locked_out() {
        let d = dir("lock");
        let _a = Ledger::open(&d).unwrap();
        assert!(Ledger::open(&d).is_err());
    }

    #[test]
    fn expected_balances_follow_landed_transfers_and_fees() {
        let d = dir("bal");
        let mut l = Ledger::open(&d).unwrap();
        l.add_account("F", "funder", 1_000_000, 1).unwrap();
        l.add_account("P", "payer", 0, 1).unwrap();
        let mut fund = res("f1", "setup", 5_000);
        fund.kind = "fund".into();
        fund.fee_payer = "F".into();
        fund.transfers = vec![("F".into(), "P".into(), 100_000)];
        l.reserve_many(&[fund, res("p1", "campaign", 5_100)], MAX_SPEND_LAMPORTS).unwrap();
        l.set_landed("f1", 10, None).unwrap();
        l.set_landed("p1", 11, None).unwrap();
        let e = l.expected_balances().unwrap();
        assert_eq!(e["F"], 1_000_000 - 100_000 - 5_000);
        assert_eq!(e["P"], 100_000 - 5_100);
        l.record_foreign("P", 1, 12).unwrap();
        assert_eq!(l.expected_balances().unwrap()["P"], 100_000 - 5_100 + 1);
    }

    #[test]
    fn wallet_rule() {
        let d = dir("wallet");
        let mut l = Ledger::open(&d).unwrap();
        l.add_account("F", "funder", 1_000_000, 1).unwrap();
        l.add_account("P", "payer-00", 0, 1).unwrap();
        l.add_account("Q", "payer-01", 0, 1).unwrap();
        l.set_meta("sweep_to", "S").unwrap();
        let tr = |sig: &str, kind: &str, payer: &str, from: &str, to: &str| {
            let mut r = res(sig, "setup", 5_000);
            r.kind = kind.into();
            r.fee_payer = payer.into();
            r.transfers = vec![(from.into(), to.into(), 1)];
            r
        };
        // fund, partner transfer, self-transfer, sweep: allowed
        l.reserve_many(&[tr("a", "fund", "F", "F", "P")], MAX_SPEND_LAMPORTS).unwrap();
        l.reserve_many(&[tr("b", "probe", "P", "P", "Q")], MAX_SPEND_LAMPORTS).unwrap();
        l.reserve_many(&[tr("c", "probe", "Q", "Q", "Q")], MAX_SPEND_LAMPORTS).unwrap();
        l.reserve_many(&[tr("d", "sweep", "P", "P", "S")], MAX_SPEND_LAMPORTS).unwrap();
        // anything else: refused, and nothing is reserved
        let before = l.charged_total().unwrap();
        assert!(l.reserve_many(&[tr("e", "probe", "P", "P", "X")], MAX_SPEND_LAMPORTS).is_err());
        assert!(l.reserve_many(&[tr("f", "probe", "P", "X", "P")], MAX_SPEND_LAMPORTS).is_err());
        assert!(l.reserve_many(&[tr("g", "probe", "X", "P", "Q")], MAX_SPEND_LAMPORTS).is_err());
        assert!(l.reserve_many(&[tr("h", "probe", "P", "P", "S")], MAX_SPEND_LAMPORTS).is_err());
        assert!(l
            .reserve_many(&[tr("i", "probe", "P", "P", "Q"), tr("j", "fund", "F", "F", "Z")], MAX_SPEND_LAMPORTS)
            .is_err());
        assert_eq!(l.charged_total().unwrap(), before);
    }
}

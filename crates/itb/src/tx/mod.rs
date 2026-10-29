//! Transaction wire formats (legacy and v1), the probe parameter library and the cost/fee/priority model.
//!
//! Everything here is hand-serialised so the bytes on the wire are exactly what the tests and the
//! verifier see. No Solana SDK types are involved.

pub mod cost;
pub mod legacy;
pub mod library;
pub mod v1;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

pub type Pubkey = [u8; 32];

pub const SYSTEM_PROGRAM: Pubkey = [0u8; 32];
pub const COMPUTE_BUDGET_PROGRAM: &str = "ComputeBudget111111111111111111111111111111";
pub const MEMO_V2_PROGRAM: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

pub fn pubkey_from_b58(s: &str) -> Result<Pubkey> {
    let v = bs58::decode(s).into_vec().map_err(|e| anyhow!("bad base58 pubkey {s}: {e}"))?;
    v.try_into().map_err(|_| anyhow!("pubkey {s} is not 32 bytes"))
}

pub fn b58(bytes: &[u8]) -> String {
    bs58::encode(bytes).into_string()
}

/// Solana "compact-u16" (shortvec) length prefix.
pub fn put_compact_u16(buf: &mut Vec<u8>, mut v: u16) {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(byte);
            return;
        }
        byte |= 0x80;
        buf.push(byte);
    }
}

pub fn get_compact_u16(b: &[u8], pos: &mut usize) -> Result<u16> {
    let mut v: u32 = 0;
    for i in 0..3 {
        let byte = *b.get(*pos).ok_or_else(|| anyhow!("truncated compact-u16"))?;
        *pos += 1;
        v |= ((byte & 0x7f) as u32) << (7 * i);
        if byte & 0x80 == 0 {
            if v > u16::MAX as u32 {
                bail!("compact-u16 overflow");
            }
            return Ok(v as u16);
        }
    }
    bail!("compact-u16 too long")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Legacy,
    V1,
}

/// The numbers that differ between probes. Everything else about a probe is fixed. No probe requests a
/// loaded-accounts data limit, so the default (64 MiB) is costed for every probe (D-023).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeParams {
    pub format: Format,
    /// Requested compute-unit limit L.
    pub cu_limit: u32,
    /// Compute-unit price in micro-lamports per CU (legacy only, 0 for v1).
    pub cu_price_micro: u64,
    /// Priority fee in lamports: legacy = ceil(L * p / 1e6), v1 = the configured value.
    pub priority_fee: u64,
}

impl ProbeParams {
    pub fn legacy(cu_limit: u32, cu_price_micro: u64) -> Self {
        let priority_fee = (cu_limit as u64 * cu_price_micro).div_ceil(1_000_000);
        ProbeParams { format: Format::Legacy, cu_limit, cu_price_micro, priority_fee }
    }

    pub fn v1(cu_limit: u32, priority_fee: u64) -> Self {
        ProbeParams { format: Format::V1, cu_limit, cu_price_micro: 0, priority_fee }
    }
}

/// Lamports every probe moves: to the other member's payer, or to itself (R_IND).
pub const PROBE_TRANSFER_LAMPORTS: u64 = 1;

/// Where a probe's transfer goes. `Partner`: the other pair member's payer, so both members write-lock both
/// payers. `SelfTransfer`: the payer itself (R_IND), so only the own payer is write-locked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Partner,
    SelfTransfer,
}

impl Shape {
    pub fn write_locks(self) -> u64 {
        match self {
            Shape::Partner => 2,
            Shape::SelfTransfer => 1,
        }
    }
}

/// One instruction in "compiled" form: indices into the account list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledIx {
    pub program_index: u8,
    pub accounts: Vec<u8>,
    pub data: Vec<u8>,
}

/// A decoded transaction of either format, enough for the verifier and the tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub format: Format,
    pub header: [u8; 3],
    pub accounts: Vec<Pubkey>,
    pub blockhash: [u8; 32],
    pub instructions: Vec<CompiledIx>,
    pub signatures: Vec<[u8; 64]>,
    /// The bytes that were signed.
    pub message: Vec<u8>,
    /// v1 only: the config mask and the decoded values.
    pub v1_config: Option<v1::Config>,
}

impl Decoded {
    /// (from, to, lamports) of every System instruction, in order. Any System instruction other than a plain
    /// transfer is an error, so a caller can compare the bytes with what the ledger was told.
    pub fn system_transfers(&self) -> Result<Vec<(Pubkey, Pubkey, u64)>> {
        let acct =
            |i: u8| self.accounts.get(i as usize).copied().ok_or_else(|| anyhow!("account index {i} out of range"));
        let mut out = Vec::new();
        for ix in &self.instructions {
            if acct(ix.program_index)? != SYSTEM_PROGRAM {
                continue;
            }
            if ix.data.len() != 12 || ix.data[..4] != [2, 0, 0, 0] || ix.accounts.len() != 2 {
                bail!("System instruction other than a transfer");
            }
            let lamports = u64::from_le_bytes(ix.data[4..12].try_into().unwrap());
            out.push((acct(ix.accounts[0])?, acct(ix.accounts[1])?, lamports));
        }
        Ok(out)
    }

    /// Same as `system_transfers`, with base58 keys.
    pub fn system_transfers_b58(&self) -> Result<Vec<(String, String, u64)>> {
        Ok(self.system_transfers()?.into_iter().map(|(f, t, l)| (b58(&f), b58(&t), l)).collect())
    }
}

pub fn decode(bytes: &[u8]) -> Result<Decoded> {
    if bytes.first() == Some(&v1::VERSION_BYTE) {
        v1::decode(bytes)
    } else {
        legacy::decode(bytes)
    }
}

pub fn system_transfer_data(lamports: u64) -> Vec<u8> {
    let mut d = Vec::with_capacity(12);
    d.extend_from_slice(&2u32.to_le_bytes());
    d.extend_from_slice(&lamports.to_le_bytes());
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_u16_roundtrip() {
        for v in [0u16, 1, 127, 128, 255, 256, 16383, 16384, 65535] {
            let mut b = Vec::new();
            put_compact_u16(&mut b, v);
            let mut pos = 0;
            assert_eq!(get_compact_u16(&b, &mut pos).unwrap(), v);
            assert_eq!(pos, b.len());
        }
        let mut b = Vec::new();
        put_compact_u16(&mut b, 128);
        assert_eq!(b, vec![0x80, 0x01]);
    }

    #[test]
    fn known_program_ids() {
        assert_eq!(b58(&SYSTEM_PROGRAM), "11111111111111111111111111111111");
        assert_eq!(pubkey_from_b58(COMPUTE_BUDGET_PROGRAM).unwrap()[0], 3);
    }
}

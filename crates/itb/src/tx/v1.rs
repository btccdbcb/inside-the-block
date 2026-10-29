//! Transaction format v1 (SIMD-0385), hand-serialised.
//!
//! Layout: version byte 129 | header (3 x u8) | config mask (u32 LE) | lifetime (32) | num instructions (u8)
//! | num addresses (u8) | addresses | config values (4 bytes per mask bit) | instruction headers
//! (program index u8, num accounts u8, data length u16 LE) | instruction payloads | signatures (64 each).
//! Signatures sign every byte before the signature block.

use super::{
    system_transfer_data, CompiledIx, Decoded, Format, ProbeParams, Pubkey, PROBE_TRANSFER_LAMPORTS, SYSTEM_PROGRAM,
};
use anyhow::{bail, ensure, Result};
use ed25519_dalek::{Signer, SigningKey};

pub const VERSION_BYTE: u8 = 129;
pub const MASK_PRIORITY_FEE: u32 = 0b11;
pub const MASK_CU_LIMIT: u32 = 1 << 2;
pub const MASK_LOADED_DATA: u32 = 1 << 3;
pub const MASK_HEAP: u32 = 1 << 4;
/// Loaded-data limit written into every v1 probe: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES, the legacy default.
pub const V1_LOADED_DATA_BYTES: u32 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Config {
    pub mask: u32,
    pub priority_fee: Option<u64>,
    pub cu_limit: Option<u32>,
    pub loaded_data: Option<u32>,
    pub heap: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub header: [u8; 3],
    pub config: Config,
    pub blockhash: [u8; 32],
    pub accounts: Vec<Pubkey>,
    pub instructions: Vec<CompiledIx>,
}

impl Message {
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(256);
        b.push(VERSION_BYTE);
        b.extend_from_slice(&self.header);
        b.extend_from_slice(&self.config.mask.to_le_bytes());
        b.extend_from_slice(&self.blockhash);
        b.push(self.instructions.len() as u8);
        b.push(self.accounts.len() as u8);
        for a in &self.accounts {
            b.extend_from_slice(a);
        }
        let c = &self.config;
        if c.mask & MASK_PRIORITY_FEE == MASK_PRIORITY_FEE {
            b.extend_from_slice(&c.priority_fee.unwrap_or(0).to_le_bytes());
        }
        if c.mask & MASK_CU_LIMIT != 0 {
            b.extend_from_slice(&c.cu_limit.unwrap_or(0).to_le_bytes());
        }
        if c.mask & MASK_LOADED_DATA != 0 {
            b.extend_from_slice(&c.loaded_data.unwrap_or(0).to_le_bytes());
        }
        if c.mask & MASK_HEAP != 0 {
            b.extend_from_slice(&c.heap.unwrap_or(32 * 1024).to_le_bytes());
        }
        for ix in &self.instructions {
            b.push(ix.program_index);
            b.push(ix.accounts.len() as u8);
            b.extend_from_slice(&(ix.data.len() as u16).to_le_bytes());
        }
        for ix in &self.instructions {
            b.extend_from_slice(&ix.accounts);
            b.extend_from_slice(&ix.data);
        }
        b
    }

    pub fn sign(&self, signers: &[&SigningKey]) -> Result<Vec<u8>> {
        let n = self.header[0] as usize;
        ensure!(signers.len() == n, "need {n} signers, got {}", signers.len());
        for (i, s) in signers.iter().enumerate() {
            ensure!(s.verifying_key().to_bytes() == self.accounts[i], "signer {i} does not match account {i}");
        }
        let mut b = self.serialize();
        let sigs: Vec<[u8; 64]> = signers.iter().map(|s| s.sign(&b).to_bytes()).collect();
        for s in sigs {
            b.extend_from_slice(&s);
        }
        ensure!(b.len() <= 4096, "v1 transaction larger than 4096 bytes");
        Ok(b)
    }
}

/// The v1 probe: one System transfer of 1 lamport; priority fee, CU limit and loaded-data limit in the config mask.
/// The loaded-data limit is set to 64 MiB, the value a legacy transaction gets by default: a v1 transaction without it
/// is limited to 0 bytes on Agave 4.3.2 (fails with MaxLoadedAccountsDataSizeExceeded; D-025). `Some(partner)`:
/// accounts [payer (s, w), partner (w), System]; `None` (R_IND): [payer, System] with a self-transfer.
pub fn probe_message(
    payer: &Pubkey,
    partner: Option<&Pubkey>,
    p: &ProbeParams,
    blockhash: &[u8; 32],
) -> Result<Message> {
    ensure!(p.format == Format::V1, "v1 builder needs v1 params");
    ensure!(partner != Some(payer), "partner must differ from the payer");
    let (accounts, to) = match partner {
        Some(q) => (vec![*payer, *q, SYSTEM_PROGRAM], 1u8),
        None => (vec![*payer, SYSTEM_PROGRAM], 0u8),
    };
    let sys = (accounts.len() - 1) as u8;
    Ok(Message {
        header: [1, 0, 1],
        config: Config {
            mask: MASK_PRIORITY_FEE | MASK_CU_LIMIT | MASK_LOADED_DATA,
            priority_fee: Some(p.priority_fee),
            cu_limit: Some(p.cu_limit),
            loaded_data: Some(V1_LOADED_DATA_BYTES),
            heap: None,
        },
        blockhash: *blockhash,
        accounts,
        instructions: vec![CompiledIx {
            program_index: sys,
            accounts: vec![0, to],
            data: system_transfer_data(PROBE_TRANSFER_LAMPORTS),
        }],
    })
}

fn take<'a>(b: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8]> {
    ensure!(b.len() >= *pos + n, "truncated v1 transaction at byte {}", *pos);
    let s = &b[*pos..*pos + n];
    *pos += n;
    Ok(s)
}

pub fn decode(bytes: &[u8]) -> Result<Decoded> {
    let mut p = 0usize;
    ensure!(take(bytes, &mut p, 1)?[0] == VERSION_BYTE, "not a v1 transaction");
    let h = take(bytes, &mut p, 3)?;
    let header = [h[0], h[1], h[2]];
    ensure!(header[0] >= 1 && header[1] < header[0], "invalid v1 header");
    let mask = u32::from_le_bytes(take(bytes, &mut p, 4)?.try_into().unwrap());
    if mask & MASK_PRIORITY_FEE != 0 && mask & MASK_PRIORITY_FEE != MASK_PRIORITY_FEE {
        bail!("priority-fee mask needs both bits");
    }
    if mask & !0b1_1111 != 0 {
        bail!("unknown config mask bits {mask:#x}");
    }
    let blockhash: [u8; 32] = take(bytes, &mut p, 32)?.try_into().unwrap();
    let nix = take(bytes, &mut p, 1)?[0] as usize;
    let nacc = take(bytes, &mut p, 1)?[0] as usize;
    let mut accounts = Vec::with_capacity(nacc);
    for _ in 0..nacc {
        accounts.push(take(bytes, &mut p, 32)?.try_into().unwrap());
    }
    let mut config = Config { mask, ..Default::default() };
    if mask & MASK_PRIORITY_FEE == MASK_PRIORITY_FEE {
        config.priority_fee = Some(u64::from_le_bytes(take(bytes, &mut p, 8)?.try_into().unwrap()));
    }
    if mask & MASK_CU_LIMIT != 0 {
        config.cu_limit = Some(u32::from_le_bytes(take(bytes, &mut p, 4)?.try_into().unwrap()));
    }
    if mask & MASK_LOADED_DATA != 0 {
        config.loaded_data = Some(u32::from_le_bytes(take(bytes, &mut p, 4)?.try_into().unwrap()));
    }
    if mask & MASK_HEAP != 0 {
        config.heap = Some(u32::from_le_bytes(take(bytes, &mut p, 4)?.try_into().unwrap()));
    }
    let mut heads = Vec::with_capacity(nix);
    for _ in 0..nix {
        let x = take(bytes, &mut p, 4)?;
        heads.push((x[0], x[1] as usize, u16::from_le_bytes([x[2], x[3]]) as usize));
    }
    let mut instructions = Vec::with_capacity(nix);
    for (program_index, na, nd) in heads {
        let accts = take(bytes, &mut p, na)?.to_vec();
        let data = take(bytes, &mut p, nd)?.to_vec();
        instructions.push(CompiledIx { program_index, accounts: accts, data });
    }
    let message = bytes[..p].to_vec();
    let nsig = header[0] as usize;
    let mut signatures = Vec::with_capacity(nsig);
    for _ in 0..nsig {
        signatures.push(take(bytes, &mut p, 64)?.try_into().unwrap());
    }
    ensure!(p == bytes.len(), "trailing bytes after v1 signatures");
    Ok(Decoded {
        format: Format::V1,
        header,
        accounts,
        blockhash,
        instructions,
        signatures,
        message,
        v1_config: Some(config),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::decode as decode_any;
    use ed25519_dalek::{Signature, Verifier};

    #[test]
    fn probe_roundtrip() {
        let payer = SigningKey::from_bytes(&[8u8; 32]);
        let p = ProbeParams::v1(1_003, 100);
        let m = probe_message(&payer.verifying_key().to_bytes(), Some(&[6u8; 32]), &p, &[1u8; 32]).unwrap();
        let wire = m.sign(&[&payer]).unwrap();
        // 1 + 3 + 4 + 32 + 1 + 1 + 3*32 + (8+4+4) + 4 + (2+12) + 64
        assert_eq!(wire.len(), 1 + 3 + 4 + 32 + 1 + 1 + 96 + 16 + 4 + 14 + 64);
        let d = decode_any(&wire).unwrap();
        assert_eq!(d.format, Format::V1);
        let c = d.v1_config.clone().unwrap();
        assert_eq!((c.priority_fee, c.cu_limit, c.loaded_data), (Some(100), Some(1_003), Some(64 * 1024 * 1024)));
        assert_eq!(c.mask, 0b1111);
        let sig = Signature::from_bytes(&d.signatures[0]);
        payer.verifying_key().verify(&d.message, &sig).unwrap();
        assert_eq!(d.instructions[0].data.len() as u64, crate::tx::cost::V1_IX_DATA_BYTES);
        assert_eq!(d.instructions[0].data, crate::tx::system_transfer_data(1));
        let s = decode_any(&probe_message(&d.accounts[0], None, &p, &[1u8; 32]).unwrap().sign(&[&payer]).unwrap());
        assert_eq!(s.unwrap().instructions[0].accounts, vec![0, 0]);
    }

    #[test]
    fn rejects_half_priority_mask_and_trailing_bytes() {
        let payer = SigningKey::from_bytes(&[8u8; 32]);
        let p = ProbeParams::v1(1_003, 100);
        let m = probe_message(&payer.verifying_key().to_bytes(), Some(&[6u8; 32]), &p, &[1u8; 32]).unwrap();
        let mut wire = m.sign(&[&payer]).unwrap();
        wire.push(0);
        assert!(decode(&wire).is_err());
        let mut bad = m.clone();
        bad.config.mask = 0b1101;
        let w = bad.sign(&[&payer]).unwrap();
        assert!(decode(&w).is_err());
    }
}

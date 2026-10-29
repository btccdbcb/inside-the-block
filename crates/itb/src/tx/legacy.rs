//! Legacy transaction format: probes, funding transfers, the Memo and the sweep.

use super::{
    get_compact_u16, pubkey_from_b58, put_compact_u16, system_transfer_data, CompiledIx, Decoded, Format, ProbeParams,
    Pubkey, COMPUTE_BUDGET_PROGRAM, MEMO_V2_PROGRAM, PROBE_TRANSFER_LAMPORTS, SYSTEM_PROGRAM,
};
use anyhow::{bail, ensure, Result};
use ed25519_dalek::{Signer, SigningKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// (num_required_signatures, num_readonly_signed, num_readonly_unsigned)
    pub header: [u8; 3],
    pub accounts: Vec<Pubkey>,
    pub blockhash: [u8; 32],
    pub instructions: Vec<CompiledIx>,
}

impl Message {
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(256);
        b.extend_from_slice(&self.header);
        put_compact_u16(&mut b, self.accounts.len() as u16);
        for a in &self.accounts {
            b.extend_from_slice(a);
        }
        b.extend_from_slice(&self.blockhash);
        put_compact_u16(&mut b, self.instructions.len() as u16);
        for ix in &self.instructions {
            b.push(ix.program_index);
            put_compact_u16(&mut b, ix.accounts.len() as u16);
            b.extend_from_slice(&ix.accounts);
            put_compact_u16(&mut b, ix.data.len() as u16);
            b.extend_from_slice(&ix.data);
        }
        b
    }

    /// Sign with the required signers in account order and return the wire bytes.
    pub fn sign(&self, signers: &[&SigningKey]) -> Result<Vec<u8>> {
        let n = self.header[0] as usize;
        ensure!(signers.len() == n, "need {n} signers, got {}", signers.len());
        for (i, s) in signers.iter().enumerate() {
            ensure!(s.verifying_key().to_bytes() == self.accounts[i], "signer {i} does not match account {i}");
        }
        let msg = self.serialize();
        let mut b = Vec::with_capacity(1 + 64 * n + msg.len());
        put_compact_u16(&mut b, n as u16);
        for s in signers {
            b.extend_from_slice(&s.sign(&msg).to_bytes());
        }
        b.extend_from_slice(&msg);
        Ok(b)
    }
}

pub fn compute_budget_data_limit(cu_limit: u32) -> Vec<u8> {
    let mut d = vec![2u8];
    d.extend_from_slice(&cu_limit.to_le_bytes());
    d
}

pub fn compute_budget_data_price(micro_lamports: u64) -> Vec<u8> {
    let mut d = vec![3u8];
    d.extend_from_slice(&micro_lamports.to_le_bytes());
    d
}

/// The legacy probe: SetComputeUnitLimit(L), SetComputeUnitPrice(p), System transfer of 1 lamport.
/// `Some(partner)`: payer -> partner (the other member's payer); accounts [payer (s, w), partner (w), System,
/// ComputeBudget]. `None` (R_IND): payer -> payer; accounts [payer (s, w), System, ComputeBudget].
pub fn probe_message(
    payer: &Pubkey,
    partner: Option<&Pubkey>,
    p: &ProbeParams,
    blockhash: &[u8; 32],
) -> Result<Message> {
    ensure!(p.format == Format::Legacy, "legacy builder needs legacy params");
    ensure!(partner != Some(payer), "partner must differ from the payer");
    let cb = pubkey_from_b58(COMPUTE_BUDGET_PROGRAM)?;
    let (accounts, to) = match partner {
        Some(q) => (vec![*payer, *q, SYSTEM_PROGRAM, cb], 1u8),
        None => (vec![*payer, SYSTEM_PROGRAM, cb], 0u8),
    };
    let (sys, cbi) = ((accounts.len() - 2) as u8, (accounts.len() - 1) as u8);
    Ok(Message {
        header: [1, 0, 2],
        accounts,
        blockhash: *blockhash,
        instructions: vec![
            CompiledIx { program_index: cbi, accounts: vec![], data: compute_budget_data_limit(p.cu_limit) },
            CompiledIx { program_index: cbi, accounts: vec![], data: compute_budget_data_price(p.cu_price_micro) },
            CompiledIx {
                program_index: sys,
                accounts: vec![0, to],
                data: system_transfer_data(PROBE_TRANSFER_LAMPORTS),
            },
        ],
    })
}

/// One funding transaction: the funder sends `amount` to each payer.
pub fn fund_message(funder: &Pubkey, transfers: &[(Pubkey, u64)], blockhash: &[u8; 32]) -> Message {
    let mut accounts = vec![*funder];
    accounts.extend(transfers.iter().map(|(pk, _)| *pk));
    accounts.push(SYSTEM_PROGRAM);
    let system_index = (accounts.len() - 1) as u8;
    let instructions = transfers
        .iter()
        .enumerate()
        .map(|(i, (_, amount))| CompiledIx {
            program_index: system_index,
            accounts: vec![0, (i + 1) as u8],
            data: system_transfer_data(*amount),
        })
        .collect();
    Message { header: [1, 0, 1], accounts, blockhash: *blockhash, instructions }
}

/// A plain transfer (used by the sweep).
pub fn transfer_message(from: &Pubkey, to: &Pubkey, lamports: u64, blockhash: &[u8; 32]) -> Message {
    Message {
        header: [1, 0, 1],
        accounts: vec![*from, *to, SYSTEM_PROGRAM],
        blockhash: *blockhash,
        instructions: vec![CompiledIx { program_index: 2, accounts: vec![0, 1], data: system_transfer_data(lamports) }],
    }
}

/// Memo v2 with the given UTF-8 text and no signer accounts.
pub fn memo_message(payer: &Pubkey, text: &str, blockhash: &[u8; 32]) -> Result<Message> {
    let memo = pubkey_from_b58(MEMO_V2_PROGRAM)?;
    ensure!(text.len() <= 566, "memo text too long for one transaction");
    Ok(Message {
        header: [1, 0, 1],
        accounts: vec![*payer, memo],
        blockhash: *blockhash,
        instructions: vec![CompiledIx { program_index: 1, accounts: vec![], data: text.as_bytes().to_vec() }],
    })
}

pub fn decode(bytes: &[u8]) -> Result<Decoded> {
    let mut pos = 0usize;
    let nsig = get_compact_u16(bytes, &mut pos)? as usize;
    let mut signatures = Vec::with_capacity(nsig);
    for _ in 0..nsig {
        ensure!(bytes.len() >= pos + 64, "truncated signatures");
        signatures.push(bytes[pos..pos + 64].try_into().unwrap());
        pos += 64;
    }
    let message = bytes[pos..].to_vec();
    let m = &message;
    let mut p = 0usize;
    ensure!(m.len() >= 3, "truncated header");
    if m[0] & 0x80 != 0 {
        bail!("versioned (v0) message, not legacy");
    }
    let header = [m[0], m[1], m[2]];
    p += 3;
    let nacc = get_compact_u16(m, &mut p)? as usize;
    let mut accounts = Vec::with_capacity(nacc);
    for _ in 0..nacc {
        ensure!(m.len() >= p + 32, "truncated accounts");
        accounts.push(m[p..p + 32].try_into().unwrap());
        p += 32;
    }
    ensure!(m.len() >= p + 32, "truncated blockhash");
    let blockhash: [u8; 32] = m[p..p + 32].try_into().unwrap();
    p += 32;
    let nix = get_compact_u16(m, &mut p)? as usize;
    let mut instructions = Vec::with_capacity(nix);
    for _ in 0..nix {
        ensure!(m.len() > p, "truncated instruction");
        let program_index = m[p];
        p += 1;
        let na = get_compact_u16(m, &mut p)? as usize;
        ensure!(m.len() >= p + na, "truncated instruction accounts");
        let accts = m[p..p + na].to_vec();
        p += na;
        let nd = get_compact_u16(m, &mut p)? as usize;
        ensure!(m.len() >= p + nd, "truncated instruction data");
        let data = m[p..p + nd].to_vec();
        p += nd;
        instructions.push(CompiledIx { program_index, accounts: accts, data });
    }
    ensure!(p == m.len(), "trailing bytes after legacy message");
    ensure!(signatures.len() == header[0] as usize, "signature count does not match header");
    Ok(Decoded {
        format: Format::Legacy,
        header,
        accounts,
        blockhash,
        instructions,
        signatures,
        message,
        v1_config: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::{decode as decode_any, ProbeParams};
    use ed25519_dalek::{Signature, Verifier};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn probe_roundtrip_and_signature() {
        let payer = key(7);
        let partner = [9u8; 32];
        let bh = [5u8; 32];
        let p = ProbeParams::legacy(1_000, 100_000);
        let msg = probe_message(&payer.verifying_key().to_bytes(), Some(&partner), &p, &bh).unwrap();
        let wire = msg.sign(&[&payer]).unwrap();
        // 1 (sig count) + 64 + 3 + 1 + 4*32 + 32 + 1 + (1+1+1+5) + (1+1+1+9) + (1+1+2+1+12)
        assert_eq!(wire.len(), 1 + 64 + 3 + 1 + 128 + 32 + 1 + 8 + 12 + 17);
        let d = decode_any(&wire).unwrap();
        assert_eq!(d.format, Format::Legacy);
        assert_eq!(d.accounts[1], partner);
        assert_eq!(d.instructions.len(), 3);
        assert_eq!(d.instructions[0].data, vec![2, 0xe8, 0x03, 0, 0]);
        let sig = Signature::from_bytes(&d.signatures[0]);
        payer.verifying_key().verify(&d.message, &sig).unwrap();
        let data_bytes: usize = d.instructions.iter().map(|i| i.data.len()).sum();
        assert_eq!(data_bytes as u64, crate::tx::cost::LEGACY_IX_DATA_BYTES);
    }

    #[test]
    fn self_transfer_probe() {
        let payer = key(7);
        let pk = payer.verifying_key().to_bytes();
        let p = ProbeParams::legacy(1_200, 80_000);
        let d = decode_any(&probe_message(&pk, None, &p, &[5u8; 32]).unwrap().sign(&[&payer]).unwrap()).unwrap();
        assert_eq!(d.header, [1, 0, 2]);
        assert_eq!(d.accounts.len(), 3);
        assert_eq!(d.instructions[2].program_index, 1);
        assert_eq!(d.instructions[2].accounts, vec![0, 0]);
        assert_eq!(d.instructions[0].program_index, 2);
        assert!(probe_message(&pk, Some(&pk), &p, &[5u8; 32]).is_err());
    }

    #[test]
    fn golden_bytes() {
        // Fixed key, partner and blockhash: the serialised message must never change silently.
        let payer = key(1);
        let p = ProbeParams::legacy(1_000, 300_000);
        let msg = probe_message(&payer.verifying_key().to_bytes(), Some(&[2u8; 32]), &p, &[3u8; 32]).unwrap();
        let hex: String = msg.serialize().iter().map(|b| format!("{b:02x}")).collect();
        let expected = concat!(
            "010002",
            "04",                                                               // header, 4 accounts
            "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c", // payer
            "0202020202020202020202020202020202020202020202020202020202020202", // partner
            "0000000000000000000000000000000000000000000000000000000000000000", // System
            "0306466fe5211732ffecadba72c39be7bc8ce5bbc5f7126b2c439b3a40000000", // ComputeBudget
            "0303030303030303030303030303030303030303030303030303030303030303", // blockhash
            "03",                                                               // 3 instructions
            "030005",
            "02e8030000", // SetComputeUnitLimit(1000)
            "030009",
            "03e093040000000000", // SetComputeUnitPrice(300000)
            "02020001",
            "0c",
            "020000000100000000000000", // transfer 1 lamport payer -> partner
        );
        assert_eq!(hex, expected);
    }

    #[test]
    fn fund_and_memo_decode() {
        let funder = key(4);
        let m = fund_message(&funder.verifying_key().to_bytes(), &[([1; 32], 10), ([2; 32], 20)], &[0; 32]);
        let d = decode_any(&m.sign(&[&funder]).unwrap()).unwrap();
        assert_eq!(d.instructions.len(), 2);
        assert_eq!(d.instructions[1].accounts, vec![0, 2]);
        let memo = memo_message(&funder.verifying_key().to_bytes(), "itb prereg", &[0; 32]).unwrap();
        let d = decode_any(&memo.sign(&[&funder]).unwrap()).unwrap();
        assert_eq!(d.instructions[0].data, b"itb prereg".to_vec());
    }
}

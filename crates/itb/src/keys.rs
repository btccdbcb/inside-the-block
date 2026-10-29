//! Key files. The funder key is only ever read inside the live service (path from ITB_FUNDER_KEY);
//! sub-payer keys are generated here and only their public keys are printed.

use crate::tx::{b58, Pubkey};
use crate::util::random_bytes;
use anyhow::{bail, ensure, Context, Result};
use ed25519_dalek::SigningKey;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Keypair {
    pub signing: SigningKey,
}

impl Keypair {
    pub fn generate() -> Self {
        Keypair { signing: SigningKey::from_bytes(&random_bytes::<32>()) }
    }

    pub fn pubkey(&self) -> Pubkey {
        self.signing.verifying_key().to_bytes()
    }

    pub fn pubkey_b58(&self) -> String {
        b58(&self.pubkey())
    }

    /// Solana CLI key file: JSON array of 64 numbers (32-byte seed followed by the 32-byte public key).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read key file {}", path.display()))?;
        let bytes: Vec<u8> = serde_json::from_str(&text).context("key file is not a JSON byte array")?;
        ensure!(bytes.len() == 64, "key file must hold 64 bytes");
        let seed: [u8; 32] = bytes[..32].try_into().unwrap();
        let kp = Keypair { signing: SigningKey::from_bytes(&seed) };
        ensure!(kp.pubkey()[..] == bytes[32..], "key file public half does not match its secret half");
        Ok(kp)
    }

    /// Write a new key file with mode 0600. Refuses to overwrite.
    pub fn save_new(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut bytes = self.signing.to_bytes().to_vec();
        bytes.extend_from_slice(&self.pubkey());
        let text = serde_json::to_string(&bytes)?;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
            .with_context(|| format!("create key file {} (refusing to overwrite)", path.display()))?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        Ok(())
    }
}

pub const PAYER_COUNT: usize = 12;

pub fn payer_path(dir: &Path, i: usize) -> PathBuf {
    dir.join("payers").join(format!("payer-{i:02}.json"))
}

/// Load the 12 sub-payers from `<secrets>/payers/`, creating any that are missing.
pub fn load_or_create_payers(secrets_dir: &Path) -> Result<Vec<Keypair>> {
    let mut out = Vec::with_capacity(PAYER_COUNT);
    for i in 0..PAYER_COUNT {
        let p = payer_path(secrets_dir, i);
        if p.exists() {
            out.push(Keypair::load(&p)?);
        } else {
            let kp = Keypair::generate();
            kp.save_new(&p)?;
            out.push(kp);
        }
    }
    Ok(out)
}

pub fn load_payers(secrets_dir: &Path) -> Result<Vec<Keypair>> {
    let mut out = Vec::with_capacity(PAYER_COUNT);
    for i in 0..PAYER_COUNT {
        let p = payer_path(secrets_dir, i);
        if !p.exists() {
            bail!("missing payer key {} (run `itb keys new-payers`)", p.display());
        }
        out.push(Keypair::load(&p)?);
    }
    Ok(out)
}

/// Throw-away signers for dry runs: never funded, never stored.
pub fn ephemeral_payers() -> Vec<Keypair> {
    (0..PAYER_COUNT).map(|_| Keypair::generate()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrip_and_no_overwrite() {
        let dir = std::env::temp_dir().join(format!("itb-keytest-{}", crate::util::wall_ns()));
        let kp = Keypair::generate();
        let p = dir.join("k.json");
        kp.save_new(&p).unwrap();
        let back = Keypair::load(&p).unwrap();
        assert_eq!(back.pubkey(), kp.pubkey());
        assert!(kp.save_new(&p).is_err());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

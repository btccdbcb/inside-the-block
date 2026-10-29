//! Small helpers: clocks, JSONL output, hashing.

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static ORIGIN: OnceLock<(Instant, u128)> = OnceLock::new();

fn origin() -> &'static (Instant, u128) {
    ORIGIN.get_or_init(|| {
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        (Instant::now(), wall)
    })
}

/// Monotonic nanoseconds since process start.
pub fn mono_ns() -> u64 {
    origin().0.elapsed().as_nanos() as u64
}

/// Wall-clock nanoseconds since the Unix epoch, derived from the monotonic clock so that differences are
/// exact within one process (no NTP slews mid-run).
pub fn wall_ns() -> u128 {
    origin().1 + origin().0.elapsed().as_nanos()
}

pub fn wall_ms() -> i64 {
    (wall_ns() / 1_000_000) as i64
}

/// Monotonic ns at which the wall clock reads `wall_ms`.
pub fn mono_ns_at_wall_ms(wall_ms: f64) -> i64 {
    let (_, wall0) = origin();
    ((wall_ms * 1e6) - *wall0 as f64) as i64
}

pub fn utc_from_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

pub fn utc_now() -> String {
    utc_from_ms(wall_ms())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let b = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(sha256_hex(&b))
}

/// Append-only JSONL writer. Every line is flushed; `sync` also fsyncs (used for money-related logs).
pub struct Jsonl {
    file: Mutex<File>,
    sync: bool,
}

impl Jsonl {
    pub fn open(path: &Path, sync: bool) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)
            .with_context(|| format!("open {}", path.display()))?;
        Ok(Jsonl { file: Mutex::new(file), sync })
    }

    pub fn write<T: Serialize>(&self, row: &T) -> Result<()> {
        let mut line = serde_json::to_string(row)?;
        line.push('\n');
        let mut f = self.file.lock().unwrap();
        f.write_all(line.as_bytes())?;
        f.flush()?;
        if self.sync {
            f.sync_data()?;
        }
        Ok(())
    }
}

/// Structured log line on stderr (one JSON object per line).
pub fn log(event: &str, fields: serde_json::Value) {
    let mut obj = serde_json::Map::new();
    obj.insert("utc".into(), utc_now().into());
    obj.insert("event".into(), event.into());
    if let serde_json::Value::Object(m) = fields {
        obj.extend(m);
    }
    eprintln!("{}", serde_json::Value::Object(obj));
}

/// Remove userinfo and query string from a URL before it is logged or stored.
pub fn redact_url(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or("");
    if let Some((scheme, rest)) = no_query.split_once("://") {
        let host_part = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
        format!("{scheme}://{host_part}")
    } else {
        no_query.to_string()
    }
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("OS random source");
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_keys_from_urls() {
        assert_eq!(redact_url("https://user:pw@rpc.example/?api-key=abc"), "https://rpc.example/");
        assert_eq!(redact_url("wss://x.example/path?token=1"), "wss://x.example/path");
    }

    #[test]
    fn utc_format() {
        assert_eq!(utc_from_ms(0), "1970-01-01T00:00:00.000Z");
    }
}

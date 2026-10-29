//! itb: the sender side of the Inside the Block study.
//!
//! Every command is read-only or dry unless it is given `--live`. See `itb --help`.

pub mod auth;
pub mod clock;
pub mod design;
pub mod gossip;
pub mod keys;
pub mod ledger;
pub mod live;
pub mod money;
pub mod programme;
pub mod quic;
pub mod reconcile;
pub mod rpc;
pub mod run;
pub mod schedule;
pub mod tx;
pub mod util;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tx::library::{FeeScale, Probe};
use util::{log, utc_now, wall_ms, Jsonl};

#[derive(Parser)]
#[command(name = "itb", version, about = "Inside the Block: probe sender, slot clock, spend ledger. Dry by default.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Gossip + stake + Kobe (+ validators.app) snapshot and the view of each target.
    Recon {
        #[arg(long, default_value = "design/targets.json")]
        targets: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Record first-shred events for N minutes and report slot gaps by position in the turn.
    Calibrate {
        #[arg(long, default_value_t = 10)]
        minutes: u64,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Upcoming turns of the targets.
    Schedule {
        #[arg(long, default_value = "design/targets.json")]
        targets: PathBuf,
        #[arg(long, default_value_t = 1.0)]
        hours: f64,
    },
    /// QUIC handshake + RTT to each target's TPU (no transaction bytes are sent).
    QuicPing {
        #[arg(long, default_value = "design/targets.json")]
        targets: PathBuf,
        #[arg(long, default_value_t = 3)]
        rounds: u32,
    },
    /// Build a probe and run simulateTransaction against mainnet (free, sends nothing).
    Simulate {
        #[arg(long, default_value = "A")]
        probe: String,
        /// Fee payer for the simulation (public key only; signature check is off).
        #[arg(long, env = "ITB_FUNDER_PUBKEY", default_value = "AET41ewiRXWPYRnSupUrYKaq51zUrKHJnq3FcCCkJ3Li")]
        payer: String,
        #[arg(long, default_value = "s1")]
        scale: String,
    },
    /// Dry run of design sessions: real schedule, clock, QUIC handshakes and signed quads; nothing is sent.
    DryRun {
        #[arg(long, default_value = "design")]
        design: PathBuf,
        /// comma-separated design sessions whose rows are fired (e.g. A,B)
        #[arg(long, default_value = "A")]
        sessions: String,
        #[arg(long, default_value_t = 60)]
        minutes: u64,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "s1")]
        scale: String,
        #[arg(long)]
        no_v1: bool,
        /// localnet: send every target's rows to this identity
        #[arg(long)]
        map_all_to: Option<String>,
    },
    /// The whole live programme (fund, smoke, pilot, pilot rules, Memo, sessions, sweep). Dry unless --live.
    Live {
        #[arg(long)]
        live: bool,
        /// localnet tests only
        #[arg(long)]
        allow_non_mainnet: bool,
        #[arg(long, default_value_t = 0.03)]
        max_spend_sol: f64,
        #[arg(long, env = "ITB_DESIGN_DIR", default_value = "design")]
        design: PathBuf,
        #[arg(long, env = "ITB_STATE_DIR", default_value = "state")]
        state: PathBuf,
        #[arg(long, env = "ITB_SECRETS_DIR", default_value = "secrets")]
        secrets: PathBuf,
        #[arg(long, env = "ITB_FUNDER_KEY", default_value = "secrets/funder.json")]
        funder_key: PathBuf,
        /// localnet rehearsal: windows of this many minutes, back to back
        #[arg(long)]
        rehearsal_minutes: Option<u64>,
        /// localnet rehearsal: send every target's rows to this identity
        #[arg(long)]
        map_all_to: Option<String>,
    },
    /// Decode a base64 transaction (legacy or v1) and check its signatures.
    Decode { b64: String },
    /// Key management (prints public keys only).
    Keys {
        #[command(subcommand)]
        cmd: KeysCmd,
    },
}

#[derive(Subcommand)]
enum KeysCmd {
    /// Create the 12 sub-payer keys in <secrets>/payers/ (never overwrites).
    NewPayers {
        #[arg(long, env = "ITB_SECRETS_DIR", default_value = "secrets")]
        secrets: PathBuf,
    },
    /// Public keys and balances of the funder and the payers.
    Show {
        #[arg(long, env = "ITB_SECRETS_DIR", default_value = "secrets")]
        secrets: PathBuf,
        #[arg(long, env = "ITB_FUNDER_PUBKEY", default_value = "AET41ewiRXWPYRnSupUrYKaq51zUrKHJnq3FcCCkJ3Li")]
        funder: String,
    },
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct TargetEntry {
    pub slot: String,
    pub identity: String,
    pub expected_client: String,
    #[serde(default)]
    pub alternate: Option<String>,
    #[serde(default)]
    pub extended: bool,
}

pub fn read_targets(path: &std::path::Path) -> Result<Vec<TargetEntry>> {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?)?;
    Ok(serde_json::from_value(v["targets"].clone())?)
}

fn parse_scale(s: &str) -> Result<FeeScale> {
    match s {
        "s1" => Ok(FeeScale::S1),
        "s10" => Ok(FeeScale::S10),
        _ => bail!("fee scale must be s1 or s10"),
    }
}

fn parse_probe(s: &str) -> Result<Probe> {
    Ok(match s {
        "A" => Probe::A,
        "B" => Probe::B,
        "Aprime" | "A'" => Probe::APrime,
        "HI" => Probe::Hi,
        "X" => Probe::X,
        "Z" => Probe::Z,
        "A1" => Probe::A1,
        "HI1" => Probe::Hi1,
        _ => bail!("unknown probe {s}"),
    })
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        if let Some(s) = e.downcast_ref::<programme::Stopped>() {
            log("stopped", json!({"reason": s.0}));
            std::process::exit(programme::EXIT_STOPPED);
        }
        log("fatal", json!({"error": format!("{e:#}")}));
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Recon { targets, out } => cmd_recon(&targets, out).await,
        Cmd::Calibrate { minutes, out } => cmd_calibrate(minutes, out).await,
        Cmd::Schedule { targets, hours } => cmd_schedule(&targets, hours).await,
        Cmd::QuicPing { targets, rounds } => cmd_quic_ping(&targets, rounds).await,
        Cmd::Simulate { probe, payer, scale } => cmd_simulate(&probe, &payer, &scale).await,
        Cmd::Decode { b64 } => cmd_decode(&b64),
        Cmd::Live {
            live,
            allow_non_mainnet,
            max_spend_sol,
            design,
            state,
            secrets,
            funder_key,
            rehearsal_minutes,
            map_all_to,
        } => {
            if !live {
                bail!("`itb live` moves SOL and needs --live (use `itb dry-run` for a dry run)");
            }
            if (map_all_to.is_some() || rehearsal_minutes.is_some()) && !allow_non_mainnet {
                bail!("--map-all-to and --rehearsal-minutes are for localnet rehearsals (--allow-non-mainnet)");
            }
            let p = programme::Programme::start(programme::Args {
                live,
                allow_non_mainnet,
                max_spend_lamports: auth::sol_to_lamports(max_spend_sol),
                design,
                state,
                secrets,
                funder_key,
                rehearsal_minutes,
                map_all_to,
            })
            .await?;
            p.run().await
        }
        Cmd::DryRun { design, sessions, minutes, out, scale, no_v1, map_all_to } => {
            live::dry_run(live::DryArgs {
                design,
                sessions: sessions.split(',').map(|s| s.trim().to_string()).collect(),
                minutes,
                out_root: out.unwrap_or_else(live::default_out_root),
                fee_scale: parse_scale(&scale)?,
                v1: !no_v1,
                map_all_to,
            })
            .await
        }
        Cmd::Keys { cmd } => match cmd {
            KeysCmd::NewPayers { secrets } => {
                let ps = keys::load_or_create_payers(&secrets)?;
                for (i, p) in ps.iter().enumerate() {
                    println!("payer-{i:02} {}", p.pubkey_b58());
                }
                Ok(())
            }
            KeysCmd::Show { secrets, funder } => {
                let rpc = rpc::Rpc::from_env()?;
                let mut names = vec![("funder".to_string(), tx::pubkey_from_b58(&funder)?)];
                if let Ok(ps) = keys::load_payers(&secrets) {
                    for (i, p) in ps.iter().enumerate() {
                        names.push((format!("payer-{i:02}"), p.pubkey()));
                    }
                }
                let keys_only: Vec<_> = names.iter().map(|(_, k)| *k).collect();
                let (slot, bals) = rpc.balances(&keys_only).await?;
                println!("slot {slot}");
                for ((n, k), b) in names.iter().zip(bals) {
                    println!("{n} {} {b}", tx::b58(k));
                }
                Ok(())
            }
        },
    }
}

async fn cmd_recon(targets: &std::path::Path, out: Option<PathBuf>) -> Result<()> {
    let rpc = rpc::Rpc::from_env()?;
    let snap = gossip::snapshot(&rpc).await?;
    let kobe = gossip::kobe_running_bam().await.unwrap_or_default();
    let vapp = gossip::validators_app().await.unwrap_or(None);
    let ts = read_targets(targets).unwrap_or_default();
    let mut views = Vec::new();
    for t in &ts {
        for id in std::iter::once(&t.identity).chain(t.alternate.iter()) {
            let v = snap.view(id);
            let va = vapp.as_ref().and_then(|m| m.get(id)).cloned();
            println!(
                "{:4} {:44} {:20} {:10} {:22} share={:3} {:8} bam={:?} vapp={:?}",
                t.slot,
                id,
                v.client_id.clone().unwrap_or_default(),
                v.version.clone().unwrap_or_default(),
                v.tpu_quic.clone().unwrap_or_default(),
                v.host_share,
                v.ingress_kind,
                kobe.get(id),
                va
            );
            views.push(json!({"slot": t.slot, "view": v, "running_bam": kobe.get(id), "validators_app": va}));
        }
    }
    if let Some(dir) = out {
        std::fs::create_dir_all(&dir)?;
        let stamp = utc_now().replace([':', '-'], "").replace('.', "");
        std::fs::write(
            dir.join(format!("recon-{stamp}.json")),
            serde_json::to_string(&json!({
                "utc": snap.utc, "targets": views, "nodes": snap.compact(),
                "kobe_running_bam": kobe, "validators_app": vapp.map(|m| m.into_iter().map(|(k, (c, v))| (k, json!([c, v]))).collect::<serde_json::Map<_, _>>()),
            }))?,
        )?;
    }
    Ok(())
}

async fn cmd_calibrate(minutes: u64, out: Option<PathBuf>) -> Result<()> {
    let sink = match &out {
        Some(p) => Some(Arc::new(Jsonl::open(p, false)?)),
        None => None,
    };
    let clock = clock::SlotClock::start(clock::SlotClock::urls_from_env(), sink);
    let end = wall_ms() + (minutes * 60_000) as i64;
    let mut errs: Vec<f64> = Vec::new();
    let mut pending: Vec<(u64, f64)> = Vec::new();
    while wall_ms() < end {
        tokio::time::sleep(Duration::from_millis(250)).await;
        // predict the slot 4..8 ahead of the latest one, check it when observed
        if let Some((latest, _)) = clock.latest() {
            let target = schedule::turn_start(latest) + 8;
            if pending.last().map(|p| p.0) != Some(target) {
                if let Some(p) = clock.predict(target) {
                    if !p.observed {
                        pending.push((target, p.ms));
                    }
                }
            }
        }
        pending.retain(|(s, ms)| match clock.observed(*s) {
            Some(o) => {
                errs.push(o.node_ms - ms);
                false
            }
            None => clock.latest().map(|(l, _)| l < s + 20).unwrap_or(true),
        });
    }
    errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| errs.get(((errs.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(f64::NAN);
    let abs: Vec<f64> = {
        let mut a: Vec<f64> = errs.iter().map(|e| e.abs()).collect();
        a.sort_by(|x, y| x.partial_cmp(y).unwrap());
        a
    };
    let qa = |p: f64| abs.get(((abs.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(f64::NAN);
    let report = json!({"gap_medians_ms_by_position": clock.gap_medians(), "gap_counts": clock.gap_counts(),
        "turn_start_prediction_error_ms": {"n": errs.len(), "p10": q(0.1), "median": q(0.5), "p90": q(0.9), "abs_median": qa(0.5), "abs_p90": qa(0.9)}});
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn cmd_schedule(targets: &std::path::Path, hours: f64) -> Result<()> {
    let rpc = rpc::Rpc::from_env()?;
    let ts = read_targets(targets)?;
    let ids: Vec<String> = ts.iter().map(|t| t.identity.clone()).collect();
    let now = rpc.slot("confirmed").await?;
    let until = now + (hours * 3600.0 / 0.2) as u64;
    let s = schedule::Schedule::load(&rpc, &ids, now, now.min(until)).await?;
    let turns = s.turns_between(now, until);
    let mut per: std::collections::BTreeMap<&str, usize> = Default::default();
    for (_, t) in &turns {
        *per.entry(t.as_str()).or_default() += 1;
    }
    println!("epoch {} slot {now}: {} target turns in the next {hours} h", s.epoch, turns.len());
    for t in &ts {
        println!("{:4} {:44} {}", t.slot, t.identity, per.get(t.identity.as_str()).unwrap_or(&0));
    }
    Ok(())
}

async fn cmd_quic_ping(targets: &std::path::Path, rounds: u32) -> Result<()> {
    let rpc = rpc::Rpc::from_env()?;
    let snap = gossip::snapshot(&rpc).await?;
    let client = quic::QuicClient::new()?;
    for t in read_targets(targets)? {
        for id in std::iter::once(&t.identity).chain(t.alternate.iter()) {
            let v = snap.view(id);
            let Some(addr) = v.tpu_quic.clone() else {
                println!("{:4} {id} no tpuQuic in gossip", t.slot);
                continue;
            };
            let mut ok = 0;
            let mut hs = Vec::new();
            let mut last_err = String::new();
            for _ in 0..rounds {
                match client.connect(&addr, Duration::from_secs(3)).await {
                    Ok((c, us)) => {
                        ok += 1;
                        hs.push(us / 1000);
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        let alive = quic::is_open(&c);
                        let rtt = c.rtt().as_millis();
                        c.close(0u32.into(), b"done");
                        hs.push(rtt as u64 * 1_000_000 + alive as u64);
                    }
                    Err(e) => last_err = e.to_string(),
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let handshakes: Vec<u64> = hs.iter().step_by(2).copied().collect();
            let rtts: Vec<u64> = hs.iter().skip(1).step_by(2).map(|x| x / 1_000_000).collect();
            let alive: Vec<u64> = hs.iter().skip(1).step_by(2).map(|x| x % 2).collect();
            println!(
                "{:4} {:44} {:18} {:22} ok {ok}/{rounds} handshake_ms {:?} rtt_ms {:?} alive_after_300ms {:?} {}",
                t.slot,
                id,
                v.client_id.unwrap_or_default(),
                addr,
                handshakes,
                rtts,
                alive,
                last_err
            );
        }
    }
    Ok(())
}

async fn cmd_simulate(probe: &str, payer: &str, scale: &str) -> Result<()> {
    let rpc = rpc::Rpc::from_env()?;
    let p = parse_probe(probe)?;
    let params = p.params(parse_scale(scale)?);
    let payer = tx::pubkey_from_b58(payer)?;
    let lock = util::random_bytes::<32>();
    let (bh, _, _) = rpc.latest_blockhash().await?;
    // The payer's key is not available here, so the transaction carries a zero signature and the
    // simulation runs with sigVerify off. Signatures are tested with our own keys on localnet.
    let unsigned = match params.format {
        tx::Format::Legacy => {
            let m = tx::legacy::probe_message(&payer, &lock, &params, &bh)?.serialize();
            let mut b = vec![1u8];
            b.extend_from_slice(&[0u8; 64]);
            b.extend_from_slice(&m);
            b
        }
        tx::Format::V1 => {
            let mut b = tx::v1::probe_message(&payer, &lock, &params, &bh)?.serialize();
            b.extend_from_slice(&[0u8; 64]);
            b
        }
    };
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&unsigned);
    let v = rpc.simulate(&b64, false).await?;
    let val = &v["value"];
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "probe": p.name(), "params": params, "predicted_cost": tx::cost::probe_cost(&params),
            "predicted_fee": tx::cost::probe_fee(&params), "predicted_priority": tx::cost::probe_priority(&params),
            "err": val["err"], "unitsConsumed": val["unitsConsumed"], "fee": val["fee"], "logs": val["logs"],
        }))?
    );
    if !val["err"].is_null() {
        bail!("simulation returned an error");
    }
    Ok(())
}

fn cmd_decode(b64: &str) -> Result<()> {
    use base64::Engine;
    use ed25519_dalek::Verifier;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim())?;
    let d = tx::decode(&bytes)?;
    let mut sig_ok = true;
    for (i, s) in d.signatures.iter().enumerate() {
        let vk = ed25519_dalek::VerifyingKey::from_bytes(&d.accounts[i])?;
        sig_ok &= vk.verify(&d.message, &ed25519_dalek::Signature::from_bytes(s)).is_ok();
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "format": format!("{:?}", d.format), "signature": tx::b58(&d.signatures[0]), "signatures_valid": sig_ok,
            "accounts": d.accounts.iter().map(|a| tx::b58(a)).collect::<Vec<_>>(), "blockhash": tx::b58(&d.blockhash),
            "instructions": d.instructions.iter().map(|ix| json!({"program_index": ix.program_index, "accounts": ix.accounts,
                "data_hex": ix.data.iter().map(|b| format!("{b:02x}")).collect::<String>()})).collect::<Vec<_>>(),
            "v1_config": d.v1_config.map(|c| json!({"mask": c.mask, "priority_fee": c.priority_fee, "cu_limit": c.cu_limit, "loaded_data": c.loaded_data})),
        }))?
    );
    Ok(())
}

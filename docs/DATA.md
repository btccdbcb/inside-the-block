# Data

Everything the results rest on is in this repository. Times ending in `_ms` are Unix milliseconds on the study
machine's clock. Every timing measure (τ, off0, Δt) is a difference within that one frame, so a constant clock offset
cancels. For reference, the clock was 3 ms behind time.cloudflare.com, time.google.com and pool.ntp.org on 2026-10-29
17:41 UTC (SNTP, best of three queries each). Times ending in `_utc` are ISO 8601 UTC.
Signatures and public keys are base58. Transactions are base64 wire bytes.

## Layout

| Path | What | Written by |
|---|---|---|
| `design/targets.json` | The frozen targets: slot (T1…T13, X1), identity, family, alternate, stake share, turns/day | `scripts/select_targets.py` |
| `design/design.jsonl` | One line per planned quad, with every randomised factor | `itb-analysis design` |
| `design/campaign.json` | Sessions, caps, sweep address, allocation, seed, hashes of the frozen files and the confirmatory code | `itb-analysis design` |
| `data/raw/<run_id>/` | One directory per run of the live programme (smoke, pilot, sessions A–D), collected | the runner, then `itb-analysis collect` |
| `data/live/` | The live programme's ledger, log and pilot decision | the runner |
| `data/dry_runs/<run_id>/` | Mainnet dry runs: nothing sent ([README](../data/dry_runs/README.md)) | the runner in dry mode |
| `results/` | Derived tables and test results | `docker compose up` (`itb-analysis reproduce`) |
| `figures/` | The figures, light and dark | same |

## `data/raw/<run_id>/`

**`quads.jsonl`**: one line per quad the runner attempted.

- Identity: `quad_id`, `run_id`, `session`, `target_slot`, `identity`, `prev_leader` (leader of slot0 − 1), `tpu_quic`.
- `view`: the target's gossip entry at decision time (`client_id`, `version`, `stake`, `tpu_quic`, `ingress_kind`,
  `host_share`).
- Design factors: `tau_ms`, `main_type`, `dt_ms`, `main_first`, `control_type` (after the pilot's v1 rule),
  `control_type_design`, `control_gap_ms`, `control_first`, `fee_scale`, `pool`.
- Turn: `slot0` is the target's first slot.
- Timing: `anchor` is `observed` (slot0 − 1 was seen) or `predicted`; `prediction` holds the anchor details;
  `planned_fire_ms`; `first_send_ms`; `send_late_ms` (first send minus plan); `handshake_us`; `decided_utc`; `fired_utc`.
- Blockhash: `blockhash`, `blockhash_slot`, `last_valid_height`.
- `status`: `fired` or `skipped`, with `reason` for a skip.
- `probes`: four entries with `role`, `probe`, `signature`, `payer`, `to`, `fee` (the expected fee) and `offset_us`.
- `live`: false in a dry run.

**`sends.jsonl`**: one line per probe, with the signed bytes (`tx_b64`) logged before they were written.

**`streams.jsonl`**: per probe, the QUIC stream times (`open_ms`, `write_ms`, `finish_ms`, `planned_ms`), `rtt_us`,
`acked`, `bytes_left` and `error`. The send time is `write_ms`.

**`slots.jsonl.gz`**: the slot stream, one line per (source, slot): `node_ms` (the RPC node's first-shred timestamp)
and `recv_ms` (when we received it). The observed first shred of a slot is the earliest over sources of
`node_ms + min(0, recv_ms − node_ms)` (`outcomes.first_shred_frame`).

**`gossip.jsonl.gz`**: every 60 s, each target's gossip view. It is used for the client rule (PREREGISTRATION §4,
eligibility 3).

**`blocks/<slot>.json.gz`**: block excerpts from `getBlock` (transaction version 1, rewards on).

- `leader` is the Fee-reward recipient.
- `sig4` lists the first 4 bytes (hex) of every signature, in block order.
- `mode` is `full` for the target's turn and any block one of our probes landed in, `signatures` for the preceding
  leader's turn, or `skipped`.
- Full excerpts add per-transaction `cost_units`, `fee`, `err` and `sum_cost_units`, and our probes in full
  (`index`, `version`, `tx_b64`, `meta`).
- Full excerpts also hold, per transaction, what its sender asked for, decoded from its bytes: `n_sig` (signatures),
  `n_writable` (writable accounts, lookup-table ones included), `cu_limit`, `cu_price` (micro-lamports per CU) and
  `loaded_data` (each null where the transaction does not set it). They are used only for the whole-block description
  in REPORT.md, never for a pre-registered test.

**`outcomes.jsonl`**: one line per probe.

- `landed`, `slot`, `index_in_block`, `slot_of_turn`, `leader`.
- `in_target_turn`: landed in slot0 … slot0 + 3, in a block whose leader is the target.
- `landed_with`: `target`, `preceding` or `other`.
- `meta_fee`, `fee_expected`, `meta_cost_units`, `cu_consumed`, `err`, `block_len`, `block_cost_units`.
- `status_slot` and `status_err`: the `getSignatureStatuses` cross-check.

**`run.json`**: counts, the completeness check result (must be empty) and the RPC hosts used.

## `results/`

- `pairs.csv`: one row per pair (main and control of every fired quad). It holds the quad's timing (`tau_ach`, `off0`,
  `turn_bin`), its client views and the pair coding (`pref_sent_first`, `dt_ach`, `pref_in_turn`, `other_in_turn`,
  `both_in_turn`, `pref_first`, `first_sent_first`, `load_cu`). `eligible` and `exclusion` (comma-separated reasons)
  apply PREREGISTRATION §4.
- `probes.csv`: one row per probe, with the quad's timing and its outcome.
- `results.json`: every test, estimate, interval, Holm-adjusted p-value, verdict, sensitivity analysis and descriptive
  table of PREREGISTRATION §5.
- `explore.json`: the exploratory analyses (`analysis/itb_analysis/explore.py`): mechanism fits to the race-reversal
  rates, insertion position, hour of day, differences between operators of one family, transactions between pair
  members, and how sorted whole blocks are. No verdict rests on them.
- `timing.json`: gate G2 and skip reasons per target, for the dry runs and the campaign.
- `verify.json`: the full offline verification ([VERIFY.md](VERIFY.md)).
- `power.json`: the power study ([POWER.md](POWER.md)).

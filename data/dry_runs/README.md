# Mainnet dry runs

A dry run is the live runner with sending switched off: it reads the real leader schedule, waits for each planned turn,
opens a real QUIC connection to the target and builds and signs the probes, but never writes them to the network.
It measures what the sender would have done. Gate G2 (PREREGISTRATION §5) and the reachability rule for targets
(§2) are computed from these files by `analysis/itb_analysis/timing.py` (`docker compose up` reruns it into
`results/timing.json`).

Each directory holds `quads.jsonl` (one line per planned quad: planned and achieved send times, anchor, skip reason),
`slots.jsonl.gz` (the slot stream: per source and slot, the node's first-shred timestamp and our receive time),
`gossip.jsonl.gz` (each target's client id, version and TPU address every 60 s) and `windows.jsonl`.

| Run | Command (commit) | UTC | Rows planned | Notes |
|---|---|---|---|---|
| 20261029T113453109-dry | `itb dry-run --sessions A,B,C,D --minutes 1200` (b7121c4) | 2026-10-29 11:34–16:27 | 854 (194 reached) | Stopped by me after 5 h to redo the targets: two of them never completed a QUIC handshake at their turns, which led to the reachability rule. Each line of `quads.jsonl` names its target slot and identity. |

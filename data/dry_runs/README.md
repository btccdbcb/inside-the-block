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
| 20261029T163724083-dry | `itb dry-run --sessions A,B,C,D --minutes 1200` (531f504 plus runner fixes made before the next commit: re-check and reconnect of the QUIC connection, pre-open) | 2026-10-29 16:37–2026-10-30 05:15 | 898 (494 reached) | Run on the redrawn targets and the new probe shape. 486 quads reached the point where the sender would have sent (all four probes built and signed, connection open); 8 were skipped (4 late, 4 first shred of the preceding slot not observed). Stopped by me at 05:15 UTC, before the freeze. The WebSocket to one of the two slot sources dropped silently often (1,019 `ws_silent_reconnect` lines in the log); the clock fell back to the second source each time. The second source answered HTTP 429 to 2,625 reconnect attempts between 2026-10-29 23:50 and 2026-10-30 00:23 UTC, because the clock retried every 0.5 s with no backoff (counted with `grep` on the run's stderr log, which is not committed); the clock now doubles the wait after each failure, up to 30 s. Counts are from `quads.jsonl` (status and skip reason of each line) and the design file the run used. |

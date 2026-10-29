# Design

How the measurement works and why it is built this way. The frozen rules (targets, tests, thresholds) are in
[PREREGISTRATION.md](PREREGISTRATION.md); this file explains the reasoning and the machinery. Numbers here are either
read from a source (named), measured (data and command named) or chosen before any data (reason given).

## One measurement: a quad

In one leader turn of one target (four consecutive slots) I send four probes, as two pairs:

- a **main pair** whose two members differ in what the scheduler might sort by (R, C1, C2, C3, R_IND), and
- a **control pair** of two identical keys (EQ), or a legacy/v1 pair for the v1 arm (V_EQ, V_HI).

Every probe is a one-signature legacy (or v1) transaction that sets its compute-unit limit and price and moves
1 lamport from its own payer to the other member's payer. Both members of a pair therefore write the same two accounts,
so the leader cannot execute them in parallel: one of them must come first, and the block shows which. The two pairs of
a quad share no account, so they do not constrain each other.

The probe parameters and the key each candidate rule computes are in PREREGISTRATION §2; the cost formula and its
sources are in §9. `crates/itb/src/tx/cost.rs` computes every probe's cost and key, and its unit tests pin the order of
every pair.

## Why this design

**Pairs that conflict.** Two transactions that write the same account are executed one after the other, so their order
in the block is a decision the leader made. Transactions that touch different accounts can be reordered or parallelised
for reasons unrelated to priority. R_IND (same keys as R, independent accounts) shows how much "order" appears without
a conflict.

**Contrasts where the candidate keys disagree.** If a leader sorts, it sorts by some key: reward per cost (the Agave
scheduler's priority, `core/src/transaction_priority.rs` at v4.3.2), CU price, or total fee. In C1 the reward/cost key
and the total-fee key prefer different members; in C2 the reward/cost key and the CU-price key disagree; in C3 every key
prefers the same member, which checks that sorting happens at all. Arrival order prefers neither member because the send
order is randomised. Which member lands first in C1 and C2 therefore names the key.

**An equal-key pair in every turn.** Same leader, same moment, same network path, same block load, but nothing for a
sorting scheduler to sort. Its order measures what breaks ties: arrival (F > 0.5), the latest arrival (F < 0.5) or
nothing related to arrival (F = 0.5). This is P1, the first confirmatory test.

**Randomised send offset τ.** A leader's turn has a hand-over: transactions that arrive before the turn starts sit in a
buffer, transactions that arrive during the turn meet a scheduler that is already producing blocks. τ is drawn per quad
from ten levels between 300 ms before and 900 ms after the first shred of the slot before the turn, balanced within
target and session. Position in the turn is then a measured factor, not a hidden one. P2 asks whether landing depends on
τ; S3 asks the same of leakage to the preceding leader; S1 asks whether a pre-turn backlog is sorted.

**Randomised gap Δt.** When the cheaper member is sent Δt ms before the dearer one, a scheduler that sorts a pending set
still puts the dearer one first if both are pending together; a scheduler that has already executed the cheaper one
cannot. The race-reversal rate against Δt (0, 2, 10, 25, 50, 100 ms) separates arrival order, a continuously drained
queue and batches of fixed length (exploratory fits, REPORT §5).

**Two validators per client family.** A family-level result from one validator could be one operator's setup. Two per
family (where two were eligible and reachable) give a first look at operator-to-operator spread (REPORT §6).

**Rules fixed before the data.** The pre-registration, the design rows and the campaign file are hashed into a mainnet
Memo after the pilot and before the first campaign probe. The confirmatory code is three files whose hashes
`design/campaign.json` records.

**A gate on the method itself.** Before reading any family's key result, G1 checks that the method recovers the
reward/cost key where the scheduler code says it is used (pooled Agave line, and Firedancer). If it does not, the key
results are reported as exploratory.

## Timing

All times are on the study machine's monotonic clock, converted once to Unix milliseconds for the logs.

- **Slot clock** (`crates/itb/src/clock.rs`). Two public WebSocket endpoints send `slotsUpdatesSubscribe` events of type
  `firstShredReceived`, stamped with the RPC node's clock (endpoints in `clock.rs`, `DEFAULT_WS`). The observed first
  shred of a slot is the earliest over the two sources of node time + min(0, receive time − node time). A connection
  silent for 3 s is replaced.
- **Anchor.** For τ ≥ 20 ms the probe fires relative to the observed first shred of the slot before the turn. For
  earlier τ (−300, −150, 0) that event has not happened yet, so the fire time is predicted from the latest observed slot
  and the median slot length; the error of that prediction is measured afterwards from the same stream (`timing.py`,
  results/timing.json).
- **Preparation.** A quad is decided and its transactions signed 900 ms before its fire time (`run.rs`, `PREP_MS`); with
  less than 350 ms left it waits for the target's next turn. The blockhash used is at most 10 s old.
- **Connection.** One QUIC connection per target to its advertised `tpuQuic` (ALPN `solana-tpu`, self-signed ed25519
  certificate, no stake). Shortly before the fire time (2 × the measured handshake + 20 ms, between 30 and 150 ms) the
  connection is checked and re-established if the server dropped it. One unidirectional stream per transaction, opened
  before the fire time, so the write time is the send time.
- **Gap.** The second member is written Δt after the first by spin-waiting on the monotonic clock; the control pair's
  first write follows the main pair's by 0.5 ms. The achieved gaps are logged per stream (`streams.jsonl`).
- **Skip rule.** A quad whose first write would leave more than 100 ms after plan is not sent and not charged. At most
  two quads are in flight at once.

Gate G2 checks the timing on the mainnet dry runs and on the campaign itself (PREREGISTRATION §5).

## The six things the brief asks to account for

| Point | How this design handles it | What it does not separate |
|---|---|---|
| Leader schedule and leader types | Turns from `getLeaderSchedule` per target identity; the leader of every block is re-read from its Fee reward (`getBlock`, rewards on). Client family from the gossip `clientId`, read every 60 s during a run; quads where it differs from the frozen family are excluded and reported (sensitivity b). Ingress kind from gossip: `shared` when another staked node advertises the same TPU host; otherwise `direct` (TPU on the node's own gossip host) or `direct_other_host`. | Operator configuration that gossip does not show (block-engine settings, relayers, custom schedulers). |
| Path from my machine to the executing node | My own QUIC client straight to each target's advertised TPU address, unstaked, no RPC forwarding, no retries. Send times, handshake times and RTT are logged per stream. Landings with the preceding leader are measured, not discarded (S3). | Arrival time at the leader (I know the send time and the RTT). Anything a shared ingress does before the leader sees the transaction. Stake-weighted QoS. |
| Interaction of my own transactions | Each pair writes only its own payers (two for R and C pairs; R_IND members each write only their own); the two pairs of a quad share nothing. Eight payers in two pools of four, so the two quads that can be in flight never share a payer; the payer permutation is randomised per quad. | Processing order of streams inside the receiving node below about 1 ms. |
| Competition inside the block | I measure the relative order of my two members, which foreign traffic shifts equally. Every block of every probed turn is archived with each transaction's signature prefix, fee, cost units and requested compute budget; the number of foreign transactions between my members, the block load and how sorted the whole block is are reported (REPORT §4, §6). Block load is a pre-registered moderator (P3). | Foreign senders' arrival times and the leader's pending set. |
| Send order | Controlled: which member goes first and the gap are randomised per pair, enforced by spin-wait and logged as achieved. | Arrival order at the leader when Δt is below the network jitter (Δt = 0 and 2 ms are reported as such). |
| Noise vs rule | An equal-key control in every quad; candidate rules that disagree by design; pre-registered tests with Holm correction; "insufficient data" below 15 pairs; an equivalence test for "no load effect"; a power study of exactly this analysis on this design ([POWER.md](POWER.md)). | Effects smaller than the design can detect (POWER.md gives the power per test). |

Two more conditions from the brief: every probe is after slot 459,653,008 (the first slot after the Alpenglow
switch-over, `ALPENGLOW_FIRST_SLOT` in `crates/itb/src/auth.rs`, checked before any send), and no probe carries a Jito
tip or goes through a bundle endpoint.

## Money and keys

- **Dry by default.** `itb dry-run` builds, signs and times everything but writes the bytes to a log instead of a
  socket. Only `itb live --live` can send, and only through a `LiveAuthorization` (`crates/itb/src/auth.rs`): mainnet
  genesis hash, current slot past the Alpenglow gate, key present, last reconciliation clean, and the worst-case fee of
  the batch inside both the compiled cap (30,000,000 lamports) and its sub-budget.
- **Reserve before send.** Every probe's worst-case fee is written to a SQLite ledger before its bytes can reach a
  socket. A crash cannot lose a charge; a restart resumes from the ledger.
- **Reconcile.** At every batch boundary the ledger is compared with on-chain balances; an unexplained drop stops the
  programme for good.
- **Sweep.** After the last session and after every blockhash has expired, everything left goes to the one address
  compiled into the binary (`MAINNET_SWEEP_TO`).
- **Keys.** The funder key is read from a path given in the environment and never enters the repository; the payer keys
  live next to it. `scripts/keyguard.py` (pre-commit and CI) refuses any file holding a keypair, and gitleaks scans every
  commit in CI.

## What this design cannot separate

The short list is in PREREGISTRATION §8; the full list, with what would separate each item, is in REPORT §9.

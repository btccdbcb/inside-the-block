# Report

What I measured, how, what came out, and what went wrong. The rules were frozen before the campaign
([PREREGISTRATION.md](docs/PREREGISTRATION.md)); the design reasoning is in [DESIGN.md](docs/DESIGN.md); the data
layout is in [DATA.md](docs/DATA.md). Every table below is generated from the committed data by `docker compose up`
(`analysis/itb_analysis/report.py`), which also checks that the tables here still match the data. Each table names the
file it comes from.

Confirmatory results (pre-registered tests, Holm-adjusted) are in §2–§5 and are labelled as such. Everything else is
exploratory or descriptive and says so.

<!-- TODO(data): §0 summary, 5-7 numbered findings with n, estimate, 95 % CI and the test id. -->

## 1. Measurement log

Every run of the sender, dry or live. Times are UTC; slots are mainnet slots. A dry run builds, signs and times every
probe but sends nothing.

<!-- table:runs -->
*Source: data/raw/*/ and data/dry_runs/*/ (quads.jsonl, run.json, outcomes.jsonl).*

| run | kind | UTC, first → last quad | slot0 range | targets | quads fired / attempted | probes | landed | fees (lamports) | sender commit |
|---|---|---|---|---|---|---|---|---|---|
| 20261029T113453109-dry | dry run | 2026-10-29 11:36 → 16:31 | 462866396–462949080 | 12 | 153/194 | — | — | — | b7121c4 |
<!-- /table:runs -->

The pre-registration Memo, checked against the committed files and the first campaign probe:

<!-- table:memo -->
*Source: data/live/memo_check.json (`itb-analysis memo-check`).*

|  |  |
|---|---|
| no data |  |
<!-- /table:memo -->

<!-- TODO(data): first probed slot vs 459,653,008; client changes seen in gossip during the runs. -->

## 2. Gates

The gates check the method before any claim is read (PREREGISTRATION §5, "Gates").

**G2, timing.** (a) share of quads whose first send left within 50 ms of plan; (b) for quads planned before the turn,
share whose send preceded the first shred of the turn's first slot. The predicted-anchor error is how far the predicted
first shred of the slot before the turn was from the observed one, for the τ levels that fire before that event.

<!-- table:timing -->
*Source: results/timing.json (timing.py).*

| runs | quads fired | (a) sent within 50 ms of plan | G2a | (b) pre-turn sends before slot 0 | G2b | predicted anchor, absolute error p50 / p90 (ms) |
|---|---|---|---|---|---|---|
| dry run | 153 | 1.000 (153) | pass | 1.000 (88) | pass | 2.0 / 13.0 |
<!-- /table:timing -->

**G1, positive controls, and G3, selection.**

<!-- table:gates -->
*Source: results/results.json → G1, G3.*

| gate |  |  | result |
|---|---|---|---|
| no data |  |  |  |
<!-- /table:gates -->

<!-- TODO(data): G4 count of pairs excluded for attribution; one sentence per failed gate and what it downgrades. -->

## 3. Verdicts

The verdict words are applied mechanically to the Holm-adjusted p-values (PREREGISTRATION §5); "insufficient data"
below 15 eligible pairs (15 quads for P2 and S3).

<!-- table:verdicts -->
*Source: results/results.json → verdicts (PREREGISTRATION §5 wording).*

| family | P1 | P2 | S1 | S2-C1 | S2-C2 | S2-C3 | S3 |
|---|---|---|---|---|---|---|---|
| no data |  |  |  |  |  |  |  |
<!-- /table:verdicts -->

### Primary tests

<!-- table:primary -->
*Source: results/results.json.*

| test |
|---|
| no data |
<!-- /table:primary -->

### Secondary tests

<!-- table:secondary -->
*Source: results/results.json.*

| test |
|---|
| no data |
<!-- /table:secondary -->

### Sensitivity analyses

(a) pairs excluded only for timing tolerance, kept and coded by their achieved values; (b) pairs excluded only for the
client rule, kept; (c) censored coding, where a pair with one member in the turn counts as that member first.

<!-- table:sensitivity -->
*Source: results/results.json → sensitivity (PREREGISTRATION §4 a, b, c).*

| analysis | family | F | P(HI first) pre-turn |
|---|---|---|---|
| no data |  |  |  |
<!-- /table:sensitivity -->

## 4. Position in the leader's turn

<!-- TODO(data): turn-map figure (figures/turn-map-*.svg) and reading of P2/S3 per family. -->

Where in their block my in-turn probes sat (exploratory):

<!-- table:insertion -->
*Source: results/explore.json → insertion (exploratory).*

| family | slot of turn | probes | position in block (0 first, 1 last), median [IQR] |
|---|---|---|---|
| no data |  |  |  |
<!-- /table:insertion -->

## 5. Load

P3 is the pre-registered load test; the per-family slopes are descriptive.

<!-- table:load -->
*Source: results/results.json.*

| test |
|---|
| no data |
<!-- /table:load -->

Every archived block of every probed turn, all transactions (exploratory). Sortedness is Kendall's τ between position
and key, signed so that +1 means the highest key first. The approximate priority leaves out instruction-data cost and
skips transactions without a CU limit (`analysis/itb_analysis/explore.py`).

<!-- table:whole_block -->
*Source: results/explore.json → whole_block (exploratory; +1 = sorted high to low).*

| family | blocks | sortedness by CU price, median [IQR] | by approx. priority, median [IQR] | transactions per block (median) | cost units per block (median) |
|---|---|---|---|---|---|
| no data |  |  |  |  |  |
<!-- /table:whole_block -->

Landing in the target's turn by session (exploratory):

<!-- table:hour -->
*Source: results/explore.json → hour (exploratory).*

| family | session | probes in the target's turn |
|---|---|---|
| no data |  |  |
<!-- /table:hour -->

## 6. Batch, queue or arrival order

Race-reversal rate: P(the higher key lands first | the lower key was sent Δt ms earlier), mid-turn, eligible R pairs
(descriptive).

<!-- table:rrr -->
*Source: results/results.json → RRR.*

| family |
|---|
| no data |
<!-- /table:rrr -->

Four models fitted to these rates by maximum likelihood (exploratory, `explore.py`): arrival order with Gaussian
jitter s; a queue drained continuously, where the higher key wins with probability a·exp(−Δt/h); batches of length B at
a random phase, a·max(0, 1 − Δt/B); and a constant a. The weight is the Akaike weight. The interval is a parametric
bootstrap of the best model (200 replicates). Recovery is how often that model has the lowest AIC on data simulated
from its own fit at the same gaps and counts: it says whether this many pairs can tell the models apart at all.

<!-- table:mechanism -->
*Source: results/explore.json → mechanism (exploratory).*

| family | pairs | lowest AIC | arrival: weight (s ms) | drain: weight (a, h ms) | batch: weight (a, B ms) | constant: weight (a) | best model: 95 % bootstrap CI | recovery of that model at this n |
|---|---|---|---|---|---|---|---|---|
| no data |  |  |  |  |  |  |  |  |
<!-- /table:mechanism -->

## 7. Clients and operators

Per validator (descriptive):

<!-- table:per_target -->
*Source: results/results.json → descriptive.per_target.*

| target | family | quads | probes in the target's turn | eligible pairs | F | R: HI first |
|---|---|---|---|---|---|---|
| no data |  |  |  |  |  |  |
<!-- /table:per_target -->

Families with two validators: intraclass correlation of in-turn landing and of F across the two (ANOVA estimator) and
a χ² test of equal in-turn landing (exploratory):

<!-- table:operators -->
*Source: results/explore.json → operators (exploratory).*

| family | target | in the target's turn | F | ICC in-turn | ICC F | χ² p (equal in-turn landing) |
|---|---|---|---|---|---|---|
| no data |  |  |  |  |  |  |
<!-- /table:operators -->

Transactions between the two members of a pair that landed in one block (exploratory):

<!-- table:between -->
*Source: results/explore.json → between (exploratory).*

| family | pairs in one block | adjacent | transactions between (median) | p90 |
|---|---|---|---|---|
| no data |  |  |  |  |
<!-- /table:between -->

The v1 arm (exploratory, PREREGISTRATION §3 rule 2):

<!-- table:v1 -->
*Source: results/results.json → descriptive (v1 arm).*

|  | V_EQ: v1 member first (equal keys) | V_HI: v1 high-key member first |
|---|---|---|
| no data |  |  |
<!-- /table:v1 -->

## 8. What went wrong

- **Devnet end-to-end test skipped.** The devnet RPC faucet answered `410 The RPC faucet is closed on this cluster`
  (2026-10-29 18:05 UTC); the web faucet needs a browser login. The real-network path was tested by the mainnet dry
  runs instead (handshakes and timing, nothing sent), and the paid smoke and pilot stages.
- **Two of the first targets were unreachable at their turns.** In the first mainnet dry run (2026-10-29 11:36–16:31
  UTC, `data/dry_runs/20261029T113453109-dry`), one JitoLabs target refused every connection near its turn (21 quads,
  `connection_closed`) and another never completed a QUIC handshake (14 quads, `quic_connect`). The target rule gained
  a reachability condition (a completed handshake in at least one of two runs) and both slots were re-selected before
  the freeze.
- **Local validator in Docker.** Agave 4.3.2 asserts that io_uring is available at start-up; Docker's default seccomp
  profile blocks it, so the first CI run of the localnet test failed. The test container now runs with seccomp
  unconfined.

<!-- TODO(data): quantified, dated entries from the campaign: RPC errors, WS drops and predicted anchors, QUIC failures per
target, late skips, probes that never landed by target, client changes, ledger/reconcile alarms, pilot decisions. -->

## 9. Deviations from the pre-registration

<!-- TODO(data): every change after the Memo, with UTC time, reason and whether it touches an estimand. "None" only if
true. -->

## 10. What this study does not separate

- **Validator vs whatever sits in front of it.** For a target behind a shared ingress (a block engine or relayer that
  several validators use), the order in the block may have been set before the validator's scheduler saw the
  transactions. My data show the combined behaviour. Separating them would need the ingress's own logs or a validator I
  operate behind the same ingress.
- **Send time vs arrival time.** I know when my bytes left and the round-trip time, not when the leader received them.
  At Δt = 0 and 2 ms the arrival order is not controlled. A capture at the leader would separate them.
- **Client vs operator.** Each family is seen through at most two validators, so a family result is a result about
  those operators' setups at that time (§7 shows how much the two differ). More validators per family would separate
  them.
- **Keys outside my range.** My probes are one-signature transfers with 1,000–6,000 CU limits and CU prices from 10,000
  to 500,000 micro-lamports (×10 at fee scale s10). Large transactions, high fees, many write locks or contended
  accounts may be handled differently.
- **Loads I did not see.** The load result covers the block loads in §5, not beyond.
- **Stake.** My sender is unstaked. Stake-weighted QoS and staked forwarding paths may change what reaches the
  scheduler and when.
- **The leader's pending set.** I do not see other senders' arrival times, so I cannot say which foreign transactions
  were competing with mine at the moment of scheduling.
- **Batch period vs jitter.** Below a few milliseconds a batch boundary and network jitter look the same; §6 gives the
  recovery rate that bounds what the mechanism fits can say.
- **Time.** Four sessions in one week on the client versions of that week (§1 lists the versions seen in gossip).

## 11. Cost

Every transaction the live programme signed, from its ledger. Fees paid are the on-chain `meta.fee` of landed
transactions.

<!-- table:cost -->
*Source: data/live/ledger.csv (the live service's ledger).*

| sub-budget | kind | transactions | landed | fees paid (lamports) |
|---|---|---|---|---|
| no data |  |  |  |  |
<!-- /table:cost -->

Ledger = chain: three totals that must agree to the lamport, computed from the chain alone for every account the
programme registered ([VERIFY.md](docs/VERIFY.md#the-memo-and-the-ledger)):

<!-- table:ledger_check -->
*Source: data/live/ledger_check.json (`itb-analysis ledger-check`).*

|  | lamports |
|---|---|
| no data |  |
<!-- /table:ledger_check -->

<!-- table:money -->
*Source: data/live/ledger.csv, data/live/ledger_check.json.*

|  |  |
|---|---|
| no data |  |
<!-- /table:money -->

## 12. Reproduce and verify

```
docker compose up                       # offline (no network): recomputes every table, figure and number, prints "0 differences"
docker compose --profile verify up      # online: re-checks 60 random claims, the Memo and ledger = chain on public RPC
```

What the verifier checks is in [VERIFY.md](docs/VERIFY.md). The full offline verification of every landing claim:

<!-- table:verify -->
*Source: results/verify.json (`itb-analysis verify`, docs/VERIFY.md).*

|  |  |
|---|---|
| claims checked offline | 0 |
| failures | 0 |
| pair orders recomputed | — |
| result | PASS |
<!-- /table:verify -->

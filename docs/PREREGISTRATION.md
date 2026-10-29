# Pre-registration

This file, `design/design.jsonl` and `design/campaign.json` are frozen before the first mainnet probe. The live programme
writes their SHA-256 hashes into a mainnet Memo transaction after the pilot and before the first campaign probe
(`campaign.json` carries this file's hash, so the Memo pins all three). Anyone can check the order on chain: the Memo's
slot is lower than the slot of every campaign probe. Any change after the freeze is a deviation and is listed in
REPORT.md §Deviations with its reason.

The analysis code is the code in `analysis/` at the frozen commit; `campaign.json` records the git tree hash of that
directory. Every number in README.md and REPORT.md comes from `docker compose up`, which reruns this analysis on the
committed data.

## 1. Question

When a Solana leader receives two transactions that write the same account, which one goes first in its block, and how
does that depend on the client the leader runs, on when during the hand-over the transactions arrive, on the gap
between them, and on how full the block is?

The study sends pairs of small, cheap transactions of my own (probes) straight to the leaders' TPU ports at planned
moments of their turns and reads the resulting order back from the blocks.

## 2. Design

### Targets

One fixed rule picks the targets, per client family (`scripts/select_targets.py`, run at the freeze; the list and the
epoch are in `design/targets.json`). A validator is eligible if gossip reports the family's client id, it has stake,
it advertises a QUIC TPU address, it is not on the exclusion list, and it completed a QUIC handshake from the study
machine in at least one of two runs. Within a family, eligible validators are ranked by stake. The top two become
primaries, preferring a TPU host not already used in the family, and the next ones become alternates.

| Slot | Family | Role |
|---|---|---|
| T1 | Agave | core (only one eligible high-turn node) |
| T2, T3 | JitoLabs | core |
| T4, T5 | AgaveBam | core |
| T6, T7 | HarmonicAgave | core |
| T8, T9 | HarmonicFiredancer | core |
| T10, T11 | Firedancer | core |
| T12 | FireBAM | core, low turn count |
| T13 | Rakurai | core, low turn count |
| X1 | Raiku | extended, descriptive only |

### The unit: a quad

Each planned row of `design.jsonl` is one quad: four probes sent in one leader turn of one target.

- **Main pair**: one of R (a high key against a low key, both writing the same two accounts), C1, C2, C3 (key-identity
  contrasts: pairs where reward/cost, fee and CU price disagree), or R_IND (the same keys, but independent accounts).
- **Control pair**: EQ (two probes with identical keys), or for the v1 arm V_EQ (matched legacy vs v1) or V_HI (v1 high
  key against legacy low key).

Probe library (fee scale s1; s10 multiplies every CU price by 10):

| Probe | CU limit | CU price (µlamports) | Use |
|---|---|---|---|
| A | 1,000 | 100,000 | C1, EQ |
| B | 6,000 | 25,000 | C1 |
| HI | 1,000 | 300,000 | R, R_IND, V_HI |
| LO | 1,200 | 80,000 | R, R_IND, V_HI |
| P | 1,000 | 200,000 | C2 |
| Q | 4,000 | 190,000 | C2 |
| X | 1,000 | 10,000 | C3 |
| Z | 1,200 | 500,000 | C3 |
| A1, HI1 | 1,003 | 100 / 300 lamports total priority fee | v1 versions of A and HI, same requested cost and reward |

Each legacy probe sets its compute limit and price and moves 1 lamport from its payer to the other member's payer, so
both members of a pair write the same two accounts. The key the Agave scheduler uses is
`priority = reward × 1,000,000 / (cost + 1)`. Here reward is the priority fee plus the unburned half of the signature
fee, and cost is the requested cost: 720 + 300 per write lock + instruction-data bytes / 4 + CU limit + 16,384 for the
loaded-data limit. The expected on-chain `costUnits` are 1,784 for a pair member, 1,484 for R_IND and 1,481 for v1.

Randomised per quad (seeded, in `design.jsonl`):

- the send offset τ: −300, −150, 0, +100, +200, +300, +400, +550, +700 or +900 ms after the observed first shred of the
  slot before the target's first slot, balanced within target × session;
- the main type (R 50 %, C1 15 %, C2 15 %, C3 10 %, R_IND 10 %);
- the gap Δt between the members: 0, 2, 10, 25, 50 or 100 ms for R and R_IND; 0 or 1 ms for C pairs; 0 or 10 ms for
  controls;
- which member is sent first;
- the payer permutation.

### Sessions and allocation

Four sessions, scheduled by the programme at the next occurrence of each window after the start, ending no later than
2026-11-03 23:59 UTC:

- A: weekend day, 10–14 UTC;
- B: weekday busy, 14–18 UTC;
- C: night, 00–05 UTC;
- D: weekday 14–17 UTC (replication; unfired quads from A–C carry over here).

A target gets min(base, ⌊0.6 × its expected turns in the window⌋) rows per session, with base 23/23/19/12 for A/B/C/D
(4/4/4/3 for the extended target). This gives 77 campaign rows for each of T2–T11, 28 for T1, 24 for T12, 17 for T13 and
15 for X1. Smoke (4 rows) and pilot (3 per core target + 1 extended) come first.

## 3. Pilot rules (coded in the programme, applied before the campaign)

The pilot fires its quads, waits for every probe to settle, then applies these rules and writes `pilot_decision.json`.
The Memo includes that file's hash.

1. **Fee scale.** Use s10 for the campaign if the s1 legacy probes land in the target's turn at under 70 % of the s10
   rate; otherwise s1.
2. **v1 arm (exploratory).** Keep it if v1 probes land in-turn on at least 6 targets and at at least 50 % of the legacy
   in-turn rate; otherwise every v1 control becomes EQ.
3. **Alternates.** A target whose in-turn landing rate is below 50 % is replaced by its alternate if the alternate
   answers a QUIC handshake; otherwise it stays.
4. **Stop checks.** The programme stops before the campaign, and stops at any later batch boundary, if any of these
   fails:
   - every landed pilot probe has fee = formula and no error;
   - the ledger equals the chain for every owned account;
   - no balance drop is unexplained;
   - at least one pilot probe landed in a target turn.

## 4. Outcomes

Landing is read from blocks (`getBlock`, transaction version 1, rewards), never from status calls alone. The collector
archives:

- the target's turn (slot0 … slot0+3) in full;
- the preceding leader's turn (slot0−4 … slot0−1) as signature prefixes;
- any other block a probe landed in.

A completeness check fails loudly if a probe sits in an archived block without a matching outcome.

Per probe:

- `in_target_turn`: landed in slot0 … slot0+3 in a block whose Fee-reward recipient is the target;
- `landed_with` ∈ {target, preceding, other};
- slot of the turn;
- index in the block;
- the block's total cost units.

Per pair landed together in the target's turn:

- `pref_first`: the preferred member is at the lower (slot, index). The preferred member is HI for R, and the
  reward/cost-preferred member for C pairs (A, Q, Z).
- `first_sent_first`: the member whose QUIC stream opened first lands first.

Timing comes from the programme's own clock frame. The observed first shred of a slot is the earliest over sources of
node timestamp + min(0, receive − node).

- τ achieved = first send − observed first shred of slot0−1.
- off0 = first send − observed first shred of slot0.
- Turn bins by off0:
  - pre: < −100 ms;
  - handover: −100 to +50 ms;
  - s0: until slot 1 starts;
  - s1;
  - s23.

### Eligibility (a pair counts only if all hold)

1. Both members landed in the target's turn (G4: Fee-reward recipient = target).
2. Neither member has an error, and both fees equal the formula.
3. The target's gossip client id is the same in the snapshots before and after the quad.
4. |τ achieved − τ planned| ≤ 100 ms and |Δt achieved − Δt planned| ≤ 1 ms + 10 % of Δt planned.

Sensitivity analyses (reported, never replacing the confirmatory ones):

- out-of-tolerance pairs coded by achieved values;
- pairs with a client transition included;
- censored coding (one member landed counts as that member first).

## 5. Hypotheses and tests

α = 0.05 throughout. Proportions get exact Clopper–Pearson 95 % intervals. Each quad contributes at most one pair per
pair kind (main, control), so pairs are independent units within a cell.

### Primary family (17 tests, Holm)

**P1 — equal-key order, per family (8 tests).** On eligible EQ pairs, F = P(first-sent member lands first). The test is
an exact two-sided binomial against 0.5. Predictions: F > 0.5 for Agave and JitoLabs (arrival order breaks ties),
F < 0.5 for Firedancer (the later arrival goes first among equals), none for the others. Reported for each family with
its interval, and by gap (0 / 10 ms) and turn bin.

**P2 — landing vs send offset, per family (8 tests).** P(probe lands in the target's turn) across the ten τ levels. The
test is a randomisation test of homogeneity: the Pearson χ² over τ levels, with τ labels permuted across quads within
target (10,000 permutations, seed 0), which is how τ was assigned. The estimand is the profile: the in-turn rate per τ
level with intervals, and the slot-of-turn distribution per τ.

**P3 — block load (1 test, pooled over the 8 core families).** On eligible R pairs, I fit a logistic regression of
`pref_first` on block load (per 10M cost units), with family, turn bin and log(Δt + 1) as covariates.

- "Load effect": the two-sided Wald p survives Holm.
- "No load effect": the 90 % interval of the slope lies inside ±0.4 logit per 10M CU, about ±0.10 in probability near
  0.5 (TOST).
- "Inconclusive": neither.

Per-family slopes are reported as descriptive.

### Secondary family (40 tests, Holm separately)

- **S1 — the backlog is sorted, per family (8).** On eligible R pairs sent in the pre bin, P(HI first) is tested with a
  one-sided exact binomial against 0.75.
- **S2 — key identity, per family × contrast (24).** For C1, C2 and C3, P(reward/cost-preferred member first) is tested
  with a one-sided exact binomial against 0.5, pooled over bins (Δt is 0–1 ms). Each candidate key predicts a different
  winner (C1, C2, C3):
  - reward/cost: (A, Q, Z);
  - CU price: (A, P, Z);
  - total fee: (B, Q, Z);
  - arrival order: no preference.

  C1 separates reward/cost from fee, C2 separates it from CU price, and C3 (every key prefers Z) checks that ordering by
  key happens at all.
- **S3 — leakage to the preceding leader vs τ, per family (8).** The same randomisation test as P2, on `landed_with`
  = preceding.

### Descriptive (no confirmatory claim)

- Race-reversal rate RRR(Δt) = P(HI lands first | LO sent Δt earlier), mid-turn bins, per family.
- F by gap and turn bin.
- Slot-of-turn profiles.
- Per-family load slopes.
- Per-validator tables.
- R_IND (independent accounts).
- The v1 arm.
- The extended target (Raiku).
- Batch/drain mechanism fits.
- Hour of day.
- The passive block layer.

Families with fewer than 15 eligible pairs in a cell report the cell as "insufficient data" rather than a verdict.

### Gates

A failed gate is reported and downgrades the claims it affects to exploratory; no data is dropped because of it.

- **G1, positive controls.** Pooled over Agave + JitoLabs, and separately for Firedancer, C1 and C2 must each show the
  reward/cost-preferred member first with a one-sided 95 % lower bound above 0.5. If this fails, S2 is exploratory and
  the failure is investigated in REPORT.md.
- **G2, timing (from the mainnet dry run and the campaign itself).** (a) The first send is within ±50 ms of its planned
  time for ≥ 90 % of fired quads. (b) For quads planned pre-turn (τ ≤ +200 ms), the achieved offset against the observed
  first shred of slot0 is negative for ≥ 90 %. If (b) fails, turn-bin results are exploratory.
- **G3, selection.** Within each pair type, landing must not depend on the key label: an exact McNemar test on the pairs
  where exactly one member landed. If p < 0.05, that type's order results are also given as worst-case bounds.
- **G4, attribution.** Every counted pair lies in a block whose Fee-reward recipient is the target. This is enforced by
  eligibility, and the count of excluded pairs is reported.

## 6. Power

`docs/POWER.md` gives the operating characteristics of exactly this analysis at exactly this design: false-positive
rate under a global null, and power under expected and weak effects (`itb-analysis power`, seeded). The low-turn
families (Agave T1, FireBAM, Rakurai) have about a sixth of the data of the others; their primary tests are kept in the
family, and their likely null results are reported as "not enough turns", not as evidence of no rule.

## 7. Budget and stopping

- Compiled cap: 30,000,000 lamports of fees (0.03 SOL), split into sub-budgets: setup 300,000, smoke 100,000, pilot
  1,200,000, campaign 28,400,000.
- Every probe's worst-case fee is reserved in a SQLite ledger before its bytes can be sent. A quad that would exceed a
  sub-budget is not sent.
- The ledger is reconciled against the chain at every batch boundary. An unexplained balance drop stops the programme
  for good.
- After the last session, the programme waits until every probe's blockhash has expired, then sweeps everything left to
  AwBiUwz1qUzkfKA5nAUu4uYEXz1vpKGvLjRAz6KvWtUn.
- The design is fixed: there are no interim looks at ordering outcomes and no optional stopping. A quad is skipped (not
  sent, not charged) if its first send would go out more than 100 ms late, or if the slot before the target's turn was
  not observed in time. Skips are logged with their reason and reported.
- Unfired quads of sessions A–C move to session D. Anything still unfired at the hard end is reported as unfired.

## 8. What this design cannot separate

- Order inside one leader's block reflects the scheduler and everything in front of it: the QUIC ingress, any block
  engine or relayer the validator runs, and other traffic for the same accounts (none expected: the accounts are fresh
  and mine).
- With direct sends I see each family through two validators at most; a family-level claim is a claim about those
  operators' setups at that time.
- Only 1-signature, 2-write-lock transfers are tested; larger or contended transactions may be treated differently.

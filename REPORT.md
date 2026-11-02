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
*Source: data/raw/*/ and data/dry_runs/*/ (quads.jsonl, run.json, outcomes.jsonl); sender commit of live runs: the service start logged in data/live/live.log.*

| run | kind | UTC, first → last quad | slot0 range | targets | quads fired / attempted | probes | landed | fees (lamports) | sender commit |
|---|---|---|---|---|---|---|---|---|---|
| 20261029T113453109-dry | dry run | 2026-10-29 11:36 → 16:31 | 462866396–462949080 | 12 | 153/194 | — | — | — | b7121c4 |
| 20261029T163724083-dry | dry run | 2026-10-29 16:41 → 10-30 05:14 | 462951944–463163148 | 14 | 486/494 | — | — | — | 531f504+uncommitted runner fixes (recheck/reconnect, pre-open) |
| 20261030T154848184-smoke | live | 2026-10-30 15:49 → 16:11 | 463341556–463347876 | 2 | 4/4 | 16 | 16 | 82,384 | e3be1fe |
| 20261030T161201581-pilot | live | 2026-10-30 16:12 → 19:11 | 463348264–463398292 | 14 | 39/39 | 156 | 156 | 950,676 | e3be1fe |
| 20261031T000007392-session-C | live | 2026-10-31 00:01 → 04:59 | 463479204–463562584 | 14 | 215/218 | 860 | 847 | 4,379,666 | e3be1fe |
| 20261031T100006476-session-A | live | 2026-10-31 10:00 → 13:58 | 463647152–463713744 | 14 | 242/245 | 968 | 951 | 4,917,284 | e3be1fe |
| 20261102T140019428-session-B | live | 2026-11-02 14:04 → 17:58 | 464522272–464587748 | 14 | 248/250 | 992 | 988 | 5,113,142 | e3be1fe |
<!-- /table:runs -->

The pre-registration Memo, checked against the committed files and the first campaign probe:

<!-- table:memo -->
*Source: data/live/memo_check.json (`itb-analysis memo-check`, public mainnet RPC).*

|  |  |
|---|---|
| Memo transaction | [39KUjB…zFNd](https://explorer.solana.com/tx/39KUjBQrwzX81CsBmLBmK8vstWpci2zYG5E6yxBrRJFLQZpPB16zMTjmYk2mApsY9nB5xYkTKhLZzGYiYkGjzFNd), slot 463,398,906, 2026-10-30 19:14:03 UTC |
| sha256 of PREREGISTRATION.md | `2bb1b26d53527aed…` = the Memo |
| sha256 of campaign.json | `65d63ef0cf5aafb9…` = the Memo |
| sha256 of design.jsonl | `91f27c8a650ab841…` = the Memo |
| sha256 of pilot_decision | `b3ab5c025404d4b5…` = the Memo |
| first campaign send | 2026-10-31 00:01:01 UTC (quad C-T6-001) |
| first campaign turn | slot 463,479,204 |
| confirmatory code changed since the freeze | none |
| result | PASS |
<!-- /table:memo -->

The first probed turn began at slot <!-- v:n.first_slot|int -->463,341,556<!-- /v -->, after slot 459,653,008 where
the brief's measurement window begins (the sender refuses earlier slots, and so does the analysis).

Gossip views that changed during the runs, or that named a client other than the target's design family. A quad whose
leader's gossip client is not its family, before or after the quad, is left out of the family tests (PREREGISTRATION §3,
eligibility rule 3) and put back in sensitivity analysis (b):

<!-- table:clients -->
*Source: gossip views in data/raw/*/quads.jsonl; design family from results/results.json → descriptive.per_target.*

| target | design family | client (gossip) | version | quads | UTC, first → last quad |
|---|---|---|---|---|---|
| T1 | Agave | Agave | 4.4.0-rc.1 | 14 | 2026-10-30 16:14 → 10-31 12:53 |
| T1 | Agave | Agave | 4.4.0 | 9 | 2026-10-31 13:25 → 11-02 17:45 |
| T2 | JitoLabs | JitoLabs | 4.3.2 | 4 | 2026-10-30 15:55 → 10-30 16:51 |
| T2 | JitoLabs | AgaveBam | 4.4.0 | 66 | 2026-10-30 18:41 → 11-02 17:51 |
| T5 | AgaveBam | AgaveBam | 4.3.2 | 52 | 2026-10-30 17:00 → 11-02 15:11 |
| T5 | AgaveBam | AgaveBam | 4.4.0 | 16 | 2026-11-02 15:22 → 11-02 17:39 |
| T6 | HarmonicAgave | HarmonicAgave | 4.3.2 | 5 | 2026-10-30 15:49 → 10-30 18:36 |
| T6 | HarmonicAgave | HarmonicAgave | 4.4.0 | 65 | 2026-10-31 00:01 → 11-02 17:58 |
<!-- /table:clients -->

## 2. Gates

The gates check the method before any claim is read (PREREGISTRATION §5, "Gates").

**G2, timing.** (a) share of quads whose first send left within 50 ms of plan; (b) for quads planned before the turn,
share whose send preceded the first shred of the turn's first slot. The predicted-anchor error is how far the predicted
first shred of the slot before the turn was from the observed one, for the τ levels that fire before that event.

<!-- table:timing -->
*Source: results/timing.json (timing.py).*

| runs | quads fired | (a) sent within 50 ms of plan | G2a | (b) pre-turn sends before slot 0 | G2b | predicted anchor, absolute error p50 / p90 (ms) |
|---|---|---|---|---|---|---|
| campaign | 748 | 0.993 (748) | pass | 1.000 (373) | pass | 2.0 / 38.2 |
| dry run | 639 | 1.000 (639) | pass | 1.000 (330) | pass | 2.0 / 17.0 |
<!-- /table:timing -->

**G1, positive controls, and G3, selection.**

<!-- table:gates -->
*Source: results/results.json → G1, G3.*

| gate |  |  | result |
|---|---|---|---|
| G1 Agave line | C1 0.75 [0.48, 0.93] (12/16) | C2 0.92 [0.62, 1.00] (11/12) | pass |
| G1 Firedancer | C1 1.00 [0.82, 1.00] (19/19) | C2 1.00 [0.80, 1.00] (17/17) | pass |
| G3 C1 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 C2 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 C3 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 EQ | preferred only in turn: 2 | other only: 0 | McNemar p 0.500 |
| G3 R | preferred only in turn: 0 | other only: 4 | McNemar p 0.125 |
| G3 R_IND | preferred only in turn: 0 | other only: 3 | McNemar p 0.250 |
| G3 V_EQ | preferred only in turn: 1 | other only: 0 | McNemar p 1.000 |
| G3 V_HI | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
<!-- /table:gates -->

<!-- TODO(data): G4 count of pairs excluded for attribution; one sentence per failed gate and what it downgrades. -->

## 3. Verdicts

The verdict words are applied mechanically to the Holm-adjusted p-values (PREREGISTRATION §5); "insufficient data"
below 15 eligible pairs (15 quads for P2 and S3).

<!-- table:verdicts -->
*Source: results/results.json → verdicts (PREREGISTRATION §5 wording).*

| family | P1 | P2 | S1 | S2-C1 | S2-C2 | S2-C3 | S3 |
|---|---|---|---|---|---|---|---|
| Agave | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| JitoLabs | arrival order breaks ties (F > 0.5) | no evidence that landing depends on the send offset | higher key first in the backlog (P(HI first) > 0.5) | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| AgaveBam | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | higher key first in the backlog (P(HI first) > 0.5) | reward/cost-preferred member first | reward/cost-preferred member first | insufficient data | no evidence that leakage depends on the send offset |
| HarmonicAgave | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | no key preference shown in the backlog | reward/cost-preferred member first | reward/cost-preferred member first | insufficient data | no evidence that leakage depends on the send offset |
| HarmonicFiredancer | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | higher key first in the backlog (P(HI first) > 0.5) | reward/cost-preferred member first | reward/cost-preferred member first | insufficient data | no evidence that leakage depends on the send offset |
| Firedancer | latest arrival first (F < 0.5) | no evidence that landing depends on the send offset | higher key first in the backlog (P(HI first) > 0.5) | reward/cost-preferred member first | reward/cost-preferred member first | reward/cost-preferred member first | no evidence that leakage depends on the send offset |
| FireBAM | insufficient data | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| Rakurai | insufficient data | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
<!-- /table:verdicts -->

### Primary tests

<!-- table:primary -->
*Source: results/results.json → P1, P2, P3, holm_primary.*

| family | P1: F (first-sent first), 95 % CI | P1 p | P1 Holm p | P2 quads | P2 p | P2 Holm p |
|---|---|---|---|---|---|---|
| Agave | 0.78 [0.52, 0.94] (14/18) | 0.031 | 0.401 | 23 | 1.000 | 1.000 |
| JitoLabs | 0.73 [0.60, 0.84] (41/56) | < 0.001 | 0.011 | 72 | 0.894 | 1.000 |
| AgaveBam | 0.46 [0.37, 0.56] (51/110) | 0.505 | 1.000 | 136 | 0.025 | 0.366 |
| HarmonicAgave | 0.55 [0.45, 0.65] (59/107) | 0.334 | 1.000 | 138 | 0.024 | 0.366 |
| HarmonicFiredancer | 0.55 [0.45, 0.65] (56/102) | 0.373 | 1.000 | 136 | 0.075 | 0.900 |
| Firedancer | 0.18 [0.11, 0.26] (18/102) | < 0.001 | < 0.001 | 125 | 0.766 | 1.000 |
| FireBAM | 0.78 [0.40, 0.97] (7/9) | 0.180 | 1.000 | 23 | 0.647 | 1.000 |
| Rakurai | 0.50 [0.21, 0.79] (6/12) | 1.000 | 1.000 | 16 | 1.000 | 1.000 |
<!-- /table:primary -->

### Secondary tests

<!-- table:secondary -->
*Source: results/results.json → S1, S2, P2 (leak), holm_secondary.*

| family | S1: P(HI first) pre-turn | S1 Holm p | S2 C1 | S2 C2 | S2 C3 | S3 Holm p |
|---|---|---|---|---|---|---|
| Agave | 0.71 [0.29, 0.96] (5/7) | 1.000 | 0.75 [0.19, 0.99] (3/4); Holm p 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.00 [0.29, 1.00] (3/3); Holm p 1.000 | 1.000 |
| JitoLabs | 0.93 [0.68, 1.00] (14/15) | 0.012 | 0.75 [0.43, 0.95] (9/12); Holm p 1.000 | 0.91 [0.59, 1.00] (10/11); Holm p 0.141 | 1.00 [0.59, 1.00] (7/7); Holm p 0.172 | 1.000 |
| AgaveBam | 0.86 [0.68, 0.96] (25/29) | 0.002 | 1.00 [0.80, 1.00] (17/17); Holm p < 0.001 | 1.00 [0.82, 1.00] (19/19); Holm p < 0.001 | 1.00 [0.77, 1.00] (14/14); Holm p 0.002 | 1.000 |
| HarmonicAgave | 0.80 [0.56, 0.94] (16/20) | 0.141 | 1.00 [0.82, 1.00] (19/19); Holm p < 0.001 | 0.96 [0.78, 1.00] (22/23); Holm p < 0.001 | 1.00 [0.77, 1.00] (14/14); Holm p 0.002 | 1.000 |
| HarmonicFiredancer | 0.88 [0.68, 0.97] (21/24) | 0.004 | 0.94 [0.71, 1.00] (16/17); Holm p 0.004 | 0.89 [0.67, 0.99] (17/19); Holm p 0.009 | 1.00 [0.77, 1.00] (14/14); Holm p 0.002 | 0.895 |
| Firedancer | 0.96 [0.80, 1.00] (25/26) | < 0.001 | 1.00 [0.82, 1.00] (19/19); Holm p < 0.001 | 1.00 [0.80, 1.00] (17/17); Holm p < 0.001 | 1.00 [0.79, 1.00] (16/16); Holm p < 0.001 | 1.000 |
| FireBAM | 1.00 [0.16, 1.00] (2/2) | 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.000 |
| Rakurai | 1.00 [0.16, 1.00] (2/2) | 1.000 | 1.00 [0.29, 1.00] (3/3); Holm p 1.000 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.000 |
<!-- /table:secondary -->

### Sensitivity analyses

(a) pairs excluded only for timing tolerance, kept and coded by their achieved values; (b) pairs excluded only for the
client rule, kept; (c) censored coding, where a pair with one member in the turn counts as that member first.

<!-- table:sensitivity -->
*Source: results/results.json → sensitivity (PREREGISTRATION §4 a, b, c).*

| analysis | family | F | P(HI first) pre-turn |
|---|---|---|---|
| a_timing_by_achieved | Agave | 0.78 [0.52, 0.94] (14/18) | 0.71 [0.29, 0.96] (5/7) |
| a_timing_by_achieved | JitoLabs | 0.69 [0.56, 0.81] (41/59) | 0.94 [0.70, 1.00] (15/16) |
| a_timing_by_achieved | AgaveBam | 0.46 [0.36, 0.56] (51/111) | 0.86 [0.68, 0.96] (25/29) |
| a_timing_by_achieved | HarmonicAgave | 0.55 [0.45, 0.65] (60/109) | 0.82 [0.60, 0.95] (18/22) |
| a_timing_by_achieved | HarmonicFiredancer | 0.55 [0.45, 0.65] (58/105) | 0.88 [0.69, 0.97] (22/25) |
| a_timing_by_achieved | Firedancer | 0.17 [0.11, 0.26] (18/103) | 0.96 [0.81, 1.00] (26/27) |
| a_timing_by_achieved | FireBAM | 0.69 [0.39, 0.91] (9/13) | 1.00 [0.29, 1.00] (3/3) |
| a_timing_by_achieved | Rakurai | 0.50 [0.21, 0.79] (6/12) | 1.00 [0.16, 1.00] (2/2) |
| a_timing_by_achieved | Raiku | 0.64 [0.31, 0.89] (7/11) | 1.00 [0.40, 1.00] (4/4) |
| b_client_included | Agave | 0.78 [0.52, 0.94] (14/18) | 0.71 [0.29, 0.96] (5/7) |
| b_client_included | JitoLabs | 0.62 [0.53, 0.71] (68/109) | 0.97 [0.83, 1.00] (30/31) |
| b_client_included | AgaveBam | 0.46 [0.37, 0.56] (51/110) | 0.86 [0.68, 0.96] (25/29) |
| b_client_included | HarmonicAgave | 0.55 [0.45, 0.65] (59/107) | 0.80 [0.56, 0.94] (16/20) |
| b_client_included | HarmonicFiredancer | 0.55 [0.45, 0.65] (56/102) | 0.88 [0.68, 0.97] (21/24) |
| b_client_included | Firedancer | 0.18 [0.11, 0.26] (18/102) | 0.96 [0.80, 1.00] (25/26) |
| b_client_included | FireBAM | 0.78 [0.40, 0.97] (7/9) | 1.00 [0.16, 1.00] (2/2) |
| b_client_included | Rakurai | 0.50 [0.21, 0.79] (6/12) | 1.00 [0.16, 1.00] (2/2) |
| b_client_included | Raiku | 0.70 [0.35, 0.93] (7/10) | 1.00 [0.40, 1.00] (4/4) |
| c_censored | Agave | 0.78 [0.52, 0.94] (14/18) | 0.71 [0.29, 0.96] (5/7) |
| c_censored | JitoLabs | 0.73 [0.60, 0.84] (41/56) | 0.93 [0.68, 1.00] (14/15) |
| c_censored | AgaveBam | 0.46 [0.37, 0.56] (51/110) | 0.86 [0.68, 0.96] (25/29) |
| c_censored | HarmonicAgave | 0.55 [0.45, 0.65] (59/107) | 0.80 [0.56, 0.94] (16/20) |
| c_censored | HarmonicFiredancer | 0.56 [0.46, 0.66] (58/104) | 0.88 [0.68, 0.97] (21/24) |
| c_censored | Firedancer | 0.18 [0.11, 0.26] (18/102) | 0.96 [0.80, 1.00] (25/26) |
| c_censored | FireBAM | 0.78 [0.40, 0.97] (7/9) | 1.00 [0.16, 1.00] (2/2) |
| c_censored | Rakurai | 0.50 [0.21, 0.79] (6/12) | 1.00 [0.16, 1.00] (2/2) |
| c_censored | Raiku | 0.70 [0.35, 0.93] (7/10) | 1.00 [0.40, 1.00] (4/4) |
<!-- /table:sensitivity -->

## 4. Position in the leader's turn

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="figures/turn-map-dark.svg">
  <img alt="Per client family, the share of probes that landed in the target's turn and the share that landed with the preceding leader, against the send offset" src="figures/turn-map-light.svg">
</picture>

<!-- TODO(data): reading of P2/S3 per family (after session D). -->

Where in their block my in-turn probes sat (exploratory):

<!-- table:insertion -->
*Source: results/explore.json → insertion (exploratory).*

| family | slot of turn | probes | position in block (0 first, 1 last), median [IQR] |
|---|---|---|---|
| Agave | 0 | 54 | 0.19 [0.12, 0.26] |
| Agave | 1 | 17 | 0.41 [0.05, 0.43] |
| Agave | 2 | 12 | 0.56 [0.20, 0.61] |
| Agave | 3 | 9 | 0.61 [0.60, 0.61] |
| JitoLabs | 0 | 345 | 0.12 [0.08, 0.19] |
| JitoLabs | 1 | 81 | 0.52 [0.09, 0.91] |
| JitoLabs | 2 | 67 | 0.59 [0.41, 0.64] |
| JitoLabs | 3 | 58 | 0.62 [0.50, 0.67] |
| AgaveBam | 0 | 312 | 0.15 [0.12, 0.28] |
| AgaveBam | 1 | 86 | 0.42 [0.22, 0.64] |
| AgaveBam | 2 | 74 | 0.69 [0.21, 0.88] |
| AgaveBam | 3 | 52 | 0.61 [0.47, 0.87] |
| HarmonicAgave | 0 | 273 | 0.07 [0.02, 0.39] |
| HarmonicAgave | 1 | 94 | 0.38 [0.03, 0.57] |
| HarmonicAgave | 2 | 71 | 0.36 [0.24, 0.66] |
| HarmonicAgave | 3 | 86 | 0.38 [0.10, 0.90] |
| HarmonicFiredancer | 0 | 263 | 0.07 [0.02, 0.30] |
| HarmonicFiredancer | 1 | 108 | 0.25 [0.05, 0.59] |
| HarmonicFiredancer | 2 | 64 | 0.37 [0.26, 0.54] |
| HarmonicFiredancer | 3 | 81 | 0.50 [0.06, 0.90] |
| Firedancer | 0 | 346 | 0.06 [0.03, 0.48] |
| Firedancer | 1 | 65 | 0.38 [0.31, 0.68] |
| Firedancer | 2 | 79 | 0.49 [0.07, 0.75] |
| Firedancer | 3 | 6 | 0.49 [0.22, 0.51] |
| FireBAM | 0 | 39 | 0.14 [0.09, 0.43] |
| FireBAM | 1 | 13 | 0.40 [0.05, 0.45] |
| FireBAM | 2 | 8 | 0.23 [0.05, 0.24] |
| FireBAM | 3 | 7 | 0.02 [0.02, 0.57] |
| Rakurai | 0 | 40 | 0.10 [0.06, 0.27] |
| Rakurai | 1 | 8 | 0.25 [0.03, 0.47] |
| Rakurai | 2 | 8 | 0.24 [0.22, 0.25] |
| Rakurai | 3 | 8 | 0.10 [0.08, 0.12] |
| Raiku | 0 | 32 | 0.12 [0.05, 0.33] |
| Raiku | 1 | 8 | 0.46 [0.12, 0.81] |
| Raiku | 2 | 4 | 0.60 [0.60, 0.62] |
| Raiku | 3 | 8 | 0.51 [0.50, 0.55] |
<!-- /table:insertion -->

## 5. Load

P3 is the pre-registered load test; the per-family slopes are descriptive.

<!-- table:load -->
*Source: results/results.json → P3 (equivalence margin ±0.4).*

| R pairs | n | β (logit per 10M CU) | 90 % CI | p | Holm p | verdict |
|---|---|---|---|---|---|---|
| pooled (P3) | 305 | -0.036 | [-0.429, 0.357] | 0.880 | 1.000 | inconclusive |
| Agave | 12 | -0.113 | [-1.749, 1.524] | 0.910 |  | descriptive |
| AgaveBam | 68 | -1.039 | [-2.759, 0.682] | 0.321 |  | descriptive |
| FireBAM | 7 | — | [—, —] | — |  | descriptive |
| Firedancer | 60 | 1.345 | [-4.201, 6.890] | 0.690 |  | descriptive |
| HarmonicAgave | 54 | 0.372 | [-0.432, 1.176] | 0.447 |  | descriptive |
| HarmonicFiredancer | 65 | -0.860 | [-1.933, 0.213] | 0.187 |  | descriptive |
| JitoLabs | 33 | -0.723 | [-2.283, 0.836] | 0.445 |  | descriptive |
| Raiku | 7 | — | [—, —] | — |  | descriptive |
| Rakurai | 6 | — | [—, —] | — |  | descriptive |
<!-- /table:load -->

Every archived block of every probed turn, all transactions (exploratory). Sortedness is Kendall's τ between position
and key, signed so that +1 means the highest key first. The approximate priority leaves out instruction-data cost and
skips transactions without a CU limit (`analysis/itb_analysis/explore.py`).

<!-- table:whole_block -->
*Source: results/explore.json → whole_block (exploratory; +1 = sorted high to low).*

| family | blocks | sortedness by CU price, median [IQR] | by approx. priority, median [IQR] | transactions per block (median) | cost units per block (median) |
|---|---|---|---|---|---|
| Agave | 92 | 0.02 [-0.03, 0.08] | 0.02 [-0.03, 0.08] | 330 | 15.4M |
| JitoLabs | 552 | 0.04 [-0.03, 0.12] | 0.06 [-0.00, 0.13] | 292 | 14.3M |
| AgaveBam | 544 | 0.07 [-0.00, 0.15] | 0.08 [0.02, 0.15] | 319 | 13.6M |
| HarmonicAgave | 552 | 0.07 [0.01, 0.12] | 0.08 [0.03, 0.14] | 316 | 13.6M |
| HarmonicFiredancer | 544 | 0.06 [-0.01, 0.12] | 0.07 [0.01, 0.12] | 314 | 13.7M |
| Firedancer | 500 | 0.13 [0.07, 0.19] | 0.11 [0.05, 0.17] | 388 | 17.7M |
| FireBAM | 92 | 0.05 [0.00, 0.14] | 0.08 [0.03, 0.16] | 302 | 15.2M |
| Rakurai | 64 | 0.01 [-0.03, 0.05] | 0.01 [-0.03, 0.07] | 403 | 16.9M |
| Raiku | 52 | 0.01 [-0.04, 0.09] | 0.04 [-0.02, 0.11] | 316 | 15.6M |
<!-- /table:whole_block -->

Landing in the target's turn by session (exploratory):

<!-- table:hour -->
*Source: results/explore.json → hour (exploratory).*

| family | session | probes in the target's turn |
|---|---|---|
| all | A | 0.96 [0.95, 0.97] (930/968) |
| all | B | 0.97 [0.95, 0.98] (959/992) |
| all | C | 0.96 [0.95, 0.98] (829/860) |
| all | pilot | 0.97 [0.94, 0.99] (152/156) |
| all | smoke | 1.00 [0.79, 1.00] (16/16) |
| Agave | A | 1.00 [0.79, 1.00] (16/16) |
| Agave | B | 1.00 [0.89, 1.00] (32/32) |
| Agave | C | 1.00 [0.90, 1.00] (36/36) |
| Agave | pilot | 1.00 [0.63, 1.00] (8/8) |
| JitoLabs | A | 0.99 [0.97, 1.00] (183/184) |
| JitoLabs | B | 1.00 [0.98, 1.00] (184/184) |
| JitoLabs | C | 1.00 [0.98, 1.00] (152/152) |
| JitoLabs | pilot | 1.00 [0.86, 1.00] (24/24) |
| JitoLabs | smoke | 1.00 [0.63, 1.00] (8/8) |
| AgaveBam | A | 0.96 [0.92, 0.98] (176/184) |
| AgaveBam | B | 0.98 [0.95, 0.99] (180/184) |
| AgaveBam | C | 0.95 [0.90, 0.98] (144/152) |
| AgaveBam | pilot | 1.00 [0.86, 1.00] (24/24) |
| HarmonicAgave | A | 0.98 [0.95, 0.99] (180/184) |
| HarmonicAgave | B | 0.94 [0.90, 0.97] (173/184) |
| HarmonicAgave | C | 0.91 [0.86, 0.95] (139/152) |
| HarmonicAgave | pilot | 1.00 [0.86, 1.00] (24/24) |
| HarmonicAgave | smoke | 1.00 [0.63, 1.00] (8/8) |
| HarmonicFiredancer | A | 0.93 [0.88, 0.96] (171/184) |
| HarmonicFiredancer | B | 0.92 [0.88, 0.96] (170/184) |
| HarmonicFiredancer | C | 0.99 [0.96, 1.00] (151/152) |
| HarmonicFiredancer | pilot | 1.00 [0.86, 1.00] (24/24) |
| Firedancer | A | 1.00 [0.98, 1.00] (164/164) |
| Firedancer | B | 0.98 [0.94, 0.99] (164/168) |
| Firedancer | C | 1.00 [0.97, 1.00] (144/144) |
| Firedancer | pilot | 1.00 [0.86, 1.00] (24/24) |
| FireBAM | A | 0.50 [0.29, 0.71] (12/24) |
| FireBAM | B | 1.00 [0.86, 1.00] (24/24) |
| FireBAM | C | 0.72 [0.53, 0.86] (23/32) |
| FireBAM | pilot | 0.67 [0.35, 0.90] (8/12) |
| Rakurai | A | 1.00 [0.74, 1.00] (12/12) |
| Rakurai | B | 1.00 [0.79, 1.00] (16/16) |
| Rakurai | C | 1.00 [0.86, 1.00] (24/24) |
| Rakurai | pilot | 1.00 [0.74, 1.00] (12/12) |
| Raiku | A | 1.00 [0.79, 1.00] (16/16) |
| Raiku | B | 1.00 [0.79, 1.00] (16/16) |
| Raiku | C | 1.00 [0.79, 1.00] (16/16) |
| Raiku | pilot | 1.00 [0.40, 1.00] (4/4) |
<!-- /table:hour -->

## 6. Batch, queue or arrival order

Race-reversal rate: P(the higher key lands first | the lower key was sent Δt ms earlier), mid-turn, eligible R pairs
(descriptive).

<!-- table:rrr -->
*Source: results/results.json → RRR.*

| family | Δt = 0 ms | Δt = 2 ms | Δt = 10 ms | Δt = 25 ms | Δt = 50 ms | Δt = 100 ms |
|---|---|---|---|---|---|---|
| Agave | — | — | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.97] (0/1) | — | 0.00 [0.00, 0.84] (0/2) |
| JitoLabs | 1.00 [0.16, 1.00] (2/2) | 1.00 [0.16, 1.00] (2/2) | 0.00 [0.00, 0.84] (0/2) | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.84] (0/2) | 0.00 [0.00, 0.52] (0/5) |
| AgaveBam | 1.00 [0.03, 1.00] (1/1) | 1.00 [0.63, 1.00] (8/8) | 0.50 [0.07, 0.93] (2/4) | 0.00 [0.00, 0.84] (0/2) | 0.00 [0.00, 0.71] (0/3) | 0.00 [0.00, 0.46] (0/6) |
| HarmonicAgave | 1.00 [0.03, 1.00] (1/1) | 0.75 [0.19, 0.99] (3/4) | 0.40 [0.05, 0.85] (2/5) | 0.57 [0.18, 0.90] (4/7) | 0.00 [0.00, 0.84] (0/2) | 0.00 [0.00, 0.97] (0/1) |
| HarmonicFiredancer | 1.00 [0.03, 1.00] (1/1) | 0.86 [0.42, 1.00] (6/7) | 0.89 [0.52, 1.00] (8/9) | 0.29 [0.04, 0.71] (2/7) | 0.00 [0.00, 0.71] (0/3) | 0.00 [0.00, 0.60] (0/4) |
| Firedancer | 1.00 [0.40, 1.00] (4/4) | 1.00 [0.40, 1.00] (4/4) | 0.60 [0.15, 0.95] (3/5) | 0.00 [0.00, 0.71] (0/3) | 0.00 [0.00, 0.52] (0/5) | 0.00 [0.00, 0.84] (0/2) |
| FireBAM | — | — | 1.00 [0.03, 1.00] (1/1) | 1.00 [0.03, 1.00] (1/1) | 0.00 [0.00, 0.97] (0/1) | — |
| Rakurai | — | — | — | — | 0.00 [0.00, 0.97] (0/1) | — |
| Raiku | — | — | — | 0.00 [0.00, 0.84] (0/2) | — | — |
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
| AgaveBam | 24 | batch | 0.00 (s = 17.89) | 0.30 (a = 1.00, h = 14.41) | 0.70 (B = 25.00, a = 1.00) | 0.00 (a = 0.46) | B [10.00, 36.01]; a [0.86, 1.00] | 0.99 |
| HarmonicAgave | 20 | batch | 0.27 (s = 51.25) | 0.26 (a = 0.80, h = 34.27) | 0.37 (B = 50.00, a = 0.78) | 0.11 (a = 0.50) | B [25.00, 88.65]; a [0.49, 1.00] | 0.66 |
| HarmonicFiredancer | 31 | batch | 0.00 (s = 36.01) | 0.28 (a = 1.00, h = 24.38) | 0.72 (B = 40.26, a = 0.97) | 0.00 (a = 0.55) | B [25.00, 55.54]; a [0.78, 1.00] | 0.90 |
| Firedancer | 23 | batch | 0.00 (s = 16.31) | 0.26 (a = 1.00, h = 13.38) | 0.74 (B = 25.00, a = 1.00) | 0.00 (a = 0.48) | B [12.27, 28.47]; a [0.90, 1.00] | 1.00 |
<!-- /table:mechanism -->

## 7. Clients and operators

Per validator (descriptive):

<!-- table:per_target -->
*Source: results/results.json → descriptive.per_target.*

| target | family | quads | probes in the target's turn | eligible pairs | F | R: HI first |
|---|---|---|---|---|---|---|
| T1 | Agave | 23 | 1.00 [0.96, 1.00] (92/92) | 46 | 0.78 [0.52, 0.94] (14/18) | 0.42 [0.15, 0.72] (5/12) |
| T2 | JitoLabs | 70 | 1.00 [0.99, 1.00] (280/280) | 8 | 0.33 [0.01, 0.91] (1/3) | 0.33 [0.01, 0.91] (1/3) |
| T3 | JitoLabs | 68 | 1.00 [0.98, 1.00] (271/272) | 129 | 0.75 [0.62, 0.86] (40/53) | 0.63 [0.44, 0.80] (19/30) |
| T4 | AgaveBam | 68 | 0.99 [0.96, 1.00] (268/272) | 134 | 0.45 [0.32, 0.59] (25/55) | 0.60 [0.42, 0.76] (21/35) |
| T5 | AgaveBam | 68 | 0.94 [0.91, 0.97] (256/272) | 126 | 0.47 [0.34, 0.61] (26/55) | 0.61 [0.42, 0.77] (20/33) |
| T6 | HarmonicAgave | 70 | 0.96 [0.94, 0.98] (270/280) | 128 | 0.55 [0.40, 0.68] (29/53) | 0.67 [0.46, 0.83] (18/27) |
| T7 | HarmonicAgave | 68 | 0.93 [0.90, 0.96] (254/272) | 126 | 0.56 [0.41, 0.69] (30/54) | 0.59 [0.39, 0.78] (16/27) |
| T8 | HarmonicFiredancer | 68 | 0.92 [0.88, 0.95] (249/272) | 119 | 0.51 [0.36, 0.66] (24/47) | 0.65 [0.45, 0.81] (20/31) |
| T9 | HarmonicFiredancer | 68 | 0.98 [0.96, 0.99] (267/272) | 131 | 0.58 [0.44, 0.71] (32/55) | 0.74 [0.56, 0.87] (25/34) |
| T10 | Firedancer | 68 | 0.99 [0.96, 1.00] (268/272) | 134 | 0.24 [0.13, 0.38] (13/54) | 0.76 [0.58, 0.89] (25/33) |
| T11 | Firedancer | 57 | 1.00 [0.98, 1.00] (228/228) | 112 | 0.10 [0.03, 0.23] (5/48) | 0.63 [0.42, 0.81] (17/27) |
| T12 | FireBAM | 23 | 0.73 [0.63, 0.82] (67/92) | 26 | 0.78 [0.40, 0.97] (7/9) | 0.86 [0.42, 1.00] (6/7) |
| T13 | Rakurai | 16 | 1.00 [0.94, 1.00] (64/64) | 32 | 0.50 [0.21, 0.79] (6/12) | 0.50 [0.12, 0.88] (3/6) |
| X1 | Raiku | 13 | 1.00 [0.93, 1.00] (52/52) | 25 | 0.70 [0.35, 0.93] (7/10) | 0.71 [0.29, 0.96] (5/7) |
<!-- /table:per_target -->

Families with two validators: intraclass correlation of in-turn landing and of F across the two (ANOVA estimator) and
a χ² test of equal in-turn landing (exploratory):

<!-- table:operators -->
*Source: results/explore.json → operators (exploratory).*

| family | target | in the target's turn | F | ICC in-turn | ICC F | χ² p (equal in-turn landing) |
|---|---|---|---|---|---|---|
| AgaveBam | T4 | 0.99 [0.96, 1.00] (268/272) | 0.45 [0.32, 0.59] (25/55) | 0.024 | -0.018 | 0.012 |
| AgaveBam | T5 | 0.94 [0.91, 0.97] (256/272) | 0.47 [0.34, 0.61] (26/55) | 0.024 | -0.018 | 0.012 |
| Firedancer | T10 | 0.99 [0.96, 1.00] (268/272) | 0.24 [0.13, 0.38] (13/54) | 0.010 | 0.043 | 0.182 |
| Firedancer | T11 | 1.00 [0.98, 1.00] (228/228) | 0.10 [0.03, 0.23] (5/48) | 0.010 | 0.043 | 0.182 |
| HarmonicAgave | T6 | 0.96 [0.94, 0.98] (270/280) | 0.55 [0.40, 0.68] (29/53) | 0.006 | -0.019 | 0.151 |
| HarmonicAgave | T7 | 0.93 [0.90, 0.96] (254/272) | 0.56 [0.41, 0.69] (30/54) | 0.006 | -0.019 | 0.151 |
| HarmonicFiredancer | T8 | 0.92 [0.88, 0.95] (249/272) | 0.51 [0.36, 0.66] (24/47) | 0.040 | -0.010 | < 0.001 |
| HarmonicFiredancer | T9 | 0.98 [0.96, 0.99] (267/272) | 0.58 [0.44, 0.71] (32/55) | 0.040 | -0.010 | < 0.001 |
| JitoLabs | T2 | 1.00 [0.99, 1.00] (280/280) | 0.33 [0.01, 0.91] (1/3) | 0.000 | 0.220 | 0.988 |
| JitoLabs | T3 | 1.00 [0.98, 1.00] (271/272) | 0.75 [0.62, 0.86] (40/53) | 0.000 | 0.220 | 0.988 |
<!-- /table:operators -->

Transactions between the two members of a pair that landed in one block (exploratory):

<!-- table:between -->
*Source: results/explore.json → between (exploratory).*

| family | pairs in one block | adjacent | transactions between (median) | p90 |
|---|---|---|---|---|
| Agave | 42 | 0.52 [0.36, 0.68] (22/42) | 0 | 22 |
| AgaveBam | 252 | 0.10 [0.07, 0.14] (25/252) | 3 | 24 |
| FireBAM | 32 | 0.41 [0.24, 0.59] (13/32) | 1 | 15 |
| Firedancer | 243 | 0.02 [0.01, 0.05] (5/243) | 11 | 24 |
| HarmonicAgave | 256 | 0.21 [0.16, 0.26] (53/256) | 4 | 36 |
| HarmonicFiredancer | 253 | 0.18 [0.13, 0.23] (45/253) | 4 | 35 |
| JitoLabs | 263 | 0.34 [0.29, 0.40] (90/263) | 2 | 21 |
| Raiku | 26 | 0.65 [0.44, 0.83] (17/26) | 0 | 19 |
| Rakurai | 32 | 0.62 [0.44, 0.79] (20/32) | 0 | 25 |
<!-- /table:between -->

The v1 arm (exploratory, PREREGISTRATION §3 rule 2):

<!-- table:v1 -->
*Source: results/results.json → descriptive (v1 arm).*

|  | V_EQ: v1 member first (equal keys) | V_HI: v1 high-key member first |
|---|---|---|
| in the target's turn: v1 probes vs legacy probes of the same quads | 0.97 [0.92, 0.99] (127/131) | 0.95 [0.91, 0.98] (168/176) |
| Agave | 0.67 [0.09, 0.99] (2/3) | 1.00 [0.16, 1.00] (2/2) |
| JitoLabs | 0.29 [0.04, 0.71] (2/7) | 1.00 [0.54, 1.00] (6/6) |
| AgaveBam | 0.33 [0.10, 0.65] (4/12) | 1.00 [0.63, 1.00] (8/8) |
| HarmonicAgave | 0.57 [0.29, 0.82] (8/14) | 1.00 [0.59, 1.00] (7/7) |
| HarmonicFiredancer | 0.50 [0.25, 0.75] (8/16) | 0.86 [0.42, 1.00] (6/7) |
| Firedancer | 0.60 [0.32, 0.84] (9/15) | 0.83 [0.36, 1.00] (5/6) |
| FireBAM | 0.00 [0.00, 0.97] (0/1) | 1.00 [0.16, 1.00] (2/2) |
| Rakurai | 0.50 [0.01, 0.99] (1/2) | 1.00 [0.16, 1.00] (2/2) |
| Raiku | 0.50 [0.01, 0.99] (1/2) | — |
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

- **Problems the live service logged**, per UTC day:

<!-- table:incidents -->
*Source: data/live/live.log (events of the live service).*

| UTC day | slot feed reconnects after an error | silent slot feed reconnects | RPC calls given up | blockhash fetch errors | gossip fetch errors | turns too close to prepare | ledger mismatches | foreign deposits |
|---|---|---|---|---|---|---|---|---|
| 2026-10-30 | 3 | 821 | 0 | 0 | 0 | 0 | 0 | 0 |
| 2026-10-31 | 83 | 1349 | 3 | 2 | 1 | 6 | 0 | 1 |
| 2026-11-01 | 310 | 48 | 0 | 0 | 0 | 0 | 0 | 0 |
| 2026-11-02 | 23 | 317 | 0 | 0 | 0 | 0 | 0 | 0 |
<!-- /table:incidents -->

  The send clock follows two public WebSocket streams of first-shred events and reconnects a stream that fails or is
  silent for 3 s. While neither stream has shown the slot before a turn, the fire time is predicted from the last
  observed slot; G2 (§2) gives how far that prediction was from the observed first shred. A turn closer than the
  minimum preparation time was passed over for a later one, with nothing sent. On 2026-11-01 no session was open, so
  nothing was due and nothing was sent. The single foreign deposit is 11,677 lamports that someone else sent to one of
  my payer accounts on 2026-10-31; the reconcile flagged it, and it is swept with the rest.
- **Quads skipped and probes that did not land**, per run:

<!-- table:losses -->
*Source: data/raw/*/quads.jsonl (status, reason), outcomes.jsonl (landed, landed_with); unfired: the session's stage_done in data/live/live.log.*

| run | quad attempts skipped at fire time | quads unfired at the session's end | probes not landed | landed with the preceding leader | landed with another leader |
|---|---|---|---|---|---|
| 20261030T154848184-smoke | — | — | 0 | 0 | 0 |
| 20261030T161201581-pilot | — | — | 0 | 0 | 4: T12 4 |
| 20261031T000007392-session-C | anchor_not_observed 1, stream_credit 2 | 3 | 13: T5 4, T12 9 | 12: T5 4, T7 8 | 6: T6 1, T7 4, T8 1 |
| 20261031T100006476-session-A | stream_credit 3 | 6 | 17: T3 1, T4 4, T5 4, T12 8 | 16: T8 8, T9 4, T12 4 | 5: T7 4, T8 1 |
| 20261102T140019428-session-B | stale_blockhash 2 | 0 | 4: T5 4 | 16: T6 8, T8 4, T10 4 | 13: T6 1, T7 2, T8 9, T9 1 |
<!-- /table:losses -->

  A skipped attempt sends nothing: `stream_credit`, the leader's QUIC endpoint did not open the four streams in time;
  `anchor_not_observed`, the first shred of the slot before the turn was not seen in time; `stale_blockhash`, the
  cached blockhash was older than the sender's limit when the quad was signed. A skipped quad is tried again on a later
  turn of the same session. Quads still unfired when C and A ended moved to session D, as pre-registered
  (PREREGISTRATION §7). Most probes that did not land were sent to T12 (FireBAM), which also has the lowest in-turn
  rate (§7). The pilot decision counted 152 of 156 pilot probes as landed because it counts landings in the target's
  turn only; the other 4 landed with another leader.
- **A target changed client.** T2, selected as JitoLabs, reported AgaveBam in gossip from the evening of 2026-10-30
  on (§1). Its quads fail the client rule, so the JitoLabs tests rest mostly on T3; sensitivity analysis (b) puts T2's
  pairs back.

## 9. Deviations from the pre-registration

None in the tests, the eligibility rules, the design rows, the targets or the session schedule. `memo-check` hashes
the pre-registration, the design, the campaign file and the three confirmatory analysis files against the Memo and
finds no change (§1). The live service ran one build of the sender, commit e3be1fe, from its start on 2026-10-30 to
the sweep, without a restart (`data/live/live.log` has one `run_live` line). The sessions ran in the windows the
programme scheduled from `design/campaign.json`: C and A on 2026-10-31, B on 2026-11-02, D on 2026-11-03.

Changes after the Memo, none of which touches an estimand (git log of this repository):

| UTC | change | why |
|---|---|---|
| 2026-10-30 20:12 | POWER.md wording ("Produced by") | wording |
| 2026-10-31 03:19 | key guard refuses the sending machine's name, server paths and private addresses | keep machine details out of the published data |
| 2026-11-02 18:20 | `docker compose` rebuilds its images on every run (layer-cached) | an image left from an older checkout gave 5 differences in a rehearsal on a fresh clone |
| 2026-11-02 | report tables: sender commit of live runs, gossip client changes, logged problems, losses per run | REPORT §1 and §8 counts come from the data, not from prose |
| 2026-11-02 and at publication | history after e3be1fe rewritten: two key-guard commits squashed into one, private names dropped from the ignore rules; data, code and results otherwise unchanged | keep private names out of the history; commits up to the deployed sender e3be1fe are unchanged |

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
| campaign | probe | 2844 | 2786 | 14,410,092 |
| pilot | probe | 156 | 156 | 950,676 |
| setup | fund | 3 | 3 | 15,000 |
| setup | memo | 1 | 1 | 5,000 |
| smoke | probe | 16 | 16 | 82,384 |
| total |  | 3020 | 2962 | 15,463,152 (0.015463 SOL) |
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
| fees paid | 15,463,152 lamports (0.015463 SOL) of the 30,000,000 cap |
| ledger = chain | not checked yet |
| sweep | not yet |
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
| claims checked offline | 2992 |
| failures | 0 |
| pair orders recomputed | — |
| result | PASS |
<!-- /table:verify -->

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
| 20261029T163724083-dry | dry run | 2026-10-29 16:41 → 10-30 05:14 | 462951944–463163148 | 14 | 486/494 | — | — | — | 531f504+uncommitted runner fixes (recheck/reconnect, pre-open) |
| 20261030T154848184-smoke | live | 2026-10-30 15:49 → 16:11 | 463341556–463347876 | 2 | 4/4 | 16 | 16 | 82,384 | — |
| 20261030T161201581-pilot | live | 2026-10-30 16:12 → 19:11 | 463348264–463398292 | 14 | 39/39 | 156 | 156 | 950,676 | — |
| 20261031T000007392-session-C | live | 2026-10-31 00:01 → 04:59 | 463479204–463562584 | 14 | 215/218 | 860 | 847 | 4,379,666 | — |
| 20261031T100006476-session-A | live | 2026-10-31 10:00 → 13:58 | 463647152–463713744 | 14 | 242/245 | 968 | 951 | 4,917,284 | — |
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
| campaign | 500 | 0.994 (500) | pass | 1.000 (248) | pass | 2.0 / 61.4 |
| dry run | 639 | 1.000 (639) | pass | 1.000 (330) | pass | 2.0 / 17.0 |
<!-- /table:timing -->

**G1, positive controls, and G3, selection.**

<!-- table:gates -->
*Source: results/results.json → G1, G3.*

| gate |  |  | result |
|---|---|---|---|
| G1 Agave line | C1 0.79 [0.49, 0.95] (11/14) | C2 0.88 [0.47, 1.00] (7/8) | pass |
| G1 Firedancer | C1 1.00 [0.69, 1.00] (10/10) | C2 1.00 [0.66, 1.00] (9/9) | pass |
| G3 C1 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 C2 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 C3 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 EQ | preferred only in turn: 1 | other only: 0 | McNemar p 1.000 |
| G3 R | preferred only in turn: 0 | other only: 3 | McNemar p 0.250 |
| G3 R_IND | preferred only in turn: 0 | other only: 1 | McNemar p 1.000 |
| G3 V_EQ | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
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
| Agave | insufficient data | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| JitoLabs | arrival order breaks ties (F > 0.5) | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| AgaveBam | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | higher key first in the backlog (P(HI first) > 0.5) | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| HarmonicAgave | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| HarmonicFiredancer | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | no key preference shown in the backlog | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| Firedancer | latest arrival first (F < 0.5) | no evidence that landing depends on the send offset | higher key first in the backlog (P(HI first) > 0.5) | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| FireBAM | insufficient data | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| Rakurai | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data |
<!-- /table:verdicts -->

### Primary tests

<!-- table:primary -->
*Source: results/results.json → P1, P2, P3, holm_primary.*

| family | P1: F (first-sent first), 95 % CI | P1 p | P1 Holm p | P2 quads | P2 p | P2 Holm p |
|---|---|---|---|---|---|---|
| Agave | 0.73 [0.39, 0.94] (8/11) | 0.227 | 1.000 | 15 | 1.000 | 1.000 |
| JitoLabs | 0.77 [0.60, 0.90] (27/35) | 0.002 | 0.030 | 49 | 0.892 | 1.000 |
| AgaveBam | 0.45 [0.33, 0.57] (31/69) | 0.470 | 1.000 | 90 | 0.156 | 1.000 |
| HarmonicAgave | 0.54 [0.42, 0.66] (39/72) | 0.556 | 1.000 | 92 | 0.030 | 0.451 |
| HarmonicFiredancer | 0.56 [0.43, 0.68] (37/66) | 0.389 | 1.000 | 90 | 0.107 | 1.000 |
| Firedancer | 0.22 [0.12, 0.33] (14/65) | < 0.001 | < 0.001 | 83 | 1.000 | 1.000 |
| FireBAM | 0.80 [0.28, 0.99] (4/5) | 0.375 | 1.000 | 17 | 0.782 | 1.000 |
| Rakurai | 0.50 [0.16, 0.84] (4/8) | 1.000 | 1.000 | 12 | 1.000 | 1.000 |
<!-- /table:primary -->

### Secondary tests

<!-- table:secondary -->
*Source: results/results.json → S1, S2, P2 (leak), holm_secondary.*

| family | S1: P(HI first) pre-turn | S1 Holm p | S2 C1 | S2 C2 | S2 C3 | S3 Holm p |
|---|---|---|---|---|---|---|
| Agave | 0.75 [0.19, 0.99] (3/4) | 1.000 | 0.75 [0.19, 0.99] (3/4); Holm p 1.000 | —; Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.000 |
| JitoLabs | 0.88 [0.47, 1.00] (7/8) | 0.844 | 0.80 [0.44, 0.97] (8/10); Holm p 1.000 | 0.88 [0.47, 1.00] (7/8); Holm p 0.844 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.000 |
| AgaveBam | 0.90 [0.70, 0.99] (19/21) | 0.004 | 1.00 [0.66, 1.00] (9/9); Holm p 0.061 | 1.00 [0.69, 1.00] (10/10); Holm p 0.035 | 1.00 [0.69, 1.00] (10/10); Holm p 0.035 | 1.000 |
| HarmonicAgave | 0.85 [0.55, 0.98] (11/13) | 0.292 | 1.00 [0.77, 1.00] (14/14); Holm p 0.002 | 0.92 [0.64, 1.00] (12/13); Holm p 0.055 | 1.00 [0.66, 1.00] (9/9); Holm p 0.061 | 1.000 |
| HarmonicFiredancer | 0.87 [0.60, 0.98] (13/15) | 0.103 | 0.90 [0.55, 1.00] (9/10); Holm p 0.290 | 0.83 [0.52, 0.98] (10/12); Holm p 0.482 | 1.00 [0.69, 1.00] (10/10); Holm p 0.035 | 1.000 |
| Firedancer | 0.94 [0.73, 1.00] (17/18) | 0.003 | 1.00 [0.69, 1.00] (10/10); Holm p 0.035 | 1.00 [0.66, 1.00] (9/9); Holm p 0.061 | 1.00 [0.74, 1.00] (12/12); Holm p 0.009 | 1.000 |
| FireBAM | 1.00 [0.03, 1.00] (1/1) | 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.000 |
| Rakurai | 1.00 [0.16, 1.00] (2/2) | 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.000 |
<!-- /table:secondary -->

### Sensitivity analyses

(a) pairs excluded only for timing tolerance, kept and coded by their achieved values; (b) pairs excluded only for the
client rule, kept; (c) censored coding, where a pair with one member in the turn counts as that member first.

<!-- table:sensitivity -->
*Source: results/results.json → sensitivity (PREREGISTRATION §4 a, b, c).*

| analysis | family | F | P(HI first) pre-turn |
|---|---|---|---|
| a_timing_by_achieved | Agave | 0.73 [0.39, 0.94] (8/11) | 0.75 [0.19, 0.99] (3/4) |
| a_timing_by_achieved | JitoLabs | 0.71 [0.54, 0.85] (27/38) | 0.89 [0.52, 1.00] (8/9) |
| a_timing_by_achieved | AgaveBam | 0.44 [0.32, 0.57] (31/70) | 0.90 [0.70, 0.99] (19/21) |
| a_timing_by_achieved | HarmonicAgave | 0.54 [0.42, 0.66] (40/74) | 0.87 [0.60, 0.98] (13/15) |
| a_timing_by_achieved | HarmonicFiredancer | 0.57 [0.44, 0.68] (39/69) | 0.88 [0.62, 0.98] (14/16) |
| a_timing_by_achieved | Firedancer | 0.21 [0.12, 0.33] (14/66) | 0.95 [0.74, 1.00] (18/19) |
| a_timing_by_achieved | FireBAM | 0.75 [0.35, 0.97] (6/8) | 1.00 [0.16, 1.00] (2/2) |
| a_timing_by_achieved | Rakurai | 0.50 [0.16, 0.84] (4/8) | 1.00 [0.16, 1.00] (2/2) |
| a_timing_by_achieved | Raiku | 0.43 [0.10, 0.82] (3/7) | 1.00 [0.29, 1.00] (3/3) |
| b_client_included | Agave | 0.73 [0.39, 0.94] (8/11) | 0.75 [0.19, 0.99] (3/4) |
| b_client_included | JitoLabs | 0.65 [0.52, 0.76] (43/66) | 0.95 [0.75, 1.00] (19/20) |
| b_client_included | AgaveBam | 0.45 [0.33, 0.57] (31/69) | 0.90 [0.70, 0.99] (19/21) |
| b_client_included | HarmonicAgave | 0.54 [0.42, 0.66] (39/72) | 0.85 [0.55, 0.98] (11/13) |
| b_client_included | HarmonicFiredancer | 0.56 [0.43, 0.68] (37/66) | 0.87 [0.60, 0.98] (13/15) |
| b_client_included | Firedancer | 0.22 [0.12, 0.33] (14/65) | 0.94 [0.73, 1.00] (17/18) |
| b_client_included | FireBAM | 0.80 [0.28, 0.99] (4/5) | 1.00 [0.03, 1.00] (1/1) |
| b_client_included | Rakurai | 0.50 [0.16, 0.84] (4/8) | 1.00 [0.16, 1.00] (2/2) |
| b_client_included | Raiku | 0.50 [0.12, 0.88] (3/6) | 1.00 [0.29, 1.00] (3/3) |
| c_censored | Agave | 0.73 [0.39, 0.94] (8/11) | 0.75 [0.19, 0.99] (3/4) |
| c_censored | JitoLabs | 0.77 [0.60, 0.90] (27/35) | 0.88 [0.47, 1.00] (7/8) |
| c_censored | AgaveBam | 0.45 [0.33, 0.57] (31/69) | 0.90 [0.70, 0.99] (19/21) |
| c_censored | HarmonicAgave | 0.54 [0.42, 0.66] (39/72) | 0.85 [0.55, 0.98] (11/13) |
| c_censored | HarmonicFiredancer | 0.57 [0.44, 0.69] (38/67) | 0.87 [0.60, 0.98] (13/15) |
| c_censored | Firedancer | 0.22 [0.12, 0.33] (14/65) | 0.94 [0.73, 1.00] (17/18) |
| c_censored | FireBAM | 0.80 [0.28, 0.99] (4/5) | 1.00 [0.03, 1.00] (1/1) |
| c_censored | Rakurai | 0.50 [0.16, 0.84] (4/8) | 1.00 [0.16, 1.00] (2/2) |
| c_censored | Raiku | 0.50 [0.12, 0.88] (3/6) | 1.00 [0.29, 1.00] (3/3) |
<!-- /table:sensitivity -->

## 4. Position in the leader's turn

<!-- TODO(data): turn-map figure (figures/turn-map-*.svg) and reading of P2/S3 per family. -->

Where in their block my in-turn probes sat (exploratory):

<!-- table:insertion -->
*Source: results/explore.json → insertion (exploratory).*

| family | slot of turn | probes | position in block (0 first, 1 last), median [IQR] |
|---|---|---|---|
| Agave | 0 | 36 | 0.14 [0.09, 0.35] |
| Agave | 1 | 11 | 0.43 [0.08, 0.60] |
| Agave | 2 | 9 | 0.26 [0.09, 0.56] |
| Agave | 3 | 4 | 0.61 [0.61, 0.65] |
| JitoLabs | 0 | 233 | 0.13 [0.08, 0.22] |
| JitoLabs | 1 | 50 | 0.52 [0.12, 0.91] |
| JitoLabs | 2 | 44 | 0.58 [0.39, 0.63] |
| JitoLabs | 3 | 40 | 0.63 [0.60, 0.68] |
| AgaveBam | 0 | 216 | 0.15 [0.11, 0.23] |
| AgaveBam | 1 | 47 | 0.44 [0.36, 0.87] |
| AgaveBam | 2 | 46 | 0.45 [0.21, 0.88] |
| AgaveBam | 3 | 35 | 0.63 [0.14, 0.87] |
| HarmonicAgave | 0 | 183 | 0.07 [0.03, 0.24] |
| HarmonicAgave | 1 | 64 | 0.31 [0.03, 0.55] |
| HarmonicAgave | 2 | 55 | 0.42 [0.23, 0.66] |
| HarmonicAgave | 3 | 49 | 0.22 [0.04, 0.92] |
| HarmonicFiredancer | 0 | 175 | 0.07 [0.02, 0.33] |
| HarmonicFiredancer | 1 | 68 | 0.28 [0.05, 0.64] |
| HarmonicFiredancer | 2 | 47 | 0.40 [0.26, 0.59] |
| HarmonicFiredancer | 3 | 56 | 0.53 [0.07, 0.92] |
| Firedancer | 0 | 235 | 0.07 [0.03, 0.55] |
| Firedancer | 1 | 36 | 0.32 [0.29, 0.42] |
| Firedancer | 2 | 60 | 0.44 [0.07, 0.75] |
| Firedancer | 3 | 1 | 0.13 [0.13, 0.13] |
| FireBAM | 0 | 27 | 0.16 [0.14, 0.55] |
| FireBAM | 1 | 9 | 0.41 [0.40, 0.45] |
| FireBAM | 2 | 4 | 0.05 [0.05, 0.17] |
| FireBAM | 3 | 3 | 0.57 [0.57, 0.57] |
| Rakurai | 0 | 32 | 0.10 [0.08, 0.15] |
| Rakurai | 1 | 4 | 0.47 [0.47, 0.49] |
| Rakurai | 2 | 8 | 0.24 [0.22, 0.25] |
| Rakurai | 3 | 4 | 0.12 [0.12, 0.12] |
| Raiku | 0 | 20 | 0.10 [0.05, 0.13] |
| Raiku | 1 | 8 | 0.46 [0.12, 0.81] |
| Raiku | 3 | 8 | 0.51 [0.50, 0.55] |
<!-- /table:insertion -->

## 5. Load

P3 is the pre-registered load test; the per-family slopes are descriptive.

<!-- table:load -->
*Source: results/results.json → P3 (equivalence margin ±0.4).*

| R pairs | n | β (logit per 10M CU) | 90 % CI | p | Holm p | verdict |
|---|---|---|---|---|---|---|
| pooled (P3) | 209 | -0.267 | [-1.035, 0.502] | 0.568 | 1.000 | inconclusive |
| Agave | 8 | — | [—, —] | — |  | descriptive |
| AgaveBam | 48 | -0.075 | [-1.913, 1.764] | 0.947 |  | descriptive |
| FireBAM | 4 | — | [—, —] | — |  | descriptive |
| Firedancer | 43 | -20.822 | [-117.292, 75.648] | 0.723 |  | descriptive |
| HarmonicAgave | 37 | 1.038 | [-0.929, 3.005] | 0.385 |  | descriptive |
| HarmonicFiredancer | 45 | -0.368 | [-1.934, 1.198] | 0.699 |  | descriptive |
| JitoLabs | 20 | -136.547 | [-168339.225, 168066.131] | 0.999 |  | descriptive |
| Raiku | 5 | — | [—, —] | — |  | descriptive |
| Rakurai | 4 | — | [—, —] | — |  | descriptive |
<!-- /table:load -->

Every archived block of every probed turn, all transactions (exploratory). Sortedness is Kendall's τ between position
and key, signed so that +1 means the highest key first. The approximate priority leaves out instruction-data cost and
skips transactions without a CU limit (`analysis/itb_analysis/explore.py`).

<!-- table:whole_block -->
*Source: results/explore.json → whole_block (exploratory; +1 = sorted high to low).*

| family | blocks | sortedness by CU price, median [IQR] | by approx. priority, median [IQR] | transactions per block (median) | cost units per block (median) |
|---|---|---|---|---|---|
| Agave | 60 | 0.02 [-0.04, 0.07] | 0.01 [-0.03, 0.06] | 276 | 12.9M |
| JitoLabs | 368 | 0.03 [-0.03, 0.12] | 0.06 [0.00, 0.13] | 245 | 11.8M |
| AgaveBam | 360 | 0.07 [-0.01, 0.15] | 0.08 [0.02, 0.15] | 258 | 11.8M |
| HarmonicAgave | 368 | 0.05 [-0.01, 0.11] | 0.07 [0.02, 0.13] | 264 | 11.4M |
| HarmonicFiredancer | 360 | 0.05 [-0.01, 0.12] | 0.07 [0.01, 0.12] | 263 | 11.9M |
| Firedancer | 332 | 0.14 [0.07, 0.21] | 0.11 [0.05, 0.17] | 323 | 15.9M |
| FireBAM | 68 | 0.05 [-0.02, 0.13] | 0.08 [0.03, 0.14] | 284 | 12.6M |
| Rakurai | 48 | 0.02 [-0.04, 0.05] | 0.01 [-0.03, 0.04] | 295 | 15.0M |
| Raiku | 36 | 0.00 [-0.04, 0.09] | 0.04 [-0.02, 0.12] | 262 | 12.4M |
<!-- /table:whole_block -->

Landing in the target's turn by session (exploratory):

<!-- table:hour -->
*Source: results/explore.json → hour (exploratory).*

| family | session | probes in the target's turn |
|---|---|---|
| all | A | 0.96 [0.95, 0.97] (930/968) |
| all | C | 0.96 [0.95, 0.98] (829/860) |
| all | pilot | 0.97 [0.94, 0.99] (152/156) |
| all | smoke | 1.00 [0.79, 1.00] (16/16) |
| Agave | A | 1.00 [0.79, 1.00] (16/16) |
| Agave | C | 1.00 [0.90, 1.00] (36/36) |
| Agave | pilot | 1.00 [0.63, 1.00] (8/8) |
| JitoLabs | A | 0.99 [0.97, 1.00] (183/184) |
| JitoLabs | C | 1.00 [0.98, 1.00] (152/152) |
| JitoLabs | pilot | 1.00 [0.86, 1.00] (24/24) |
| JitoLabs | smoke | 1.00 [0.63, 1.00] (8/8) |
| AgaveBam | A | 0.96 [0.92, 0.98] (176/184) |
| AgaveBam | C | 0.95 [0.90, 0.98] (144/152) |
| AgaveBam | pilot | 1.00 [0.86, 1.00] (24/24) |
| HarmonicAgave | A | 0.98 [0.95, 0.99] (180/184) |
| HarmonicAgave | C | 0.91 [0.86, 0.95] (139/152) |
| HarmonicAgave | pilot | 1.00 [0.86, 1.00] (24/24) |
| HarmonicAgave | smoke | 1.00 [0.63, 1.00] (8/8) |
| HarmonicFiredancer | A | 0.93 [0.88, 0.96] (171/184) |
| HarmonicFiredancer | C | 0.99 [0.96, 1.00] (151/152) |
| HarmonicFiredancer | pilot | 1.00 [0.86, 1.00] (24/24) |
| Firedancer | A | 1.00 [0.98, 1.00] (164/164) |
| Firedancer | C | 1.00 [0.97, 1.00] (144/144) |
| Firedancer | pilot | 1.00 [0.86, 1.00] (24/24) |
| FireBAM | A | 0.50 [0.29, 0.71] (12/24) |
| FireBAM | C | 0.72 [0.53, 0.86] (23/32) |
| FireBAM | pilot | 0.67 [0.35, 0.90] (8/12) |
| Rakurai | A | 1.00 [0.74, 1.00] (12/12) |
| Rakurai | C | 1.00 [0.86, 1.00] (24/24) |
| Rakurai | pilot | 1.00 [0.74, 1.00] (12/12) |
| Raiku | A | 1.00 [0.79, 1.00] (16/16) |
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
| Agave | — | — | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.97] (0/1) | — | 0.00 [0.00, 0.97] (0/1) |
| JitoLabs | — | 1.00 [0.16, 1.00] (2/2) | — | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.84] (0/2) | 0.00 [0.00, 0.71] (0/3) |
| AgaveBam | — | 1.00 [0.48, 1.00] (5/5) | 0.50 [0.07, 0.93] (2/4) | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.71] (0/3) | 0.00 [0.00, 0.52] (0/5) |
| HarmonicAgave | 1.00 [0.03, 1.00] (1/1) | 0.75 [0.19, 0.99] (3/4) | 0.33 [0.01, 0.91] (1/3) | 0.75 [0.19, 0.99] (3/4) | — | 0.00 [0.00, 0.97] (0/1) |
| HarmonicFiredancer | — | 0.80 [0.28, 0.99] (4/5) | 0.86 [0.42, 1.00] (6/7) | 0.20 [0.01, 0.72] (1/5) | 0.00 [0.00, 0.84] (0/2) | 0.00 [0.00, 0.71] (0/3) |
| Firedancer | 1.00 [0.03, 1.00] (1/1) | 1.00 [0.40, 1.00] (4/4) | 0.60 [0.15, 0.95] (3/5) | 0.00 [0.00, 0.71] (0/3) | 0.00 [0.00, 0.84] (0/2) | 0.00 [0.00, 0.84] (0/2) |
| FireBAM | — | — | — | — | 0.00 [0.00, 0.97] (0/1) | — |
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
| AgaveBam | 18 | batch | 0.04 (s = 18.69) | 0.36 (a = 1.00, h = 14.23) | 0.60 (B = 23.06, a = 1.00) | 0.00 (a = 0.39) | B [10.00, 50.00]; a [0.70, 1.00] | 0.95 |
| HarmonicFiredancer | 22 | batch | 0.05 (s = 32.22) | 0.32 (a = 0.96, h = 22.78) | 0.64 (B = 35.79, a = 0.93) | 0.00 (a = 0.50) | B [18.80, 60.98]; a [0.68, 1.00] | 0.88 |
| Firedancer | 17 | batch | 0.02 (s = 17.78) | 0.27 (a = 1.00, h = 14.14) | 0.71 (B = 25.00, a = 1.00) | 0.00 (a = 0.47) | B [12.57, 28.47]; a [0.75, 1.00] | 1.00 |
<!-- /table:mechanism -->

## 7. Clients and operators

Per validator (descriptive):

<!-- table:per_target -->
*Source: results/results.json → descriptive.per_target.*

| target | family | quads | probes in the target's turn | eligible pairs | F | R: HI first |
|---|---|---|---|---|---|---|
| T1 | Agave | 15 | 1.00 [0.94, 1.00] (60/60) | 30 | 0.73 [0.39, 0.94] (8/11) | 0.38 [0.09, 0.76] (3/8) |
| T2 | JitoLabs | 47 | 1.00 [0.98, 1.00] (188/188) | 8 | 0.33 [0.01, 0.91] (1/3) | 0.33 [0.01, 0.91] (1/3) |
| T3 | JitoLabs | 45 | 0.99 [0.97, 1.00] (179/180) | 83 | 0.81 [0.64, 0.93] (26/32) | 0.59 [0.33, 0.82] (10/17) |
| T4 | AgaveBam | 45 | 0.98 [0.94, 0.99] (176/180) | 88 | 0.40 [0.24, 0.58] (14/35) | 0.60 [0.39, 0.79] (15/25) |
| T5 | AgaveBam | 45 | 0.93 [0.89, 0.97] (168/180) | 83 | 0.50 [0.32, 0.68] (17/34) | 0.57 [0.34, 0.77] (13/23) |
| T6 | HarmonicAgave | 47 | 0.99 [0.97, 1.00] (187/188) | 87 | 0.49 [0.32, 0.66] (18/37) | 0.67 [0.41, 0.87] (12/18) |
| T7 | HarmonicAgave | 45 | 0.91 [0.86, 0.95] (164/180) | 82 | 0.60 [0.42, 0.76] (21/35) | 0.74 [0.49, 0.91] (14/19) |
| T8 | HarmonicFiredancer | 45 | 0.94 [0.90, 0.97] (170/180) | 80 | 0.55 [0.36, 0.74] (16/29) | 0.70 [0.47, 0.87] (16/23) |
| T9 | HarmonicFiredancer | 45 | 0.98 [0.94, 0.99] (176/180) | 86 | 0.57 [0.39, 0.73] (21/37) | 0.64 [0.41, 0.83] (14/22) |
| T10 | Firedancer | 45 | 1.00 [0.98, 1.00] (180/180) | 90 | 0.28 [0.14, 0.45] (10/36) | 0.68 [0.46, 0.85] (17/25) |
| T11 | Firedancer | 38 | 1.00 [0.98, 1.00] (152/152) | 74 | 0.14 [0.04, 0.32] (4/29) | 0.67 [0.41, 0.87] (12/18) |
| T12 | FireBAM | 17 | 0.63 [0.51, 0.75] (43/68) | 15 | 0.80 [0.28, 0.99] (4/5) | 0.75 [0.19, 0.99] (3/4) |
| T13 | Rakurai | 12 | 1.00 [0.93, 1.00] (48/48) | 24 | 0.50 [0.16, 0.84] (4/8) | 0.50 [0.07, 0.93] (2/4) |
| X1 | Raiku | 9 | 1.00 [0.90, 1.00] (36/36) | 17 | 0.50 [0.12, 0.88] (3/6) | 0.60 [0.15, 0.95] (3/5) |
<!-- /table:per_target -->

Families with two validators: intraclass correlation of in-turn landing and of F across the two (ANOVA estimator) and
a χ² test of equal in-turn landing (exploratory):

<!-- table:operators -->
*Source: results/explore.json → operators (exploratory).*

| family | target | in the target's turn | F | ICC in-turn | ICC F | χ² p (equal in-turn landing) |
|---|---|---|---|---|---|---|
| AgaveBam | T4 | 0.98 [0.94, 0.99] (176/180) | 0.40 [0.24, 0.58] (14/35) | 0.018 | -0.009 | 0.073 |
| AgaveBam | T5 | 0.93 [0.89, 0.97] (168/180) | 0.50 [0.32, 0.68] (17/34) | 0.018 | -0.009 | 0.073 |
| Firedancer | T10 | 1.00 [0.98, 1.00] (180/180) | 0.28 [0.14, 0.45] (10/36) | — | 0.026 | — |
| Firedancer | T11 | 1.00 [0.98, 1.00] (152/152) | 0.14 [0.04, 0.32] (4/29) | — | 0.026 | — |
| HarmonicAgave | T6 | 0.99 [0.97, 1.00] (187/188) | 0.49 [0.32, 0.66] (18/37) | 0.071 | -0.002 | < 0.001 |
| HarmonicAgave | T7 | 0.91 [0.86, 0.95] (164/180) | 0.60 [0.42, 0.76] (21/35) | 0.071 | -0.002 | < 0.001 |
| HarmonicFiredancer | T8 | 0.94 [0.90, 0.97] (170/180) | 0.55 [0.36, 0.74] (16/29) | 0.009 | -0.031 | 0.173 |
| HarmonicFiredancer | T9 | 0.98 [0.94, 0.99] (176/180) | 0.57 [0.39, 0.73] (21/37) | 0.009 | -0.031 | 0.173 |
| JitoLabs | T2 | 1.00 [0.98, 1.00] (188/188) | 0.33 [0.01, 0.91] (1/3) | 0.000 | 0.334 | 0.983 |
| JitoLabs | T3 | 0.99 [0.97, 1.00] (179/180) | 0.81 [0.64, 0.93] (26/32) | 0.000 | 0.334 | 0.983 |
<!-- /table:operators -->

Transactions between the two members of a pair that landed in one block (exploratory):

<!-- table:between -->
*Source: results/explore.json → between (exploratory).*

| family | pairs in one block | adjacent | transactions between (median) | p90 |
|---|---|---|---|---|
| Agave | 29 | 0.48 [0.29, 0.67] (14/29) | 1 | 24 |
| AgaveBam | 164 | 0.09 [0.05, 0.15] (15/164) | 3 | 32 |
| FireBAM | 20 | 0.30 [0.12, 0.54] (6/20) | 1 | 24 |
| Firedancer | 163 | 0.02 [0.01, 0.06] (4/163) | 11 | 29 |
| HarmonicAgave | 173 | 0.23 [0.17, 0.30] (40/173) | 4 | 29 |
| HarmonicFiredancer | 170 | 0.19 [0.13, 0.26] (32/170) | 4 | 39 |
| JitoLabs | 174 | 0.37 [0.30, 0.45] (65/174) | 2 | 17 |
| Raiku | 18 | 0.72 [0.47, 0.90] (13/18) | 0 | 17 |
| Rakurai | 24 | 0.67 [0.45, 0.84] (16/24) | 0 | 22 |
<!-- /table:between -->

The v1 arm (exploratory, PREREGISTRATION §3 rule 2):

<!-- table:v1 -->
*Source: results/results.json → descriptive (v1 arm).*

|  | V_EQ: v1 member first (equal keys) | V_HI: v1 high-key member first |
|---|---|---|
| in the target's turn: v1 probes vs legacy probes of the same quads | 0.96 [0.90, 0.99] (99/103) | 0.95 [0.90, 0.98] (125/131) |
| Agave | 0.50 [0.01, 0.99] (1/2) | 1.00 [0.16, 1.00] (2/2) |
| JitoLabs | 0.20 [0.01, 0.72] (1/5) | 1.00 [0.54, 1.00] (6/6) |
| AgaveBam | 0.38 [0.09, 0.76] (3/8) | 1.00 [0.63, 1.00] (8/8) |
| HarmonicAgave | 0.67 [0.30, 0.93] (6/9) | 1.00 [0.40, 1.00] (4/4) |
| HarmonicFiredancer | 0.36 [0.11, 0.69] (4/11) | 0.83 [0.36, 1.00] (5/6) |
| Firedancer | 0.62 [0.32, 0.86] (8/13) | 0.75 [0.19, 0.99] (3/4) |
| FireBAM | — | 1.00 [0.16, 1.00] (2/2) |
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
| campaign | probe | 1852 | 1798 | 9,296,950 |
| pilot | probe | 156 | 156 | 950,676 |
| setup | fund | 2 | 2 | 10,000 |
| setup | memo | 1 | 1 | 5,000 |
| smoke | probe | 16 | 16 | 82,384 |
| total |  | 2027 | 1973 | 10,345,010 (0.010345 SOL) |
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
| fees paid | 10,345,010 lamports (0.010345 SOL) of the 30,000,000 cap |
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
| claims checked offline | 2000 |
| failures | 0 |
| pair orders recomputed | — |
| result | PASS |
<!-- /table:verify -->

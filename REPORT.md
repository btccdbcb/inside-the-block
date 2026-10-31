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
| campaign | 258 | 0.996 (258) | pass | 1.000 (127) | pass | 2.1 / 88.6 |
| dry run | 639 | 1.000 (639) | pass | 1.000 (330) | pass | 2.0 / 17.0 |
<!-- /table:timing -->

**G1, positive controls, and G3, selection.**

<!-- table:gates -->
*Source: results/results.json → G1, G3.*

| gate |  |  | result |
|---|---|---|---|
| G1 Agave line | C1 0.82 [0.48, 0.98] (9/11) | C2 1.00 [0.40, 1.00] (4/4) | fail |
| G1 Firedancer | C1 1.00 [0.59, 1.00] (7/7) | C2 1.00 [0.40, 1.00] (4/4) | fail |
| G3 C1 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 C2 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 C3 | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 EQ | preferred only in turn: 0 | other only: 0 | McNemar p 1.000 |
| G3 R | preferred only in turn: 0 | other only: 2 | McNemar p 0.500 |
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
| Agave | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data |
| JitoLabs | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| AgaveBam | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| HarmonicAgave | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| HarmonicFiredancer | F not distinguishable from 0.5 | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| Firedancer | latest arrival first (F < 0.5) | no evidence that landing depends on the send offset | insufficient data | insufficient data | insufficient data | insufficient data | no evidence that leakage depends on the send offset |
| FireBAM | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data |
| Rakurai | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data | insufficient data |
<!-- /table:verdicts -->

### Primary tests

<!-- table:primary -->
*Source: results/results.json → P1, P2, P3, holm_primary.*

| family | P1: F (first-sent first), 95 % CI | P1 p | P1 Holm p | P2 quads | P2 p | P2 Holm p |
|---|---|---|---|---|---|---|
| Agave | 0.75 [0.35, 0.97] (6/8) | 0.289 | 1.000 | 11 | 1.000 | 1.000 |
| JitoLabs | 0.79 [0.54, 0.94] (15/19) | 0.019 | 0.307 | 26 | 1.000 | 1.000 |
| AgaveBam | 0.33 [0.19, 0.51] (12/36) | 0.065 | 0.979 | 44 | 0.400 | 1.000 |
| HarmonicAgave | 0.62 [0.44, 0.79] (20/32) | 0.215 | 1.000 | 46 | 0.252 | 1.000 |
| HarmonicFiredancer | 0.52 [0.33, 0.71] (15/29) | 1.000 | 1.000 | 44 | 0.502 | 1.000 |
| Firedancer | 0.18 [0.07, 0.35] (6/33) | < 0.001 | 0.006 | 42 | 1.000 | 1.000 |
| FireBAM | 1.00 [0.16, 1.00] (2/2) | 0.500 | 1.000 | 11 | 0.203 | 1.000 |
| Rakurai | 0.67 [0.22, 0.96] (4/6) | 0.688 | 1.000 | 9 | 1.000 | 1.000 |
<!-- /table:primary -->

### Secondary tests

<!-- table:secondary -->
*Source: results/results.json → S1, S2, P2 (leak), holm_secondary.*

| family | S1: P(HI first) pre-turn | S1 Holm p | S2 C1 | S2 C2 | S2 C3 | S3 Holm p |
|---|---|---|---|---|---|---|
| Agave | 0.50 [0.01, 0.99] (1/2) | 1.000 | 0.75 [0.19, 0.99] (3/4); Holm p 1.000 | —; Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.000 |
| JitoLabs | 0.80 [0.28, 0.99] (4/5) | 1.000 | 0.86 [0.42, 1.00] (6/7); Holm p 1.000 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.000 |
| AgaveBam | 0.91 [0.59, 1.00] (10/11) | 0.223 | 1.00 [0.29, 1.00] (3/3); Holm p 1.000 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.00 [0.48, 1.00] (5/5); Holm p 1.000 | 1.000 |
| HarmonicAgave | 0.75 [0.19, 0.99] (3/4) | 1.000 | 1.00 [0.54, 1.00] (6/6); Holm p 0.562 | 0.88 [0.47, 1.00] (7/8); Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.000 |
| HarmonicFiredancer | 0.83 [0.36, 1.00] (5/6) | 1.000 | 0.50 [0.01, 0.99] (1/2); Holm p 1.000 | 0.71 [0.29, 0.96] (5/7); Holm p 1.000 | 1.00 [0.63, 1.00] (8/8); Holm p 0.152 | 1.000 |
| Firedancer | 1.00 [0.69, 1.00] (10/10) | 0.039 | 1.00 [0.59, 1.00] (7/7); Holm p 0.289 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.000 |
| FireBAM | — | 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.00 [0.16, 1.00] (2/2); Holm p 1.000 | 1.000 |
| Rakurai | 1.00 [0.03, 1.00] (1/1) | 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.00 [0.40, 1.00] (4/4); Holm p 1.000 | 1.00 [0.03, 1.00] (1/1); Holm p 1.000 | 1.000 |
<!-- /table:secondary -->

### Sensitivity analyses

(a) pairs excluded only for timing tolerance, kept and coded by their achieved values; (b) pairs excluded only for the
client rule, kept; (c) censored coding, where a pair with one member in the turn counts as that member first.

<!-- table:sensitivity -->
*Source: results/results.json → sensitivity (PREREGISTRATION §4 a, b, c).*

| analysis | family | F | P(HI first) pre-turn |
|---|---|---|---|
| a_timing_by_achieved | Agave | 0.75 [0.35, 0.97] (6/8) | 0.50 [0.01, 0.99] (1/2) |
| a_timing_by_achieved | JitoLabs | 0.75 [0.51, 0.91] (15/20) | 0.83 [0.36, 1.00] (5/6) |
| a_timing_by_achieved | AgaveBam | 0.33 [0.19, 0.51] (12/36) | 0.91 [0.59, 1.00] (10/11) |
| a_timing_by_achieved | HarmonicAgave | 0.62 [0.44, 0.78] (21/34) | 0.83 [0.36, 1.00] (5/6) |
| a_timing_by_achieved | HarmonicFiredancer | 0.55 [0.36, 0.73] (17/31) | 0.83 [0.36, 1.00] (5/6) |
| a_timing_by_achieved | Firedancer | 0.18 [0.07, 0.35] (6/34) | 1.00 [0.72, 1.00] (11/11) |
| a_timing_by_achieved | FireBAM | 0.80 [0.28, 0.99] (4/5) | 1.00 [0.03, 1.00] (1/1) |
| a_timing_by_achieved | Rakurai | 0.67 [0.22, 0.96] (4/6) | 1.00 [0.03, 1.00] (1/1) |
| a_timing_by_achieved | Raiku | 0.33 [0.01, 0.91] (1/3) | 1.00 [0.29, 1.00] (3/3) |
| b_client_included | Agave | 0.75 [0.35, 0.97] (6/8) | 0.50 [0.01, 0.99] (1/2) |
| b_client_included | JitoLabs | 0.66 [0.48, 0.81] (23/35) | 0.92 [0.62, 1.00] (11/12) |
| b_client_included | AgaveBam | 0.33 [0.19, 0.51] (12/36) | 0.91 [0.59, 1.00] (10/11) |
| b_client_included | HarmonicAgave | 0.62 [0.44, 0.79] (20/32) | 0.75 [0.19, 0.99] (3/4) |
| b_client_included | HarmonicFiredancer | 0.52 [0.33, 0.71] (15/29) | 0.83 [0.36, 1.00] (5/6) |
| b_client_included | Firedancer | 0.18 [0.07, 0.35] (6/33) | 1.00 [0.69, 1.00] (10/10) |
| b_client_included | FireBAM | 1.00 [0.16, 1.00] (2/2) | — |
| b_client_included | Rakurai | 0.67 [0.22, 0.96] (4/6) | 1.00 [0.03, 1.00] (1/1) |
| b_client_included | Raiku | 0.33 [0.01, 0.91] (1/3) | 1.00 [0.29, 1.00] (3/3) |
| c_censored | Agave | 0.75 [0.35, 0.97] (6/8) | 0.50 [0.01, 0.99] (1/2) |
| c_censored | JitoLabs | 0.79 [0.54, 0.94] (15/19) | 0.80 [0.28, 0.99] (4/5) |
| c_censored | AgaveBam | 0.33 [0.19, 0.51] (12/36) | 0.91 [0.59, 1.00] (10/11) |
| c_censored | HarmonicAgave | 0.62 [0.44, 0.79] (20/32) | 0.75 [0.19, 0.99] (3/4) |
| c_censored | HarmonicFiredancer | 0.52 [0.33, 0.71] (15/29) | 0.83 [0.36, 1.00] (5/6) |
| c_censored | Firedancer | 0.18 [0.07, 0.35] (6/33) | 1.00 [0.69, 1.00] (10/10) |
| c_censored | FireBAM | 1.00 [0.16, 1.00] (2/2) | — |
| c_censored | Rakurai | 0.67 [0.22, 0.96] (4/6) | 1.00 [0.03, 1.00] (1/1) |
| c_censored | Raiku | 0.33 [0.01, 0.91] (1/3) | 1.00 [0.29, 1.00] (3/3) |
<!-- /table:sensitivity -->

## 4. Position in the leader's turn

<!-- TODO(data): turn-map figure (figures/turn-map-*.svg) and reading of P2/S3 per family. -->

Where in their block my in-turn probes sat (exploratory):

<!-- table:insertion -->
*Source: results/explore.json → insertion (exploratory).*

| family | slot of turn | probes | position in block (0 first, 1 last), median [IQR] |
|---|---|---|---|
| Agave | 0 | 24 | 0.16 [0.09, 0.30] |
| Agave | 1 | 11 | 0.43 [0.08, 0.60] |
| Agave | 2 | 5 | 0.56 [0.56, 0.57] |
| Agave | 3 | 4 | 0.61 [0.61, 0.65] |
| JitoLabs | 0 | 112 | 0.10 [0.06, 0.19] |
| JitoLabs | 1 | 28 | 0.52 [0.12, 0.91] |
| JitoLabs | 2 | 23 | 0.58 [0.49, 0.65] |
| JitoLabs | 3 | 21 | 0.67 [0.63, 0.72] |
| AgaveBam | 0 | 111 | 0.15 [0.11, 0.23] |
| AgaveBam | 1 | 16 | 0.44 [0.42, 0.55] |
| AgaveBam | 2 | 28 | 0.45 [0.19, 0.86] |
| AgaveBam | 3 | 13 | 0.82 [0.04, 0.91] |
| HarmonicAgave | 0 | 83 | 0.06 [0.03, 0.49] |
| HarmonicAgave | 1 | 33 | 0.09 [0.02, 0.53] |
| HarmonicAgave | 2 | 22 | 0.42 [0.27, 0.55] |
| HarmonicAgave | 3 | 33 | 0.15 [0.04, 0.67] |
| HarmonicFiredancer | 0 | 91 | 0.05 [0.02, 0.12] |
| HarmonicFiredancer | 1 | 32 | 0.25 [0.12, 0.37] |
| HarmonicFiredancer | 2 | 25 | 0.34 [0.19, 0.57] |
| HarmonicFiredancer | 3 | 27 | 0.51 [0.15, 0.69] |
| Firedancer | 0 | 120 | 0.12 [0.03, 0.64] |
| Firedancer | 1 | 23 | 0.32 [0.27, 0.77] |
| Firedancer | 2 | 24 | 0.65 [0.11, 0.75] |
| Firedancer | 3 | 1 | 0.13 [0.13, 0.13] |
| FireBAM | 0 | 16 | 0.31 [0.15, 0.49] |
| FireBAM | 1 | 8 | 0.43 [0.40, 0.46] |
| FireBAM | 2 | 4 | 0.05 [0.05, 0.17] |
| FireBAM | 3 | 3 | 0.57 [0.57, 0.57] |
| Rakurai | 0 | 20 | 0.10 [0.09, 0.12] |
| Rakurai | 1 | 4 | 0.47 [0.47, 0.49] |
| Rakurai | 2 | 8 | 0.24 [0.22, 0.25] |
| Rakurai | 3 | 4 | 0.12 [0.12, 0.12] |
| Raiku | 0 | 12 | 0.10 [0.09, 0.12] |
| Raiku | 1 | 4 | 0.12 [0.11, 0.12] |
| Raiku | 3 | 4 | 0.51 [0.51, 0.54] |
<!-- /table:insertion -->

## 5. Load

P3 is the pre-registered load test; the per-family slopes are descriptive.

<!-- table:load -->
*Source: results/results.json → P3 (equivalence margin ±0.4).*

| R pairs | n | β (logit per 10M CU) | 90 % CI | p | Holm p | verdict |
|---|---|---|---|---|---|---|
| pooled (P3) | 101 | -0.511 | [-1.693, 0.672] | 0.477 | 1.000 | inconclusive |
| Agave | 5 | — | [—, —] | — |  | descriptive |
| AgaveBam | 23 | -20.386 | [-49264.499, 49223.726] | 0.999 |  | descriptive |
| FireBAM | 1 | — | [—, —] | — |  | descriptive |
| Firedancer | 23 | 17.000 | [-363253.946, 363287.947] | 1.000 |  | descriptive |
| HarmonicAgave | 16 | 61.387 | [-464643.086, 464765.860] | 1.000 |  | descriptive |
| HarmonicFiredancer | 21 | -62.503 | [-133950.980, 133825.974] | 0.999 |  | descriptive |
| JitoLabs | 10 | -61.709 | [-93587.503, 93464.085] | 0.999 |  | descriptive |
| Raiku | 4 | — | [—, —] | — |  | descriptive |
| Rakurai | 2 | — | [—, —] | — |  | descriptive |
<!-- /table:load -->

Every archived block of every probed turn, all transactions (exploratory). Sortedness is Kendall's τ between position
and key, signed so that +1 means the highest key first. The approximate priority leaves out instruction-data cost and
skips transactions without a CU limit (`analysis/itb_analysis/explore.py`).

<!-- table:whole_block -->
*Source: results/explore.json → whole_block (exploratory; +1 = sorted high to low).*

| family | blocks | sortedness by CU price, median [IQR] | by approx. priority, median [IQR] | transactions per block (median) | cost units per block (median) |
|---|---|---|---|---|---|
| Agave | 44 | 0.02 [-0.04, 0.07] | 0.01 [-0.03, 0.06] | 277 | 12.9M |
| JitoLabs | 184 | 0.03 [-0.04, 0.12] | 0.05 [-0.01, 0.12] | 290 | 12.3M |
| AgaveBam | 176 | 0.05 [-0.01, 0.15] | 0.07 [0.02, 0.15] | 284 | 12.4M |
| HarmonicAgave | 184 | 0.05 [-0.00, 0.11] | 0.08 [0.03, 0.13] | 304 | 12.9M |
| HarmonicFiredancer | 176 | 0.06 [-0.00, 0.11] | 0.06 [0.02, 0.11] | 304 | 13.5M |
| Firedancer | 168 | 0.12 [0.07, 0.17] | 0.10 [0.04, 0.15] | 376 | 16.5M |
| FireBAM | 44 | 0.05 [-0.02, 0.11] | 0.08 [0.04, 0.16] | 310 | 15.2M |
| Rakurai | 36 | 0.04 [-0.02, 0.07] | 0.02 [-0.03, 0.04] | 314 | 15.7M |
| Raiku | 20 | 0.01 [-0.03, 0.12] | 0.05 [-0.02, 0.14] | 302 | 12.8M |
<!-- /table:whole_block -->

Landing in the target's turn by session (exploratory):

<!-- table:hour -->
*Source: results/explore.json → hour (exploratory).*

| family | session | probes in the target's turn |
|---|---|---|
| all | C | 0.96 [0.95, 0.98] (829/860) |
| all | pilot | 0.97 [0.94, 0.99] (152/156) |
| all | smoke | 1.00 [0.79, 1.00] (16/16) |
| Agave | C | 1.00 [0.90, 1.00] (36/36) |
| Agave | pilot | 1.00 [0.63, 1.00] (8/8) |
| JitoLabs | C | 1.00 [0.98, 1.00] (152/152) |
| JitoLabs | pilot | 1.00 [0.86, 1.00] (24/24) |
| JitoLabs | smoke | 1.00 [0.63, 1.00] (8/8) |
| AgaveBam | C | 0.95 [0.90, 0.98] (144/152) |
| AgaveBam | pilot | 1.00 [0.86, 1.00] (24/24) |
| HarmonicAgave | C | 0.91 [0.86, 0.95] (139/152) |
| HarmonicAgave | pilot | 1.00 [0.86, 1.00] (24/24) |
| HarmonicAgave | smoke | 1.00 [0.63, 1.00] (8/8) |
| HarmonicFiredancer | C | 0.99 [0.96, 1.00] (151/152) |
| HarmonicFiredancer | pilot | 1.00 [0.86, 1.00] (24/24) |
| Firedancer | C | 1.00 [0.97, 1.00] (144/144) |
| Firedancer | pilot | 1.00 [0.86, 1.00] (24/24) |
| FireBAM | C | 0.72 [0.53, 0.86] (23/32) |
| FireBAM | pilot | 0.67 [0.35, 0.90] (8/12) |
| Rakurai | C | 1.00 [0.86, 1.00] (24/24) |
| Rakurai | pilot | 1.00 [0.74, 1.00] (12/12) |
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
| JitoLabs | — | 1.00 [0.03, 1.00] (1/1) | — | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.97] (0/1) |
| AgaveBam | — | 1.00 [0.16, 1.00] (2/2) | 0.50 [0.01, 0.99] (1/2) | — | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.71] (0/3) |
| HarmonicAgave | 1.00 [0.03, 1.00] (1/1) | 1.00 [0.03, 1.00] (1/1) | 1.00 [0.03, 1.00] (1/1) | 0.67 [0.09, 0.99] (2/3) | — | — |
| HarmonicFiredancer | — | 1.00 [0.16, 1.00] (2/2) | 1.00 [0.40, 1.00] (4/4) | 0.33 [0.01, 0.91] (1/3) | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.84] (0/2) |
| Firedancer | 1.00 [0.03, 1.00] (1/1) | 1.00 [0.16, 1.00] (2/2) | 0.33 [0.01, 0.91] (1/3) | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.97] (0/1) | 0.00 [0.00, 0.97] (0/1) |
| FireBAM | — | — | — | — | 0.00 [0.00, 0.97] (0/1) | — |
| Rakurai | — | — | — | — | 0.00 [0.00, 0.97] (0/1) | — |
| Raiku | — | — | — | 0.00 [0.00, 0.97] (0/1) | — | — |
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
| T1 | Agave | 11 | 1.00 [0.92, 1.00] (44/44) | 22 | 0.75 [0.35, 0.97] (6/8) | 0.20 [0.01, 0.72] (1/5) |
| T2 | JitoLabs | 24 | 1.00 [0.96, 1.00] (96/96) | 8 | 0.33 [0.01, 0.91] (1/3) | 0.33 [0.01, 0.91] (1/3) |
| T3 | JitoLabs | 22 | 1.00 [0.96, 1.00] (88/88) | 42 | 0.88 [0.62, 0.98] (14/16) | 0.57 [0.18, 0.90] (4/7) |
| T4 | AgaveBam | 22 | 1.00 [0.96, 1.00] (88/88) | 44 | 0.37 [0.16, 0.62] (7/19) | 0.50 [0.21, 0.79] (6/12) |
| T5 | AgaveBam | 22 | 0.91 [0.83, 0.96] (80/88) | 40 | 0.29 [0.10, 0.56] (5/17) | 0.64 [0.31, 0.89] (7/11) |
| T6 | HarmonicAgave | 24 | 0.99 [0.94, 1.00] (95/96) | 41 | 0.62 [0.35, 0.85] (10/16) | 0.62 [0.24, 0.91] (5/8) |
| T7 | HarmonicAgave | 22 | 0.86 [0.77, 0.93] (76/88) | 38 | 0.62 [0.35, 0.85] (10/16) | 1.00 [0.63, 1.00] (8/8) |
| T8 | HarmonicFiredancer | 22 | 0.99 [0.94, 1.00] (87/88) | 39 | 0.62 [0.32, 0.86] (8/13) | 0.78 [0.40, 0.97] (7/9) |
| T9 | HarmonicFiredancer | 22 | 1.00 [0.96, 1.00] (88/88) | 44 | 0.44 [0.20, 0.70] (7/16) | 0.58 [0.28, 0.85] (7/12) |
| T10 | Firedancer | 22 | 1.00 [0.96, 1.00] (88/88) | 44 | 0.12 [0.01, 0.36] (2/17) | 0.69 [0.39, 0.91] (9/13) |
| T11 | Firedancer | 20 | 1.00 [0.95, 1.00] (80/80) | 38 | 0.25 [0.07, 0.52] (4/16) | 0.60 [0.26, 0.88] (6/10) |
| T12 | FireBAM | 11 | 0.70 [0.55, 0.83] (31/44) | 9 | 1.00 [0.16, 1.00] (2/2) | 0.00 [0.00, 0.97] (0/1) |
| T13 | Rakurai | 9 | 1.00 [0.90, 1.00] (36/36) | 18 | 0.67 [0.22, 0.96] (4/6) | 0.50 [0.01, 0.99] (1/2) |
| X1 | Raiku | 5 | 1.00 [0.83, 1.00] (20/20) | 10 | 0.33 [0.01, 0.91] (1/3) | 0.75 [0.19, 0.99] (3/4) |
<!-- /table:per_target -->

Families with two validators: intraclass correlation of in-turn landing and of F across the two (ANOVA estimator) and
a χ² test of equal in-turn landing (exploratory):

<!-- table:operators -->
*Source: results/explore.json → operators (exploratory).*

| family | target | in the target's turn | F | ICC in-turn | ICC F | χ² p (equal in-turn landing) |
|---|---|---|---|---|---|---|
| AgaveBam | T4 | 1.00 [0.96, 1.00] (88/88) | 0.37 [0.16, 0.62] (7/19) | 0.080 | -0.046 | 0.011 |
| AgaveBam | T5 | 0.91 [0.83, 0.96] (80/88) | 0.29 [0.10, 0.56] (5/17) | 0.080 | -0.046 | 0.011 |
| Firedancer | T10 | 1.00 [0.96, 1.00] (88/88) | 0.12 [0.01, 0.36] (2/17) | — | -0.004 | — |
| Firedancer | T11 | 1.00 [0.95, 1.00] (80/80) | 0.25 [0.07, 0.52] (4/16) | — | -0.004 | — |
| HarmonicAgave | T6 | 0.99 [0.94, 1.00] (95/96) | 0.62 [0.35, 0.85] (10/16) | 0.104 | -0.067 | 0.002 |
| HarmonicAgave | T7 | 0.86 [0.77, 0.93] (76/88) | 0.62 [0.35, 0.85] (10/16) | 0.104 | -0.067 | 0.002 |
| HarmonicFiredancer | T8 | 0.99 [0.94, 1.00] (87/88) | 0.62 [0.32, 0.86] (8/13) | -0.000 | -0.009 | 1.000 |
| HarmonicFiredancer | T9 | 1.00 [0.96, 1.00] (88/88) | 0.44 [0.20, 0.70] (7/16) | -0.000 | -0.009 | 1.000 |
| JitoLabs | T2 | 1.00 [0.96, 1.00] (96/96) | 0.33 [0.01, 0.91] (1/3) | — | 0.455 | — |
| JitoLabs | T3 | 1.00 [0.96, 1.00] (88/88) | 0.88 [0.62, 0.98] (14/16) | — | 0.455 | — |
<!-- /table:operators -->

Transactions between the two members of a pair that landed in one block (exploratory):

<!-- table:between -->
*Source: results/explore.json → between (exploratory).*

| family | pairs in one block | adjacent | transactions between (median) | p90 |
|---|---|---|---|---|
| Agave | 21 | 0.57 [0.34, 0.78] (12/21) | 0 | 24 |
| AgaveBam | 81 | 0.09 [0.04, 0.17] (7/81) | 4 | 44 |
| FireBAM | 15 | 0.40 [0.16, 0.68] (6/15) | 1 | 28 |
| Firedancer | 82 | 0.01 [0.00, 0.07] (1/82) | 12 | 44 |
| HarmonicAgave | 84 | 0.24 [0.15, 0.34] (20/84) | 4 | 24 |
| HarmonicFiredancer | 85 | 0.19 [0.11, 0.29] (16/85) | 3 | 33 |
| JitoLabs | 87 | 0.39 [0.29, 0.50] (34/87) | 2 | 21 |
| Raiku | 10 | 0.80 [0.44, 0.97] (8/10) | 0 | 7 |
| Rakurai | 18 | 0.67 [0.41, 0.87] (12/18) | 0 | 18 |
<!-- /table:between -->

The v1 arm (exploratory, PREREGISTRATION §3 rule 2):

<!-- table:v1 -->
*Source: results/results.json → descriptive (v1 arm).*

|  | V_EQ: v1 member first (equal keys) | V_HI: v1 high-key member first |
|---|---|---|
| in the target's turn: v1 probes vs legacy probes of the same quads | 0.95 [0.86, 0.99] (57/60) | 0.94 [0.86, 0.98] (66/70) |
| Agave | 1.00 [0.03, 1.00] (1/1) | 1.00 [0.16, 1.00] (2/2) |
| JitoLabs | 0.33 [0.01, 0.91] (1/3) | 1.00 [0.29, 1.00] (3/3) |
| AgaveBam | 0.00 [0.00, 0.84] (0/2) | 1.00 [0.40, 1.00] (4/4) |
| HarmonicAgave | 0.75 [0.19, 0.99] (3/4) | 1.00 [0.40, 1.00] (4/4) |
| HarmonicFiredancer | 0.38 [0.09, 0.76] (3/8) | 0.80 [0.28, 0.99] (4/5) |
| Firedancer | 0.60 [0.15, 0.95] (3/5) | 0.67 [0.09, 0.99] (2/3) |
| FireBAM | — | 1.00 [0.16, 1.00] (2/2) |
| Rakurai | 0.00 [0.00, 0.97] (0/1) | 1.00 [0.16, 1.00] (2/2) |
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
| campaign | probe | 872 | 847 | 4,379,666 |
| pilot | probe | 156 | 156 | 950,676 |
| setup | fund | 1 | 1 | 5,000 |
| setup | memo | 1 | 1 | 5,000 |
| smoke | probe | 16 | 16 | 82,384 |
| total |  | 1046 | 1021 | 5,422,726 (0.005423 SOL) |
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
| fees paid | 5,422,726 lamports (0.005423 SOL) of the 30,000,000 cap |
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
| claims checked offline | 1032 |
| failures | 0 |
| pair orders recomputed | — |
| result | PASS |
<!-- /table:verify -->

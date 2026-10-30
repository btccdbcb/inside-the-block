"""Render docs/POWER.md from results/power.json (no hand-typed numbers)."""

from __future__ import annotations

import json
from pathlib import Path

from .analysis import FAMILIES

INTRO = """# Power and false-positive rate of the pre-registered analysis

Produced by `itb-analysis power-report` from `results/power.json`, which `itb-analysis power` produced
(`docker compose --profile power up` reruns both). Each scenario simulates {n_sim} complete campaigns on the frozen
design (`design/design.jsonl`, the rows of sessions A–D), analyses each with the same code as the real data
(`analysis/itb_analysis/analysis.py`), and counts how often each test rejects after Holm. The randomisation tests use
{n_perm} permutations here (10,000 in the real analysis).

## How the synthetic data are made (`analysis/itb_analysis/sim.py`)

These are assumptions, chosen before any mainnet data, to size the study. They are not findings.

- Send time: the planned offset τ plus a sender error, normal(0, 10) ms. The slot before the turn and the turn's slots
  last normal(200, 50) ms each (at least 100 ms). Both from the mainnet dry run of 2026-10-29: first-shred intervals
  had median 200 ms (p10 148, p90 290), and the achieved τ was within −19..+7 ms of plan (p10..p90 per τ level).
- Whole-quad loss: 8 % (QUIC refused or blockhash stale). The mainnet dry run skipped quads mainly for QUIC reasons.
- Per-probe landing in the target's turn: `land` for τ ≤ +400 ms, `land_late` after that; leakage to the preceding
  leader for τ ≤ 0 with probability `leak`.
- Pair order:
  - pre-turn: the key-preferred member first with probability `sorted` (R) or `key` (C pairs);
  - mid-turn: first-sent first with probability F at equal keys;
  - when the higher key was sent Δt later, it still lands first with probability
    0.5 + (rrr0 − 0.5) · exp(−Δt / horizon).
- Load: block cost units normal(30M, 10M), clipped to 5–50M (the block limit at 200 ms slots, Agave v4.3.2
  `runtime/src/slot_params.rs`). The R-pair logit shifts by β per 10M CU.
- Eligibility: both members in the turn, τ within tolerance (95 %), Δt within tolerance (97 %).

| Scenario | Ground truth (default; family-specific values in brackets) |
|---|---|
{truth_rows}

## Familywise false-positive rate

Under `null`, no family has any rule. A primary test rejected in **{null_any}** of the campaigns, against a nominal 5 %.

## Power of the primary tests (share of campaigns where the test rejects after Holm)

{primary_table}

## Secondary tests (share of campaigns rejecting after Holm, secondary family)

{secondary_table}

## Load test (P3): share of campaigns where "no load effect" (TOST ±0.4) is declared

{equiv_table}

## Eligible equal-key pairs per family (mean per campaign)

{n_table}

## What this means for the claims

{reading}
"""


def _pct(x: float) -> str:
    return f"{100 * x:.0f} %"


def _truth_row(name: str, sc: dict) -> str:
    d = sc["truth"]["default"]
    extra = "; ".join(f"{fam}: " + ", ".join(f"{k} {v}" for k, v in vals.items())
                      for fam, vals in sc["truth"].items() if fam != "default")
    base = ", ".join(f"{k} {v}" for k, v in d.items())
    return f"| {name} | {base}{' [' + extra + ']' if extra else ''} |"


def _table(rows: list[str], cols: list[str], cell) -> str:
    out = ["| | " + " | ".join(cols) + " |", "|---" * (len(cols) + 1) + "|"]
    for r in rows:
        out.append(f"| {r} | " + " | ".join(cell(r, c) for c in cols) + " |")
    return "\n".join(out)


def render(power_json: Path, out: Path) -> str:
    r = json.loads(Path(power_json).read_text())
    scen = list(r)
    any_s = next(iter(r.values()))
    primary_keys = [f"P1:{f}" for f in FAMILIES] + [f"P2:{f}" for f in FAMILIES] + ["P3"]
    secondary_keys = sorted({k for v in r.values() for k in v["secondary"]})
    reading = []
    exp = r.get("expected")
    if exp:
        big = [f for f in FAMILIES if exp["eligible_eq_pairs_mean"].get(f, 0) >= 40]
        small = [f for f in FAMILIES if f not in big]
        reading.append(f"- Families with enough turns ({', '.join(big)}) give about "
                       f"{min(exp['eligible_eq_pairs_mean'][f] for f in big):.0f}–"
                       f"{max(exp['eligible_eq_pairs_mean'][f] for f in big):.0f} eligible equal-key pairs. "
                       "Their F estimates carry 95 % intervals of roughly "
                       f"±{1.96 * (0.25 / min(exp['eligible_eq_pairs_mean'][f] for f in big)) ** 0.5:.2f} "
                       "(1.96 × √(0.25 / n) at the smallest n), whether or not a test rejects.")
        reading.append(f"- The low-turn families ({', '.join(small)}) have too few turns in the session windows for "
                       "any test to reject at this size. Their results are reported as estimates with wide intervals, "
                       "and a non-rejection there is not evidence of no rule.")
    if "load" in r and "null" in r and exp:
        reading.append("- Load (P3). With no load effect, the pooled test declares \"no load effect\" in "
                       f"{_pct(r['null']['load_equivalent']['pooled'])} of campaigns when ordering is a coin flip "
                       f"(null) but only {_pct(exp['load_equivalent']['pooled'])} when ordering is mostly by key "
                       "(expected): near P = 0.9 a logit slope is poorly determined. A true slope of 0.3 per 10M CU "
                       f"is detected in {_pct(r['load']['primary']['P3'])}. \"Inconclusive\" is therefore a likely "
                       "load verdict, and REPORT.md will say so rather than read it as no effect.")
    text = INTRO.format(
        n_sim=any_s["n_sim"], n_perm=any_s["n_perm"],
        truth_rows="\n".join(_truth_row(s, r[s]) for s in scen),
        null_any=_pct(r["null"]["any_primary_rejection"]) if "null" in r else "n/a",
        primary_table=_table(primary_keys, scen, lambda k, s: _pct(r[s]["primary"].get(k, 0.0))),
        secondary_table=_table(secondary_keys, scen, lambda k, s: _pct(r[s]["secondary"].get(k, 0.0))),
        equiv_table=_table(["pooled"], scen, lambda k, s: _pct(r[s]["load_equivalent"].get(k, 0.0))),
        n_table=_table(FAMILIES, scen, lambda f, s: f"{r[s]['eligible_eq_pairs_mean'].get(f, 0):.0f}"),
        reading="\n".join(reading),
    )
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    Path(out).write_text(text)
    return text

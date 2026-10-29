"""Power simulation (docs/POWER.md): synthetic pairs and probes tables with the columns outcomes.py writes, built from
the real design rows, under ground-truth scenarios. The analysis code in analysis.py runs on them unchanged.

What is simulated per fired quad:
  * the send offset: off0 = tau - SLOT_MS + clock error (normal, mean -10 ms, sd 40 ms; replaced by the dry-run
    distribution when it is known),
  * whole-quad loss (QUIC eviction, stale blockhash) with probability QUAD_LOSS,
  * per-probe landing in the target's turn with probability land(tau) (scenario), else a leak to the preceding leader
    with probability leak(tau), else not landed,
  * the order of each pair landed together: pre-turn backlog -> key order with probability `sorted`, otherwise a coin;
    mid-turn -> first-sent first with probability F (equal keys) or, when the higher key was sent dt later, the higher
    key first with probability rrr0 * exp(-dt / horizon); a load term beta * (load - 3) on the logit of the R pairs,
  * block load ~ normal(30M, 10M) CU, clipped to [5M, 60M].
"""

from __future__ import annotations

import math

import numpy as np
import pandas as pd

from .outcomes import turn_bin

SLOT_MS = 400.0
QUAD_LOSS = 0.08
CLOCK_MEAN, CLOCK_SD = -10.0, 40.0
TAU_OK = 0.95
DT_OK = 0.97


def _logit(p):
    return math.log(p / (1 - p))


def _expit(x):
    return 1 / (1 + math.exp(-x))


# A scenario maps a client family to its ground truth. Missing keys fall back to `default`.
SCENARIOS: dict[str, dict] = {
    # no family has any rule: every primary test is a true null (measures the familywise false-positive rate)
    "null": {"default": {"F": 0.5, "sorted": 0.5, "rrr0": 0.5, "horizon": 1.0, "key": 0.5, "beta": 0.0,
                         "land": 0.85, "land_late": 0.85, "leak": 0.0}},
    # what the plan expects: Agave-line first-sent-first, Firedancer later-first, key order in the backlog, a race
    # horizon of a few ms, landing falling for sends after the turn starts, no load effect
    "expected": {
        "default": {"F": 0.5, "sorted": 0.9, "rrr0": 0.9, "horizon": 5.0, "key": 0.9, "beta": 0.0,
                    "land": 0.85, "land_late": 0.6, "leak": 0.03},
        "Agave": {"F": 0.7}, "JitoLabs": {"F": 0.7}, "Firedancer": {"F": 0.35},
    },
    # as expected, with a modest load effect everywhere (logit +0.3 per 10M CU)
    "load": {"default": {"F": 0.5, "sorted": 0.9, "rrr0": 0.9, "horizon": 5.0, "key": 0.9, "beta": 0.3,
                         "land": 0.85, "land_late": 0.6, "leak": 0.03}},
    # weak effects at the edge of detectability
    "weak": {
        "default": {"F": 0.5, "sorted": 0.8, "rrr0": 0.7, "horizon": 2.0, "key": 0.65, "beta": 0.0,
                    "land": 0.8, "land_late": 0.7, "leak": 0.02},
        "Agave": {"F": 0.62}, "JitoLabs": {"F": 0.62}, "Firedancer": {"F": 0.4},
    },
}


def truth(scenario: dict, family: str) -> dict:
    return {**scenario["default"], **scenario.get(family, {})}


def simulate(design: list[dict], targets: dict[str, dict], scenario: dict, seed: int,
             sessions=("A", "B", "C", "D")) -> tuple[pd.DataFrame, pd.DataFrame]:
    rng = np.random.default_rng(seed)
    pairs, probes = [], []
    for row in design:
        if row["session"] not in sessions:
            continue
        t = targets[row["target_slot"]]
        fam = t["expected_client"]
        g = truth(scenario, fam)
        tau = row["tau_ms"]
        off0 = tau - SLOT_MS + rng.normal(CLOCK_MEAN, CLOCK_SD)
        bin_ = turn_bin(off0, SLOT_MS, 2 * SLOT_MS)
        tau_ok = rng.random() < TAU_OK
        lost = rng.random() < QUAD_LOSS
        load = float(np.clip(rng.normal(30e6, 10e6), 5e6, 60e6))
        p_land = g["land"] if tau <= 400 else g["land_late"]
        p_leak = g["leak"] if tau <= 0 else 0.0
        common = {"quad_id": row["quad_id"], "target": row["target_slot"], "family": fam, "tau_ms": tau,
                  "turn_bin": bin_}
        landed = {}
        for role in ("main_pref", "main_other", "control_pref", "control_other"):
            u = rng.random()
            if lost:
                where = None
            elif u < p_land:
                where = "target"
            elif u < p_land + p_leak:
                where = "preceding"
            else:
                where = None
            landed[role] = where
            probes.append({**common, "role": role, "in_target_turn": where == "target", "landed_with": where,
                           "slot_of_turn": int(rng.integers(0, 4)) if where == "target" else None})
        for kind in ("main", "control"):
            ptype = row["main_type"] if kind == "main" else row["control_type"]
            dt = row["dt_ms"] if kind == "main" else row["control_gap_ms"]
            first = row["main_first"] if kind == "main" else row["control_first"]
            pref_sent_first = (first == "pref") if dt > 0 else bool(rng.random() < 0.5)
            both = landed[f"{kind}_pref"] == "target" and landed[f"{kind}_other"] == "target"
            pref_first = None
            if both:
                pref_first = bool(rng.random() < _p_pref_first(g, ptype, dt, pref_sent_first, bin_, load))
            fsf = None if pref_first is None else (pref_first == pref_sent_first)
            eligible = both and tau_ok and rng.random() < DT_OK
            pairs.append({**common, "pair": kind, "pair_type": ptype, "dt_planned": dt,
                          "pref_sent_first": pref_sent_first, "pref_landed": landed[f"{kind}_pref"] is not None,
                          "other_landed": landed[f"{kind}_other"] is not None, "both_in_turn": both,
                          "pref_first": pref_first, "first_sent_first": fsf, "load_cu": load if both else None,
                          "eligible": eligible})
    return pd.DataFrame(pairs), pd.DataFrame(probes)


def _p_pref_first(g: dict, ptype: str, dt: float, pref_sent_first: bool, bin_: str | None, load: float) -> float:
    if ptype in ("EQ", "V_EQ"):
        # equal keys: the label is arbitrary, only the send order matters
        return g["F"] if pref_sent_first else 1 - g["F"]
    key = g["sorted"] if ptype in ("R", "R_IND", "V_HI") else g["key"]
    if bin_ == "pre":
        p = key
    elif pref_sent_first:
        p = max(key, g["F"])
    else:
        p = 0.5 + (g["rrr0"] - 0.5) * math.exp(-dt / g["horizon"]) if dt > 0 else key
    if ptype == "R" and g["beta"]:
        p = _expit(_logit(min(max(p, 0.01), 0.99)) + g["beta"] * (load / 1e7 - 3))
    return p

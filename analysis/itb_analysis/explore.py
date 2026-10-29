"""Exploratory analyses: the descriptive items of PREREGISTRATION §5 that need more than a proportion. Nothing here is
tested for a verdict, and REPORT.md labels every number from it as exploratory. This module is not one of the three
confirmatory files whose hashes the design records.

Inputs: results/pairs.csv and results/probes.csv (outcomes.py) and the block excerpts under data/raw/<run>/blocks/.
Output: results/explore.json.

  mechanism      the race-reversal cells RRR(dt) per family, fitted by four models of how a leader orders two
                 transactions that reach it dt ms apart (maximum likelihood, compared by AIC):
                   arrival   arrival order with jitter: RRR = Phi(-dt / (s * sqrt 2))
                   drain     a priority queue drained continuously: RRR = a * exp(-dt / h)
                   batch     batches of length B at a random phase: RRR = a * max(0, 1 - dt / B)
                   constant  no dependence on dt: RRR = a
  insertion      where in-turn probes sit in their block (index / (transactions - 1)) by tau level and slot of turn
  hour           in-turn landing by session and by 4-hour UTC block, per family
  operators      families with two or more targets: per-target in-turn landing and F, and a chi-square test of equal
                 in-turn landing across the family's targets (descriptive p)
  between        pairs whose members landed in the same block: how many other transactions sit between them
  whole_block    every archived block of a target's turn (leader = target): Kendall's tau between position and the
                 requested CU price, and between position and an approximate scheduler priority, for all transactions
                 in the block (+1 = sorted high to low); and the block load (cost units, transactions)

The approximate scheduler priority is reward / (cost + 1) with reward = priority fee + half the signature fee (the
leader's share) and cost = 720 per signature + 300 per writable account + the requested CU limit + 8 CU per 32 KiB of
the requested loaded-data limit (64 MiB when not set). It leaves out the instruction-data cost, which the excerpts do
not hold, and skips transactions that set no CU limit (their default limit depends on their instructions).
"""

from __future__ import annotations

import gzip
import json
import math
from pathlib import Path

import numpy as np
import pandas as pd
from scipy import optimize
from scipy import stats as sps

from .analysis import EXTENDED, FAMILIES, MIDTURN_BINS, MIN_CELL, TAU_LEVELS, _bool_series, _f, _prop

PANELS = FAMILIES + EXTENDED
SIG_FEE = 5000
PAGE = 32 * 1024
DEFAULT_LOADED = 64 * 1024 * 1024


# --- blocks -----------------------------------------------------------------------------------------------------------

def load_blocks(raw: Path) -> dict[int, dict]:
    """Every full block excerpt under raw/<run>/blocks/, by slot."""
    out = {}
    root = Path(raw)
    if not root.exists():
        return out
    for p in sorted(root.glob("*/blocks/*.json.gz")):
        ex = json.loads(gzip.decompress(p.read_bytes()))
        if ex.get("mode") == "full":
            out[ex["slot"]] = ex
    return out


def _scheduler_priority(ex: dict) -> list[float | None]:
    out = []
    for n_sig, n_w, limit, price, loaded in zip(ex["n_sig"], ex["n_writable"], ex["cu_limit"], ex["cu_price"],
                                                ex["loaded_data"], strict=True):
        if n_sig is None or limit is None or price is None:
            out.append(None)
            continue
        reward = -(-limit * price // 1_000_000) + SIG_FEE // 2 * n_sig
        cost = 720 * n_sig + 300 * n_w + limit + 8 * -(-(loaded or DEFAULT_LOADED) // PAGE)
        out.append(reward / (cost + 1))
    return out


def _sortedness(keys: list) -> tuple[float | None, int]:
    """Kendall's tau-b between position and key, signed so that +1 means highest key first."""
    pos = [i for i, k in enumerate(keys) if k is not None]
    if len(pos) < 10:
        return None, len(pos)
    k = [keys[i] for i in pos]
    if len(set(k)) < 2:
        return None, len(pos)
    tau = sps.kendalltau(pos, k).statistic
    return (None if math.isnan(tau) else -float(tau)), len(pos)


def _quantiles(x) -> dict:
    x = [v for v in x if v is not None and not (isinstance(v, float) and math.isnan(v))]
    if not x:
        return {"n": 0}
    q = np.quantile(x, [0.1, 0.25, 0.5, 0.75, 0.9])
    return {"n": len(x), "p10": _f(q[0]), "p25": _f(q[1]), "median": _f(q[2]), "p75": _f(q[3]), "p90": _f(q[4])}


def whole_block(probes: pd.DataFrame, blocks: dict[int, dict]) -> dict:
    if not len(probes) or not blocks:
        return {}
    turns = probes.drop_duplicates("quad_id")[["slot0", "family", "identity"]].drop_duplicates()
    rows = []
    for t in turns.itertuples():
        for k in range(4):
            ex = blocks.get(int(t.slot0) + k)
            if ex is None or ex.get("leader") != t.identity or "cu_price" not in ex:
                continue
            price, n_price = _sortedness([p if p else None for p in ex["cu_price"]])
            prio, n_prio = _sortedness(_scheduler_priority(ex))
            rows.append({"slot": ex["slot"], "family": t.family, "slot_of_turn": k, "n_tx": ex["n_tx"],
                         "cost_units": ex["sum_cost_units"], "price_tau": price, "n_price": n_price,
                         "priority_tau": prio, "n_priority": n_prio})
    if not rows:
        return {}
    d = pd.DataFrame(rows).drop_duplicates("slot")
    out = {}
    for fam in PANELS:
        f = d[d.family == fam]
        if not len(f):
            continue
        out[fam] = {"blocks": len(f), "n_tx": _quantiles(f.n_tx), "cost_units": _quantiles(f.cost_units),
                    "price_tau": _quantiles(f.price_tau), "priority_tau": _quantiles(f.priority_tau),
                    "by_slot_of_turn": {str(k): {"blocks": len(g), "price_tau": _quantiles(g.price_tau),
                                                 "priority_tau": _quantiles(g.priority_tau)}
                                        for k, g in f.groupby("slot_of_turn")}}
    return out


# --- pairs and probes -------------------------------------------------------------------------------------------------

def _rrr_cells(pairs: pd.DataFrame, fam: str) -> list[tuple[float, int, int]]:
    r = pairs[(pairs.pair_type == "R") & _bool_series(pairs.eligible) & pairs.turn_bin.isin(MIDTURN_BINS)
              & ~_bool_series(pairs.pref_sent_first) & (pairs.family == fam)]
    return [(float(dt), int(_bool_series(g.pref_first).sum()), len(g)) for dt, g in r.groupby("dt_planned")]


MODELS = {
    # name: (parameter names, bounds, starting points, RRR(dt, params))
    "arrival": (["s"], [(0.05, 500.0)], [[1.0], [10.0], [100.0]],
                lambda dt, p: sps.norm.cdf(-dt / (p[0] * math.sqrt(2)))),
    "drain": (["a", "h"], [(0.0, 1.0), (0.05, 1000.0)], [[0.9, 2.0], [0.7, 20.0], [0.5, 200.0]],
              lambda dt, p: p[0] * np.exp(-dt / p[1])),
    "batch": (["a", "B"], [(0.0, 1.0), (0.5, 1000.0)], [[0.9, 5.0], [0.7, 50.0], [0.5, 400.0]],
              lambda dt, p: p[0] * np.clip(1 - dt / p[1], 0, None)),
    "constant": (["a"], [(0.0, 1.0)], [[0.5]], lambda dt, p: np.full_like(dt, p[0], dtype=float)),
}


def _loglik(pr, k, n):
    pr = np.clip(pr, 1e-9, 1 - 1e-9)
    return float(np.sum(k * np.log(pr) + (n - k) * np.log(1 - pr)))


def fit_mechanisms(cells: list[tuple[float, int, int]]) -> dict:
    dt = np.array([c[0] for c in cells], dtype=float)
    k = np.array([c[1] for c in cells], dtype=float)
    n = np.array([c[2] for c in cells], dtype=float)
    fits = {}
    for name, (pnames, bounds, starts, f) in MODELS.items():
        best = None
        for x0 in starts:
            r = optimize.minimize(lambda p, f=f: -_loglik(f(dt, p), k, n), x0, bounds=bounds, method="L-BFGS-B")
            if best is None or r.fun < best.fun:
                best = r
        ll = -float(best.fun)
        fits[name] = {"params": {p: _f(v) for p, v in zip(pnames, best.x, strict=True)}, "loglik": _f(ll),
                      "aic": _f(2 * len(pnames) - 2 * ll)}
    amin = min(v["aic"] for v in fits.values())
    w = {m: math.exp(-(v["aic"] - amin) / 2) for m, v in fits.items()}
    for m, v in fits.items():
        v["akaike_weight"] = _f(w[m] / sum(w.values()))
    return {"best": min(fits, key=lambda m: fits[m]["aic"]), "models": fits}


def mechanism(pairs: pd.DataFrame) -> dict:
    out = {}
    for fam in PANELS:
        cells = _rrr_cells(pairs, fam)
        n = sum(c[2] for c in cells)
        res = {"cells": {str(int(dt)): _prop(k, m) for dt, k, m in cells}, "n": n}
        if n < MIN_CELL or len(cells) < 3:
            res["fit"] = "insufficient data"
        else:
            res["fit"] = fit_mechanisms(cells)
        out[fam] = res
    return out


def insertion(probes: pd.DataFrame, blocks: dict[int, dict]) -> dict:
    p = probes[_bool_series(probes.in_target_turn)].copy()
    if not len(p) or not blocks:
        return {}
    n_tx = p.slot.map(lambda s: blocks.get(int(s), {}).get("n_tx"))
    p["rel"] = [i / (m - 1) if m and m > 1 else None for i, m in zip(p.index_in_block, n_tx, strict=True)]
    out = {}
    for fam in PANELS:
        f = p[p.family == fam]
        if not len(f):
            continue
        out[fam] = {"by_tau": {str(t): _quantiles(f[f.tau_ms == t].rel) for t in TAU_LEVELS},
                    "by_slot_of_turn": {str(int(k)): _quantiles(g.rel) for k, g in f.groupby("slot_of_turn")}}
    return out


def hour(probes: pd.DataFrame) -> dict:
    if not len(probes) or "fired_utc" not in probes:
        return {}
    p = probes.assign(h4=pd.to_datetime(probes.fired_utc, utc=True, format="ISO8601").dt.hour // 4 * 4,
                      land=_bool_series(probes.in_target_turn))
    out = {}
    for fam in ["all"] + PANELS:
        f = p if fam == "all" else p[p.family == fam]
        if not len(f):
            continue
        out[fam] = {"by_session": {s: _prop(int(g.land.sum()), len(g)) for s, g in f.groupby("session")},
                    "by_utc_4h": {f"{int(h):02d}": _prop(int(g.land.sum()), len(g)) for h, g in f.groupby("h4")}}
    return out


def operators(pairs: pd.DataFrame, probes: pd.DataFrame) -> dict:
    out = {}
    el = pairs[_bool_series(pairs.eligible) & (pairs.pair_type == "EQ")] if len(pairs) else pairs
    for fam in PANELS:
        f = probes[probes.family == fam] if len(probes) else probes
        targets = sorted(f.target.unique()) if len(f) else []
        if len(targets) < 2:
            continue
        rows, table = {}, []
        for t in targets:
            g = f[f.target == t]
            k = int(_bool_series(g.in_target_turn).sum())
            e = el[el.target == t]
            rows[t] = {"in_turn": _prop(k, len(g)), "F": _prop(int(_bool_series(e.first_sent_first).sum()), len(e))}
            table.append([k, len(g) - k])
        res = {"targets": rows}
        tab = np.array(table)
        if tab.sum(axis=0).min() > 0 and (tab.sum(axis=1) > 0).all():
            chi2, pval, dof, _ = sps.chi2_contingency(tab)
            res["in_turn_homogeneity"] = {"chi2": _f(chi2), "dof": int(dof), "p_value": _f(pval)}
        out[fam] = res
    return out


def between(pairs: pd.DataFrame, probes: pd.DataFrame) -> dict:
    if not len(pairs) or not len(probes) or "index_in_block" not in probes:
        return {}
    idx = probes.set_index(["quad_id", "role"]).index_in_block.to_dict()
    both = pairs[_bool_series(pairs.same_block) & _bool_series(pairs.both_in_turn)]
    gaps = []
    for r in both.itertuples():
        a, b = idx[(r.quad_id, f"{r.pair}_pref")], idx[(r.quad_id, f"{r.pair}_other")]
        gaps.append({"family": r.family, "pair_type": r.pair_type, "gap": abs(int(a) - int(b)) - 1})
    if not gaps:
        return {}
    g = pd.DataFrame(gaps)
    out = {}
    for fam in PANELS:
        f = g[g.family == fam]
        if len(f):
            out[fam] = {"pairs": len(f), "adjacent": _prop(int((f.gap == 0).sum()), len(f)),
                        "gap": _quantiles(f.gap.tolist()),
                        "by_pair_type": {t: _quantiles(h.gap.tolist()) for t, h in f.groupby("pair_type")}}
    return out


def run(pairs: pd.DataFrame, probes: pd.DataFrame, blocks: dict[int, dict]) -> dict:
    if not len(pairs):
        return {}
    return {"mechanism": mechanism(pairs), "insertion": insertion(probes, blocks),
            "hour": hour(probes), "operators": operators(pairs, probes), "between": between(pairs, probes),
            "whole_block": whole_block(probes, blocks)}

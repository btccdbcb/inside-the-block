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
                 with a parametric bootstrap of the best model's parameters and a recovery check (how often the
                 generating model wins on data simulated at the observed gaps and counts)
  insertion      where in-turn probes sit in their block (index / (transactions - 1)) by tau level and slot of turn
  hour           in-turn landing by session and by 4-hour UTC block, per family
  operators      families with two or more targets: per-target in-turn landing and F, the intraclass correlation of
                 each across the family's targets (ANOVA estimator), and a chi-square test of equal in-turn landing
                 (descriptive p)
  between        pairs whose members landed in the same block: how many other transactions sit between them
  whole_block    every archived block of a target's turn (leader = target): Kendall's tau between position and the
                 requested CU price, and between position and an approximate scheduler priority, for all transactions
                 in the block (+1 = sorted high to low); and the block load (cost units, transactions)
  send_order     S1 and S2 split by whether the preferred member was sent first or second; and P1, S1, S2 on the
                 campaign runs only (without smoke and pilot)

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
from scipy import stats as sps

from .analysis import EXTENDED, FAMILIES, MIDTURN_BINS, MIN_CELL, TAU_LEVELS, _bool_series, _f, _order_tables, _prop

PANELS = FAMILIES + EXTENDED
CAMPAIGN = ("A", "B", "C", "D")
SIG_FEE = 5000
PAGE = 32 * 1024
DEFAULT_LOADED = 64 * 1024 * 1024
N_BOOT = 200


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
    # name: (parameter names, RRR(dt, params)); dt and the parameters broadcast (numpy)
    "arrival": (["s"], lambda dt, p: sps.norm.cdf(-dt / (p[0] * math.sqrt(2)))),
    "drain": (["a", "h"], lambda dt, p: p[0] * np.exp(-dt / p[1])),
    "batch": (["a", "B"], lambda dt, p: p[0] * np.clip(1 - dt / p[1], 0, None)),
    "constant": (["a"], lambda dt, p: np.full(np.shape(dt), p[0], dtype=float)),
}
# Maximum likelihood by grid search in two passes: a coarse grid (share a in steps of 0.025, time scales s, h, B at 81
# log-spaced values from 0.05 to 1,000 ms), then 41 x 41 points around the best coarse point (a to within 0.0013, time
# scale to within 0.6 %). Deterministic, and fast enough for the bootstrap and the recovery check.
A_COARSE = np.linspace(0.0, 1.0, 41)
T_COARSE = np.geomspace(0.05, 1000.0, 81)


def _ll_grid(pr, k, n):
    pr = np.clip(pr, 1e-9, 1 - 1e-9)
    return (k * np.log(pr) + (n - k) * np.log(1 - pr)).sum(axis=-1)


def _around(grid, i, geometric=False):
    lo, hi = grid[max(i - 1, 0)], grid[min(i + 1, len(grid) - 1)]
    return np.geomspace(lo, hi, 41) if geometric else np.linspace(lo, hi, 41)


def _fit_model(name: str, dt, k, n) -> tuple[np.ndarray, float]:
    """Maximum-likelihood parameters and log-likelihood of one model."""
    if name == "constant":
        a = k.sum() / n.sum()
        return np.array([a]), float(_ll_grid(np.full(dt.shape, a), k, n))
    f = MODELS[name][1]
    if name == "arrival":
        grid = T_COARSE
        for _ in range(2):
            ll = _ll_grid(f(dt[None, :], (grid[:, None],)), k, n)
            i = int(np.argmax(ll))
            best = (np.array([grid[i]]), float(ll[i]))
            grid = _around(grid, i, geometric=True)
        return best
    ga, gt = A_COARSE, T_COARSE
    for _ in range(2):
        ll = _ll_grid(f(dt[None, None, :], (ga[:, None, None], gt[None, :, None])), k, n)
        i, j = np.unravel_index(int(np.argmax(ll)), ll.shape)
        best = (np.array([ga[i], gt[j]]), float(ll[i, j]))
        ga, gt = _around(ga, i), _around(gt, j, geometric=True)
    return best


def _aic_best(dt, k, n) -> str:
    return min(MODELS, key=lambda m: 2 * len(MODELS[m][0]) - 2 * _fit_model(m, dt, k, n)[1])


def fit_mechanisms(cells: list[tuple[float, int, int]], n_boot: int = 0, seed: int = 0) -> dict:
    """Fit every model; with n_boot > 0 also (1) a parametric bootstrap of the best model's parameters (percentile
    95 % intervals) and (2) a recovery check: data simulated from each fitted model at the observed gaps and counts,
    refitted, and the share of replicates in which the generating model has the lowest AIC."""
    dt = np.array([c[0] for c in cells], dtype=float)
    k = np.array([c[1] for c in cells], dtype=float)
    n = np.array([c[2] for c in cells], dtype=float)
    fits, xs = {}, {}
    for name, (pnames, _) in MODELS.items():
        x, ll = _fit_model(name, dt, k, n)
        xs[name] = x
        fits[name] = {"params": {p: _f(v) for p, v in zip(pnames, x, strict=True)}, "loglik": _f(ll),
                      "aic": _f(2 * len(pnames) - 2 * ll)}
    amin = min(v["aic"] for v in fits.values())
    w = {m: math.exp(-(v["aic"] - amin) / 2) for m, v in fits.items()}
    for m, v in fits.items():
        v["akaike_weight"] = _f(w[m] / sum(w.values()))
    best = min(fits, key=lambda m: fits[m]["aic"])
    out = {"best": best, "models": fits}
    if n_boot:
        rng = np.random.default_rng(seed)
        nn = n.astype(int)

        def draw(m):
            return rng.binomial(nn, np.clip(MODELS[m][1](dt, xs[m]), 0, 1)).astype(float)

        boot = np.array([_fit_model(best, dt, draw(best), n)[0] for _ in range(n_boot)])
        out["bootstrap"] = {"n": n_boot, "ci95": {p: [_f(np.quantile(boot[:, i], 0.025)),
                                                      _f(np.quantile(boot[:, i], 0.975))]
                                                  for i, p in enumerate(MODELS[best][0])}}
        n_rec = max(20, n_boot // 2)
        out["recovery"] = {"n": n_rec, "share_correct": {
            m: _f(sum(_aic_best(dt, draw(m), n) == m for _ in range(n_rec)) / n_rec) for m in MODELS}}
    return out


def mechanism(pairs: pd.DataFrame, n_boot: int = N_BOOT, seed: int = 0) -> dict:
    out = {}
    for fam in PANELS:
        cells = _rrr_cells(pairs, fam)
        n = sum(c[2] for c in cells)
        res = {"cells": {str(int(dt)): _prop(k, m) for dt, k, m in cells}, "n": n}
        if n < MIN_CELL or len(cells) < 3:
            res["fit"] = "insufficient data"
        else:
            res["fit"] = fit_mechanisms(cells, n_boot=n_boot, seed=seed)
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


def icc_binary(groups: list[list[bool]]) -> float | None:
    """One-way ANOVA estimator of the intraclass correlation of a 0/1 outcome across groups (here: the targets of one
    family): the share of the outcome's variance that lies between operators rather than within one."""
    groups = [np.asarray(g, dtype=float) for g in groups if len(g)]
    k, sizes = len(groups), np.array([len(g) for g in groups], dtype=float)
    big_n = sizes.sum()
    if k < 2 or big_n - k < 1:
        return None
    means = np.array([g.mean() for g in groups])
    p = float(np.concatenate(groups).mean())
    msb = float((sizes * (means - p) ** 2).sum() / (k - 1))
    msw = float((sizes * means * (1 - means)).sum() / (big_n - k))
    n0 = (big_n - (sizes ** 2).sum() / big_n) / (k - 1)
    den = msb + (n0 - 1) * msw
    return _f((msb - msw) / den) if den > 0 else None


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
        res = {"targets": rows,
               "icc_in_turn": icc_binary([_bool_series(f[f.target == t].in_target_turn).tolist() for t in targets]),
               "icc_F": icc_binary([_bool_series(el[el.target == t].first_sent_first).tolist() for t in targets])}
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


def send_order(pairs: pd.DataFrame) -> dict:
    """S1 and S2 split by whether the preferred member was sent first, and the order claims on the campaign runs only.

    In R pairs the design sends HI second whenever dt > 0 (it is a race), so S1's pre-turn P(HI first) is not
    randomised over send order; C1 and C2 randomise it. The campaign-only tables leave out the smoke and pilot runs,
    which were sent before the Memo with the pre-registration hash landed (same coding, `_order_tables`)."""
    el = pairs[_bool_series(pairs.eligible)]
    out = {"split": {}, "campaign_only": _order_tables(el[el.session.isin(CAMPAIGN)]) if "session" in el else {}}
    for fam in PANELS:
        f = el[el.family == fam]
        row = {}
        for name, d in (("S1", f[(f.pair_type == "R") & (f.turn_bin == "pre")]), ("C1", f[f.pair_type == "C1"]),
                        ("C2", f[f.pair_type == "C2"])):
            first = _bool_series(d.pref_sent_first)
            row[name] = {"pref_sent_first": _prop(int(_bool_series(d[first].pref_first).sum()), int(first.sum())),
                         "pref_sent_second": _prop(int(_bool_series(d[~first].pref_first).sum()), int((~first).sum()))}
        out["split"][fam] = row
    return out


def run(pairs: pd.DataFrame, probes: pd.DataFrame, blocks: dict[int, dict], n_boot: int = N_BOOT) -> dict:
    if not len(pairs):
        return {}
    return {"mechanism": mechanism(pairs, n_boot=n_boot), "insertion": insertion(probes, blocks),
            "hour": hour(probes), "operators": operators(pairs, probes), "between": between(pairs, probes),
            "whole_block": whole_block(probes, blocks), "send_order": send_order(pairs)}

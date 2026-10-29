"""The pre-registered analysis (docs/PREREGISTRATION.md). Input: the pairs and probes tables from outcomes.py (or from
sim.py, which writes the same columns). Output: one JSON-able dict; every number in README/REPORT comes from it.

Primary family (Holm over its 17 tests):
  P1  equal-key order F per client family (8): P(first-sent member lands first) on eligible EQ pairs; exact two-sided
      test against 0.5.
  P2  landing in the target's turn vs tau per family (8): randomisation test of homogeneity across the tau levels (quads
      permuted within target).
  P3  load (1, pooled): logistic slope of P(HI first) on block load (per 10M CU) in eligible R pairs, with family, turn
      bin and log(dt + 1) as covariates; "load effect" if the two-sided p survives Holm, "no load effect" if the 90 %
      interval lies inside +-0.4 (TOST). Per-family slopes are descriptive.
Secondary family (Holm separately):
  S1  backlog ordered by key: P(HI first) in eligible R pairs sent pre-turn, one-sided test of p <= 0.5, per family.
  S2  key identity: for C1, C2, C3, P(reward/cost-preferred member first), one-sided test of p <= 0.5, per family.
  S3  leakage to the preceding leader vs tau, per family (same randomisation test as P2).
Descriptive: race-reversal rate vs dt, F by gap and turn bin, slot-of-turn profiles, per-family load slopes,
per-validator tables, R_IND, the v1 arm, the extended target (Raiku).
Sensitivity (a, b, c of §4) and the verdict words of §5.
Gates: G1 positive controls, G3 selection (McNemar on in-turn landing by key label), G4 attribution (enforced by
eligibility). G2 (timing) is computed from the run logs by timing.py.
"""

from __future__ import annotations

import math

import numpy as np
import pandas as pd

from . import stats

FAMILIES = ["Agave", "JitoLabs", "AgaveBam", "HarmonicAgave", "HarmonicFiredancer", "Firedancer", "FireBAM", "Rakurai"]
EXTENDED = ["Raiku"]
# G1 positive controls: pooled Agave + JitoLabs (the reference scheduler and its most common fork), and Firedancer
G1_GROUPS = {"Agave line": ("Agave", "JitoLabs"), "Firedancer": ("Firedancer",)}
TAU_LEVELS = [-300, -150, 0, 100, 200, 300, 400, 550, 700, 900]
LOAD_UNIT = 10_000_000
LOAD_MARGIN = 0.4
SORTED_NULL = 0.5
MIDTURN_BINS = ("s0", "s1", "s23")
MIN_CELL = 15
TOLERANCE_REASONS = {"tau_out_of_tolerance", "dt_out_of_tolerance"}
CLIENT_REASONS = {"client_transition", "client_not_family"}


def _prop(k: int, n: int) -> dict:
    lo, hi = stats.clopper_pearson(k, n)
    return {"k": int(k), "n": int(n), "p": (k / n) if n else None, "ci95": [_f(lo), _f(hi)]}


def _f(x):
    return None if x is None or (isinstance(x, float) and math.isnan(x)) else round(float(x), 6)


def _bool_series(s: pd.Series) -> pd.Series:
    return s.astype("boolean").fillna(False).astype(bool)


def p1_equal_key(pairs: pd.DataFrame) -> dict:
    eq = pairs[(pairs.pair_type == "EQ") & pairs.eligible]
    out = {}
    for fam in FAMILIES + EXTENDED:
        d = eq[eq.family == fam]
        k, n = int(_bool_series(d.first_sent_first).sum()), len(d)
        r = _prop(k, n)
        r["p_value"] = stats.binom_p(k, n, 0.5)
        by = {}
        for (gap, b), g in d.groupby(["dt_planned", "turn_bin"]):
            by[f"gap{int(gap)}_{b}"] = _prop(int(_bool_series(g.first_sent_first).sum()), len(g))
        r["by_gap_bin"] = by
        out[fam] = r
    return out


def p2_landing(probes: pd.DataFrame, n_perm: int = 10_000, seed: int = 0) -> dict:
    out = {}
    if "client_ok" in probes:
        probes = probes[_bool_series(probes.client_ok)]
    for fam in FAMILIES + EXTENDED:
        d = probes[probes.family == fam]
        res = {"profile": {}, "n_quads": int(d.quad_id.nunique()) if len(d) else 0}
        for tau in TAU_LEVELS:
            g = d[d.tau_ms == tau]
            res["profile"][str(tau)] = {
                "land": _prop(int(_bool_series(g.in_target_turn).sum()), len(g)),
                "leak": _prop(int((g.landed_with == "preceding").sum()), len(g)),
                "slot_of_turn": {str(k): int(v) for k, v in g.slot_of_turn.dropna().astype(int).value_counts()
                                 .sort_index().items()},
            }
        if len(d):
            q = d.assign(land=_bool_series(d.in_target_turn), leak=(d.landed_with == "preceding")).groupby(
                "quad_id").agg(tau=("tau_ms", "first"), target=("target", "first"), land=("land", "sum"),
                               leak=("leak", "sum"), n=("land", "size"))
            for name in ("land", "leak"):
                chi2, p = stats.permutation_homogeneity(q.tau.values, q[name].values, q.n.values, q.target.values,
                                                        n_perm=n_perm, seed=seed)
                res[f"{name}_chi2"], res[f"{name}_p_value"] = _f(chi2), _f(p)
        out[fam] = res
    return out


def _load_fit(d: pd.DataFrame, extra: list[str]):
    dummies = [pd.get_dummies(d[c].fillna("none"), prefix=c, drop_first=True).astype(float) for c in extra]
    cov = pd.concat(dummies + [np.log1p(d.dt_planned.astype(float)).rename("logdt")], axis=1).values
    return stats.logistic_slope(_bool_series(d.pref_first).values, d.load_cu.values / LOAD_UNIT, cov,
                                margin=LOAD_MARGIN)


def _slope_dict(s, d: pd.DataFrame) -> dict:
    return {"n": s.n, "beta": _f(s.beta), "se": _f(s.se), "ci90": [_f(s.ci90[0]), _f(s.ci90[1])],
            "p_value": _f(s.p_two_sided), "equivalent": s.equivalent,
            "load_range_10m": [_f(d.load_cu.min() / LOAD_UNIT), _f(d.load_cu.max() / LOAD_UNIT)] if len(d) else None}


def p3_load(pairs: pd.DataFrame) -> dict:
    r = pairs[(pairs.pair_type == "R") & pairs.eligible & pairs.load_cu.notna()]
    core = r[r.family.isin(FAMILIES)]
    out = {"pooled": _slope_dict(_load_fit(core, ["family", "turn_bin"]), core) if len(core) >= 10 else
           {"n": len(core), "beta": None, "p_value": None, "equivalent": False}, "by_family": {}}
    for fam in FAMILIES + EXTENDED:
        d = r[r.family == fam]
        out["by_family"][fam] = _slope_dict(_load_fit(d, ["turn_bin"]), d) if len(d) >= 10 else \
            {"n": len(d), "beta": None}
    return out


def s1_backlog(pairs: pd.DataFrame) -> dict:
    r = pairs[(pairs.pair_type == "R") & pairs.eligible & (pairs.turn_bin == "pre")]
    out = {}
    for fam in FAMILIES + EXTENDED:
        d = r[r.family == fam]
        k, n = int(_bool_series(d.pref_first).sum()), len(d)
        out[fam] = {**_prop(k, n), "p_value": stats.binom_p(k, n, SORTED_NULL, "greater")}
    return out


def s2_key(pairs: pd.DataFrame) -> dict:
    out = {}
    for fam in FAMILIES + EXTENDED:
        res = {}
        for c in ("C1", "C2", "C3"):
            d = pairs[(pairs.pair_type == c) & pairs.eligible & (pairs.family == fam)]
            k, n = int(_bool_series(d.pref_first).sum()), len(d)
            res[c] = {**_prop(k, n), "p_value": stats.binom_p(k, n, 0.5, "greater")}
        out[fam] = res
    return out


def race_reversal(pairs: pd.DataFrame) -> dict:
    """RRR(dt) = P(HI lands first | LO sent dt earlier), mid-turn bins, eligible R pairs with dt > 0 (LO first)."""
    r = pairs[(pairs.pair_type == "R") & pairs.eligible & pairs.turn_bin.isin(MIDTURN_BINS) & ~pairs.pref_sent_first]
    out = {}
    for fam in FAMILIES + EXTENDED:
        d = r[r.family == fam]
        out[fam] = {str(int(dt)): _prop(int(_bool_series(g.pref_first).sum()), len(g))
                    for dt, g in d.groupby("dt_planned")}
    return out


def gate_positive_controls(pairs: pd.DataFrame) -> dict:
    """G1: in each control group, C1 and C2 put the reward/cost-preferred member first (one-sided 95 % bound > 0.5)."""
    out = {}
    for name, fams in G1_GROUPS.items():
        res = {}
        for c in ("C1", "C2"):
            d = pairs[(pairs.pair_type == c) & pairs.eligible & pairs.family.isin(fams)]
            k, n = int(_bool_series(d.pref_first).sum()), len(d)
            lo = stats.clopper_pearson(k, n, 2 * stats.ALPHA)[0] if n else None
            res[c] = {**_prop(k, n), "lower95_one_sided": _f(lo), "pass": lo is not None and lo > 0.5}
        res["pass"] = all(res[c]["pass"] for c in ("C1", "C2"))
        out[name] = res
    out["pass"] = all(v["pass"] for v in out.values())
    return out


def gate_selection(pairs: pd.DataFrame) -> dict:
    """G3: within each pair type, does landing in the target's turn depend on the key label? Exact McNemar on the
    discordant pairs (exactly one member in the turn)."""
    out = {}
    for t, d in pairs.groupby("pair_type"):
        a, o = _bool_series(d.pref_in_turn), _bool_series(d.other_in_turn)
        b, c = int((a & ~o).sum()), int((~a & o).sum())
        out[t] = {"pref_only": b, "other_only": c, "p_value": stats.binom_p(b, b + c, 0.5)}
    return out


def _reasons(pairs: pd.DataFrame) -> pd.Series:
    return pairs.exclusion.fillna("").map(lambda x: frozenset(r for r in x.split(",") if r))


def _order_tables(d: pd.DataFrame) -> dict:
    """The order claims (P1 F, S1 pre-turn P(HI first), S2 per contrast) per family, as proportions with intervals."""
    out = {}
    for fam in FAMILIES + EXTENDED:
        f = d[d.family == fam]
        eq, r = f[f.pair_type == "EQ"], f[(f.pair_type == "R") & (f.turn_bin == "pre")]
        row = {"P1_F": _prop(int(_bool_series(eq.first_sent_first).sum()), len(eq)),
               "S1_hi_first_pre": _prop(int(_bool_series(r.pref_first).sum()), len(r))}
        for c in ("C1", "C2", "C3"):
            g = f[f.pair_type == c]
            row[f"S2_{c}"] = _prop(int(_bool_series(g.pref_first).sum()), len(g))
        out[fam] = row
    return out


def sensitivity(pairs: pd.DataFrame) -> dict:
    """The three sensitivity analyses of PREREGISTRATION §4, reported next to the confirmatory results:
    (a) pairs excluded only for timing tolerance, kept and coded by their achieved values (send order and turn bin are
        always the achieved ones); (b) pairs excluded only for the client rule, kept; (c) censored coding: a pair with
        exactly one member in the target's turn counts as that member first."""
    if not len(pairs):
        return {}
    reasons, ok = _reasons(pairs), _bool_series(pairs.eligible)
    timing = pairs[ok | reasons.map(lambda r: bool(r) and r <= TOLERANCE_REASONS)]
    client = pairs[ok | reasons.map(lambda r: bool(r) and r <= CLIENT_REASONS)]
    a, o = _bool_series(pairs.pref_in_turn), _bool_series(pairs.other_in_turn)
    one = pairs[(a != o) & reasons.map(lambda r: r == {"not_both_in_turn"})].copy()
    one["pref_first"] = _bool_series(one.pref_in_turn)
    one["first_sent_first"] = _bool_series(one.pref_in_turn) == _bool_series(one.pref_sent_first)
    censored = pd.concat([pairs[ok], one], ignore_index=True)
    return {name: {"pairs": len(d), "added": len(d) - int(ok.sum()), "tables": _order_tables(d)}
            for name, d in (("a_timing_by_achieved", timing), ("b_client_included", client),
                            ("c_censored", censored))}


def descriptive(pairs: pd.DataFrame, probes: pd.DataFrame) -> dict:
    """Descriptive tables named in PREREGISTRATION §5 that need no test: R_IND, the v1 arm, per-validator rows."""
    el = pairs[_bool_series(pairs.eligible)] if len(pairs) else pairs
    out = {"R_IND_hi_first": {}, "V_EQ_v1_first": {}, "V_HI_hi1_first": {}, "per_target": {}}
    for fam in FAMILIES + EXTENDED:
        f = el[el.family == fam] if len(el) else el
        for key, t in (("R_IND_hi_first", "R_IND"), ("V_EQ_v1_first", "V_EQ"), ("V_HI_hi1_first", "V_HI")):
            g = f[f.pair_type == t] if len(f) else f
            out[key][fam] = _prop(int(_bool_series(g.pref_first).sum()) if len(g) else 0, len(g))
    if "probe" in probes:
        v1 = probes[probes.probe.isin(["A1", "HI1"])]
        legacy = probes[probes.probe.isin(["A", "HI"]) & probes.quad_id.isin(v1.quad_id)]
        out["v1_in_turn"] = {"v1": _prop(int(_bool_series(v1.in_target_turn).sum()), len(v1)),
                             "legacy_same_quads": _prop(int(_bool_series(legacy.in_target_turn).sum()), len(legacy))}
    for tgt, d in probes.groupby("target") if len(probes) else []:
        e = el[el.target == tgt] if len(el) else el
        eq, r = e[e.pair_type == "EQ"], e[e.pair_type == "R"]
        out["per_target"][tgt] = {
            "identities": sorted(d.identity.unique()) if "identity" in d else None,
            "family": d.family.iloc[0], "quads": int(d.quad_id.nunique()),
            "in_turn": _prop(int(_bool_series(d.in_target_turn).sum()), len(d)),
            "eligible_pairs": len(e), "F": _prop(int(_bool_series(eq.first_sent_first).sum()), len(eq)),
            "R_hi_first": _prop(int(_bool_series(r.pref_first).sum()), len(r))}
    return out


def verdicts(res: dict) -> dict:
    """The verdict words of PREREGISTRATION §5, applied mechanically to the Holm-adjusted p-values."""
    hp, hs, a = res["holm_primary"], res["holm_secondary"], stats.ALPHA
    out = {}
    for fam in FAMILIES:
        p1, p2, s1 = res["P1"][fam], res["P2"][fam], res["S1"][fam]
        v = {}
        v["P1"] = ("insufficient data" if p1["n"] < MIN_CELL else
                   ("arrival order breaks ties (F > 0.5)" if p1["p"] > 0.5 else "latest arrival first (F < 0.5)")
                   if hp[f"P1:{fam}"] < a else "F not distinguishable from 0.5")
        for key, test, word in (("P2", hp[f"P2:{fam}"], "landing"), ("S3", hs[f"S3:{fam}"], "leakage")):
            v[key] = ("insufficient data" if p2["n_quads"] < MIN_CELL else
                      f"{word} depends on the send offset" if test < a else
                      f"no evidence that {word} depends on the send offset")
        v["S1"] = ("insufficient data" if s1["n"] < MIN_CELL else
                   "higher key first in the backlog (P(HI first) > 0.5)" if hs[f"S1:{fam}"] < a else
                   "no key preference shown in the backlog")
        for c in ("C1", "C2", "C3"):
            s2 = res["S2"][fam][c]
            v[f"S2-{c}"] = ("insufficient data" if s2["n"] < MIN_CELL else
                            "reward/cost-preferred member first" if hs[f"S2-{c}:{fam}"] < a else
                            "no preference shown")
        out[fam] = v
    return out


def _or1(p):
    return 1.0 if p is None else p


def holm_family(tests: dict[str, float]) -> dict[str, float]:
    return {k: _f(v) for k, v in stats.holm(tests).items()}


def run(pairs: pd.DataFrame, probes: pd.DataFrame, n_perm: int = 10_000, seed: int = 0) -> dict:
    res = {"P1": p1_equal_key(pairs), "P2": p2_landing(probes, n_perm, seed), "P3": p3_load(pairs),
           "S1": s1_backlog(pairs), "S2": s2_key(pairs), "RRR": race_reversal(pairs),
           "G1": gate_positive_controls(pairs), "G3": gate_selection(pairs)}
    # the family size is fixed by the pre-registration: a test without data enters with p = 1
    primary, secondary = {}, {}
    for fam in FAMILIES:
        primary[f"P1:{fam}"] = res["P1"][fam]["p_value"]
        primary[f"P2:{fam}"] = _or1(res["P2"][fam].get("land_p_value"))
        secondary[f"S1:{fam}"] = res["S1"][fam]["p_value"]
        for c in ("C1", "C2", "C3"):
            secondary[f"S2-{c}:{fam}"] = res["S2"][fam][c]["p_value"]
        secondary[f"S3:{fam}"] = _or1(res["P2"][fam].get("leak_p_value"))
    pooled = res["P3"]["pooled"]
    primary["P3"] = _or1(pooled.get("p_value"))
    res["holm_primary"] = holm_family(primary)
    res["holm_secondary"] = holm_family(secondary)
    if pooled.get("beta") is not None:
        adj = res["holm_primary"].get("P3")
        pooled["verdict"] = ("load effect" if adj is not None and adj < stats.ALPHA else
                             "no load effect (equivalent within ±0.4)" if pooled["equivalent"] else "inconclusive")
    else:
        pooled["verdict"] = "insufficient data"
    res["verdicts"] = verdicts(res)
    res["sensitivity"] = sensitivity(pairs)
    res["descriptive"] = descriptive(pairs, probes)
    return res

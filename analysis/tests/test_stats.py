import math

import numpy as np

from itb_analysis import stats


def test_clopper_pearson_known_values():
    lo, hi = stats.clopper_pearson(5, 10)
    assert math.isclose(lo, 0.187086, abs_tol=1e-5) and math.isclose(hi, 0.812914, abs_tol=1e-5)
    assert stats.clopper_pearson(0, 10)[0] == 0.0 and stats.clopper_pearson(10, 10)[1] == 1.0
    assert all(math.isnan(v) for v in stats.clopper_pearson(0, 0))


def test_holm_is_monotone_and_capped():
    adj = stats.holm({"a": 0.01, "b": 0.04, "c": 0.03})
    assert math.isclose(adj["a"], 0.03) and math.isclose(adj["c"], 0.06) and math.isclose(adj["b"], 0.06)
    assert stats.holm({"x": 0.6, "y": 0.9}) == {"x": 1.0, "y": 1.0}


def test_tost_proportion():
    assert stats.tost_proportion(100, 200, margin=0.15).equivalent
    assert not stats.tost_proportion(5, 10, margin=0.15).equivalent
    assert not stats.tost_proportion(180, 200, margin=0.15).equivalent


def test_permutation_homogeneity_detects_dependence_only_when_present():
    rng = np.random.default_rng(1)
    levels = np.repeat(np.arange(10), 40)
    strata = np.tile(np.arange(4), 100)
    flat = rng.binomial(4, 0.6, size=400)
    _, p_flat = stats.permutation_homogeneity(levels, flat, np.full(400, 4), strata, n_perm=999, seed=2)
    peaked = rng.binomial(4, np.where(levels == 5, 0.95, 0.4))
    _, p_peak = stats.permutation_homogeneity(levels, peaked, np.full(400, 4), strata, n_perm=999, seed=2)
    assert p_flat > 0.05 and p_peak < 0.01


def test_logistic_slope_tost():
    rng = np.random.default_rng(3)
    x = rng.uniform(0, 5, 2000)
    y0 = rng.random(2000) < 0.8
    s0 = stats.logistic_slope(y0, x, margin=0.4)
    assert s0.equivalent and abs(s0.beta) < 0.2
    y1 = rng.random(2000) < 1 / (1 + np.exp(-(2 - 0.8 * x)))
    s1 = stats.logistic_slope(y1, x, margin=0.4)
    assert not s1.equivalent and s1.p_two_sided < 1e-6


def test_newcombe():
    d, (lo, hi) = stats.newcombe_diff(56, 70, 48, 80)
    assert math.isclose(d, 0.2) and lo < 0.2 < hi and 0.05 < lo < 0.07 and 0.33 < hi < 0.35

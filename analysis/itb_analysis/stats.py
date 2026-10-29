"""Statistical tools of the pre-registration (docs/PREREGISTRATION.md §Analysis).

Deterministic: every resampling takes a seed.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

import numpy as np
from scipy import stats as sps

ALPHA = 0.05


def clopper_pearson(k: int, n: int, alpha: float = ALPHA) -> tuple[float, float]:
    """Exact two-sided (1 - alpha) interval for a binomial proportion; (nan, nan) for n = 0."""
    if n == 0:
        return (math.nan, math.nan)
    lo = 0.0 if k == 0 else float(sps.beta.ppf(alpha / 2, k, n - k + 1))
    hi = 1.0 if k == n else float(sps.beta.ppf(1 - alpha / 2, k + 1, n - k))
    return (lo, hi)


def binom_p(k: int, n: int, p0: float = 0.5, alternative: str = "two-sided") -> float:
    """Exact binomial test p-value; 1.0 for n = 0 (no evidence)."""
    if n == 0:
        return 1.0
    return float(sps.binomtest(k, n, p0, alternative=alternative).pvalue)


def holm(pvalues: dict[str, float]) -> dict[str, float]:
    """Holm step-down adjusted p-values (monotone, capped at 1)."""
    items = sorted(pvalues.items(), key=lambda kv: kv[1])
    m = len(items)
    out, running = {}, 0.0
    for i, (name, p) in enumerate(items):
        running = max(running, min(1.0, (m - i) * p))
        out[name] = running
    return out


@dataclass
class Equivalence:
    estimate: float
    ci90: tuple[float, float]
    equivalent: bool


def tost_proportion(k: int, n: int, p0: float = 0.5, margin: float = 0.15, alpha: float = ALPHA) -> Equivalence:
    """TOST for a proportion: equivalent to p0 within ±margin if the exact (1 - 2 alpha) interval lies inside."""
    lo, hi = clopper_pearson(k, n, 2 * alpha)
    est = k / n if n else math.nan
    ok = n > 0 and lo > p0 - margin and hi < p0 + margin
    return Equivalence(est, (lo, hi), bool(ok))


def chi2_stat(k: np.ndarray, n: np.ndarray) -> float:
    """Pearson chi-square of homogeneity for successes k out of n per level (levels with n = 0 dropped)."""
    keep = n > 0
    k, n = k[keep].astype(float), n[keep].astype(float)
    if len(k) < 2 or n.sum() == 0:
        return 0.0
    p = k.sum() / n.sum()
    if p in (0.0, 1.0):
        return 0.0
    exp1, exp0 = n * p, n * (1 - p)
    return float((((k - exp1) ** 2) / exp1 + (((n - k) - exp0) ** 2) / exp0).sum())


def permutation_homogeneity(levels, successes, trials, strata, n_perm: int = 10_000, seed: int = 0):
    """Randomisation test that the success rate does not depend on the level.

    One entry per cluster (a quad): its level (tau), its successes and trials (probes), and its stratum (target).
    Levels are permuted across clusters within each stratum, which is how they were assigned.
    Returns (chi-square, p-value).
    """
    levels = np.asarray(levels)
    succ = np.asarray(successes, dtype=float)
    tri = np.asarray(trials, dtype=float)
    strata = np.asarray(strata)
    lv = np.unique(levels)
    idx = np.searchsorted(lv, levels)

    def stat(ix):
        k = np.bincount(ix, weights=succ, minlength=len(lv))
        n = np.bincount(ix, weights=tri, minlength=len(lv))
        return chi2_stat(k, n)

    obs = stat(idx)
    rng = np.random.default_rng(seed)
    groups = [np.flatnonzero(strata == s) for s in np.unique(strata)]
    ge = 0
    perm = idx.copy()
    for _ in range(n_perm):
        for g in groups:
            perm[g] = idx[g][rng.permutation(len(g))]
        ge += stat(perm) >= obs - 1e-12
    return obs, (ge + 1) / (n_perm + 1)


@dataclass
class Slope:
    beta: float
    se: float
    ci90: tuple[float, float]
    p_two_sided: float
    equivalent: bool
    n: int


def logistic_slope(y, x, covariates=None, margin: float = 0.4, alpha: float = ALPHA) -> Slope:
    """Slope of logit P(y) on x (optional covariate matrix), Wald interval, TOST against ±margin (logit scale).

    A margin of 0.4 per unit of x is about ±0.10 in probability near p = 0.5.
    """
    import statsmodels.api as sm

    y = np.asarray(y, dtype=float)
    x = np.asarray(x, dtype=float)
    cols = [np.ones_like(x), x]
    if covariates is not None and np.size(covariates):
        cov = np.asarray(covariates, dtype=float)
        cols += [cov[:, j] for j in range(cov.shape[1])] if cov.ndim == 2 else [cov]
    X = np.column_stack(cols)
    n = len(y)
    if n < 10 or y.min() == y.max():
        return Slope(math.nan, math.nan, (math.nan, math.nan), 1.0, False, n)
    try:
        fit = sm.GLM(y, X, family=sm.families.Binomial()).fit()
    except Exception:  # perfect separation and similar: no estimate
        return Slope(math.nan, math.nan, (math.nan, math.nan), 1.0, False, n)
    b, se = float(fit.params[1]), float(fit.bse[1])
    z90 = sps.norm.ppf(1 - alpha)
    ci = (b - z90 * se, b + z90 * se)
    p = float(2 * sps.norm.sf(abs(b / se))) if se > 0 else 1.0
    return Slope(b, se, ci, p, bool(ci[0] > -margin and ci[1] < margin), n)


def newcombe_diff(k1: int, n1: int, k2: int, n2: int, alpha: float = ALPHA) -> tuple[float, tuple[float, float]]:
    """Difference p1 - p2 with Newcombe's hybrid score interval (method 10)."""

    def wilson(k, n):
        if n == 0:
            return (0.0, 1.0)
        z = sps.norm.ppf(1 - alpha / 2)
        p = k / n
        c = (p + z * z / (2 * n)) / (1 + z * z / n)
        h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
        return (c - h, c + h)

    p1 = k1 / n1 if n1 else math.nan
    p2 = k2 / n2 if n2 else math.nan
    l1, u1 = wilson(k1, n1)
    l2, u2 = wilson(k2, n2)
    d = p1 - p2
    lo = d - math.sqrt((p1 - l1) ** 2 + (u2 - p2) ** 2)
    hi = d + math.sqrt((u1 - p1) ** 2 + (p2 - l2) ** 2)
    return d, (lo, hi)

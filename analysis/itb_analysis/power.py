"""Operating characteristics of the pre-registered analysis at the frozen design (docs/POWER.md).

For each scenario, `n_sim` synthetic campaigns are analysed with analysis.run; the output is the share of campaigns in
which each test rejects after Holm (power under an effect, false-positive rate under the null), and how often the load
test declares equivalence.
"""

from __future__ import annotations

import json
from pathlib import Path

from . import analysis, sim
from .stats import ALPHA


def operating_characteristics(design: list[dict], targets: dict[str, dict], scenario: str, n_sim: int,
                              n_perm: int, seed: int) -> dict:
    sc = sim.SCENARIOS[scenario]
    reject_p: dict[str, int] = {}
    reject_s: dict[str, int] = {}
    equiv: dict[str, int] = {}
    any_primary = 0
    n_eq: dict[str, list[int]] = {}
    for i in range(n_sim):
        pairs, probes = sim.simulate(design, targets, sc, seed + i)
        res = analysis.run(pairs, probes, n_perm=n_perm, seed=seed + i)
        hit = False
        for k, p in res["holm_primary"].items():
            r = p is not None and p < ALPHA
            reject_p[k] = reject_p.get(k, 0) + r
            hit |= r
        any_primary += hit
        for k, p in res["holm_secondary"].items():
            reject_s[k] = reject_s.get(k, 0) + (p is not None and p < ALPHA)
        equiv["pooled"] = equiv.get("pooled", 0) + bool(res["P3"]["pooled"].get("equivalent"))
        for fam, r in res["P3"]["by_family"].items():
            equiv[fam] = equiv.get(fam, 0) + bool(r.get("equivalent"))
        for fam, r in res["P1"].items():
            n_eq.setdefault(fam, []).append(r["n"])
    return {
        "scenario": scenario, "truth": sc, "n_sim": n_sim, "n_perm": n_perm,
        "any_primary_rejection": any_primary / n_sim,
        "primary": {k: v / n_sim for k, v in sorted(reject_p.items())},
        "secondary": {k: v / n_sim for k, v in sorted(reject_s.items())},
        "load_equivalent": {k: v / n_sim for k, v in sorted(equiv.items())},
        "eligible_eq_pairs_mean": {k: sum(v) / len(v) for k, v in sorted(n_eq.items())},
    }


def run(design_dir: Path, out: Path, scenarios, n_sim: int, n_perm: int, seed: int, log=print) -> dict:
    design = [json.loads(line) for line in (design_dir / "design.jsonl").read_text().splitlines() if line.strip()]
    targets = {t["slot"]: t for t in json.loads((design_dir / "targets.json").read_text())["targets"]}
    result = {}
    for s in scenarios:
        log(f"power: scenario {s}, {n_sim} campaigns")
        result[s] = operating_characteristics(design, targets, s, n_sim, n_perm, seed)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=1, sort_keys=True) + "\n")
    return result

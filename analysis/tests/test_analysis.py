import json
from pathlib import Path

from itb_analysis import analysis, sim

DESIGN = Path(__file__).resolve().parents[2] / "design"


def load():
    rows = [json.loads(line) for line in (DESIGN / "design.jsonl").read_text().splitlines() if line.strip()]
    targets = {t["slot"]: t for t in json.loads((DESIGN / "targets.json").read_text())["targets"]}
    return rows, targets


def test_sim_tables_have_the_analysis_columns():
    rows, targets = load()
    pairs, probes = sim.simulate(rows, targets, sim.SCENARIOS["expected"], seed=1)
    assert {"pair_type", "eligible", "family", "first_sent_first", "pref_first", "load_cu", "turn_bin"} <= set(pairs)
    assert {"quad_id", "target", "tau_ms", "in_target_turn", "landed_with", "slot_of_turn"} <= set(probes)
    assert len(pairs) == 2 * sum(r["session"] in "ABCD" for r in rows)


def test_primary_family_has_17_tests_and_finds_a_strong_effect():
    rows, targets = load()
    pairs, probes = sim.simulate(rows, targets, sim.SCENARIOS["expected"], seed=2)
    res = analysis.run(pairs, probes, n_perm=99, seed=0)
    assert len(res["holm_primary"]) == 17
    assert len(res["holm_secondary"]) == 40
    # JitoLabs F = 0.7 on ~70 eligible pairs: far from 0.5
    assert res["P1"]["JitoLabs"]["p_value"] < 0.01
    assert res["P3"]["pooled"]["verdict"] in ("no load effect (equivalent within ±0.4)", "inconclusive")


def test_null_scenario_has_no_large_p1_effect():
    rows, targets = load()
    pairs, probes = sim.simulate(rows, targets, sim.SCENARIOS["null"], seed=3)
    res = analysis.run(pairs, probes, n_perm=99, seed=0)
    for fam in analysis.FAMILIES:
        r = res["P1"][fam]
        if r["n"] >= 40:
            assert abs(r["p"] - 0.5) < 0.2

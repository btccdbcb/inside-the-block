import json
from collections import Counter

import numpy as np

from itb_analysis.design import (
    PHI_LEVELS,
    Allocation,
    balanced,
    dumps_row,
    generate,
    systematic_split,
)

TARGETS = [{"slot": f"T{i}", "extended": False} for i in range(1, 14)] + [{"slot": "X1", "extended": True}]


def test_balanced_margins_exact():
    rng = np.random.default_rng(1)
    out = balanced(["a", "b", "c"], [0.5, 0.3, 0.2], 10, rng)
    assert Counter(out) == {"a": 5, "b": 3, "c": 2}


def test_systematic_split_sizes():
    rng = np.random.default_rng(2)
    items = list(range(77))
    out = systematic_split(items, {"A": 23, "B": 23, "C": 19, "D": 12}, rng)
    assert {k: len(v) for k, v in out.items()} == {"A": 23, "B": 23, "C": 19, "D": 12}
    assert sorted(sum(out.values(), [])) == items


def test_design_is_deterministic_and_balanced():
    a = generate(TARGETS, Allocation(), 42)
    b = generate(TARGETS, Allocation(), 42)
    assert [dumps_row(r) for r in a] == [dumps_row(r) for r in b]
    ids = [r["quad_id"] for r in a]
    assert len(ids) == len(set(ids))
    camp = [r for r in a if r["session"] in "ABCD" and r["target_slot"] == "T1"]
    assert len(camp) == 77
    phi = Counter(r["phi_ms"] for r in camp)
    assert set(phi) == set(PHI_LEVELS) and max(phi.values()) - min(phi.values()) <= 1
    races = [r for r in camp if r["main_type"] in ("R", "R_IND")]
    assert all(r["main_first"] == "other" for r in races if r["dt_ms"] > 0)
    for r in a:
        assert sorted(r["payer_perm"]) == [0, 1, 2, 3]
        json.loads(dumps_row(r))
    pilot = [r for r in a if r["session"] == "pilot"]
    assert len(pilot) == 13 * 3 + 1
    for t in [f"T{i}" for i in range(1, 14)]:
        rows = [r for r in pilot if r["target_slot"] == t]
        assert {r["fee_scale"] for r in rows} == {"s1", "s10"}
        assert any(r["control_type"].startswith("V_") for r in rows)

import json
from collections import Counter

import numpy as np

from itb_analysis.design import (
    TAU_LEVELS,
    Allocation,
    balanced,
    dumps_row,
    generate,
    rows_by_session,
    session_hours,
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
    # proportional within every run of consecutive items (strata): each block of 10 gets ~10 * n_s / 77 of bin s
    for start in range(0, 70, 10):
        block = set(range(start, start + 10))
        for k, n in {"A": 23, "B": 23, "C": 19, "D": 12}.items():
            got = len(block & set(out[k]))
            assert abs(got - 10 * n / 77) < 2, (start, k, got)


def test_design_is_deterministic_and_balanced():
    a = generate(TARGETS, Allocation(), 42)
    b = generate(TARGETS, Allocation(), 42)
    assert [dumps_row(r) for r in a] == [dumps_row(r) for r in b]
    ids = [r["quad_id"] for r in a]
    assert len(ids) == len(set(ids))
    camp = [r for r in a if r["session"] in "ABCD" and r["target_slot"] == "T1"]
    assert len(camp) == 77
    tau = Counter(r["tau_ms"] for r in camp)
    assert set(tau) == set(TAU_LEVELS) and max(tau.values()) - min(tau.values()) <= 1
    # every session sees every tau level in proportion (no session x tau confounding)
    for s, n in Allocation().per_target.items():
        st = Counter(r["tau_ms"] for r in camp if r["session"] == s)
        for lv in TAU_LEVELS:
            assert abs(st[lv] - n * tau[lv] / 77) < 2, (s, lv, st[lv])
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


def test_low_turn_targets_get_fewer_rows():
    assert session_hours() == {"A": 4, "B": 4, "C": 5, "D": 3}
    base = {"A": 23, "B": 23, "C": 19, "D": 12}
    assert rows_by_session({"turns_per_day": 1400}, base, 0.6) == base
    assert rows_by_session({"turns_per_day": 49}, base, 0.6) == {"A": 4, "B": 4, "C": 6, "D": 3}
    targets = [dict(t, turns_per_day=49 if t["slot"] == "T13" else 1000) for t in TARGETS]
    rows = generate(targets, Allocation(), 7)
    t13 = Counter(r["session"] for r in rows if r["target_slot"] == "T13" and r["session"] in "ABCD")
    assert t13 == {"A": 4, "B": 4, "C": 6, "D": 3}

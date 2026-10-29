import json
import math
from pathlib import Path

from itb_analysis import explore, sim

DESIGN = Path(__file__).resolve().parents[2] / "design"


def test_mechanism_fit_recovers_a_drain_queue():
    # RRR(dt) = 0.9 exp(-dt / 5) observed with 400 pairs per gap
    cells = [(dt, round(400 * 0.9 * math.exp(-dt / 5)), 400) for dt in (0, 2, 10, 25, 50, 100)]
    fit = explore.fit_mechanisms(cells)
    assert fit["best"] == "drain"
    assert abs(fit["models"]["drain"]["params"]["h"] - 5) < 1
    assert fit["models"]["constant"]["aic"] > fit["models"]["drain"]["aic"] + 100


def test_mechanism_bootstrap_and_recovery():
    cells = [(dt, round(60 * 0.9 * math.exp(-dt / 5)), 60) for dt in (0, 2, 10, 25, 50, 100)]
    fit = explore.fit_mechanisms(cells, n_boot=40, seed=1)
    lo, hi = fit["bootstrap"]["ci95"]["h"] if fit["best"] == "drain" else fit["bootstrap"]["ci95"]["B"]
    assert lo < hi
    assert set(fit["recovery"]["share_correct"]) == set(explore.MODELS)
    assert fit == explore.fit_mechanisms(cells, n_boot=40, seed=1)  # seeded


def test_icc():
    assert explore.icc_binary([[True] * 50, [False] * 50]) > 0.9
    assert abs(explore.icc_binary([[True, False] * 25, [False, True] * 25])) < 0.05


def test_mechanism_fit_recovers_arrival_order():
    cells = [(dt, round(300 * (0.5 if dt == 0 else 0.0)), 300) for dt in (0, 2, 10, 25, 50, 100)]
    assert explore.fit_mechanisms(cells)["best"] == "arrival"


def test_whole_block_sortedness_sign():
    block = {"slot": 100, "mode": "full", "leader": "L", "n_tx": 20, "sum_cost_units": 1,
             "cu_price": [100_000 * k for k in range(20, 0, -1)], "cu_limit": [10_000] * 20, "n_sig": [1] * 20,
             "n_writable": [2] * 20, "loaded_data": [None] * 20}
    import pandas as pd

    probes = pd.DataFrame([{"quad_id": "q", "slot0": 100, "family": "Agave", "identity": "L"}])
    res = explore.whole_block(probes, {100: block})
    assert res["Agave"]["price_tau"]["median"] == 1.0
    assert res["Agave"]["priority_tau"]["median"] == 1.0


def test_runs_on_simulated_tables():
    rows = [json.loads(line) for line in (DESIGN / "design.jsonl").read_text().splitlines() if line.strip()]
    targets = {t["slot"]: t for t in json.loads((DESIGN / "targets.json").read_text())["targets"]}
    pairs, probes = sim.simulate(rows, targets, sim.SCENARIOS["expected"], seed=4)
    res = explore.run(pairs, probes, {}, n_boot=10)
    assert res["mechanism"]["JitoLabs"]["fit"]["best"] in explore.MODELS
    json.dumps(res)

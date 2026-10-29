"""Gate G2 (timing) from run directories (dry or live): quads.jsonl plus the slot stream (slots.jsonl[.gz]).

  (a) sender execution: the first send left within +-50 ms of its planned time, for >= 90 % of fired quads;
  (b) region hit: quads planned pre-turn (tau <= +200 ms) sent before the observed first shred of slot0, for >= 90 %.
Also the error of the fire anchor per tau level and per anchor kind, and skip reasons per target (reachability). For a
quad fired on a predicted anchor (the first shred of slot0 - 1 not yet seen at fire time), the achieved minus planned
tau is the clock's prediction error for that slot (the send itself is on time to < 1 ms), so `anchor_error_ms` is the
slot-clock calibration measured on the real schedule.
"""

from __future__ import annotations

from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

from .collect import read_jsonl
from .outcomes import first_shred_frame

EXEC_TOLERANCE_MS = 50.0
PRE_TURN_MAX_TAU = 200
SHARE = 0.90


def _slots(run_dir: Path) -> list[dict]:
    for name in ("slots.jsonl.gz", "slots.jsonl"):
        if (run_dir / name).exists():
            return read_jsonl(run_dir / name)
    return []


def _q(x, qs):
    return [round(float(v), 1) for v in np.quantile(x, qs)] if len(x) else None


def g2(run_dirs) -> dict:
    fired, skips = [], defaultdict(Counter)
    shreds: dict[int, float] = {}
    for d in run_dirs:
        d = Path(d)
        shreds.update(first_shred_frame(_slots(d)))
        for q in read_jsonl(d / "quads.jsonl"):
            if q.get("status") == "fired":
                fired.append(q)
            skips[q["target_slot"]][q.get("reason") or q.get("status")] += 1
    late = np.array([q["send_late_ms"] for q in fired if q.get("send_late_ms") is not None])
    pre_off0, by_tau, by_anchor, missing = [], defaultdict(list), defaultdict(list), 0
    for q in fired:
        a0, a_prev = shreds.get(q["slot0"]), shreds.get(q["slot0"] - 1)
        if a0 is None or a_prev is None or q.get("first_send_ms") is None:
            missing += 1
            continue
        off0 = q["first_send_ms"] - a0
        err = q["first_send_ms"] - a_prev - q["tau_ms"]
        by_tau[q["tau_ms"]].append(err)
        by_anchor[q.get("anchor") or "unknown"].append(err)
        if q["tau_ms"] <= PRE_TURN_MAX_TAU:
            pre_off0.append(off0)
    pre_off0 = np.array(pre_off0)
    a_share = float((np.abs(late) <= EXEC_TOLERANCE_MS).mean()) if len(late) else None
    b_share = float((pre_off0 < 0).mean()) if len(pre_off0) else None
    return {
        "fired": len(fired),
        "a_exec_within_50ms": {"share": a_share, "n": len(late), "pass": a_share is not None and a_share >= SHARE,
                               "late_ms_p10_p50_p90": _q(late, [0.1, 0.5, 0.9])},
        "b_pre_turn_before_slot0": {"share": b_share, "n": len(pre_off0),
                                    "pass": b_share is not None and b_share >= SHARE,
                                    "off0_ms_p10_p50_p90": _q(pre_off0, [0.1, 0.5, 0.9])},
        "tau_error_ms_by_level": {str(k): {"n": len(v), "p10_p50_p90": _q(np.array(v), [0.1, 0.5, 0.9])}
                                  for k, v in sorted(by_tau.items())},
        "anchor_error_ms": {k: {"n": len(v), "p10_p50_p90": _q(np.array(v), [0.1, 0.5, 0.9]),
                                "abs_p50_p90": _q(np.abs(np.array(v)), [0.5, 0.9])}
                            for k, v in sorted(by_anchor.items())},
        "quads_without_shred_observation": missing,
        "outcomes_by_target": {k: dict(v) for k, v in sorted(skips.items())},
    }

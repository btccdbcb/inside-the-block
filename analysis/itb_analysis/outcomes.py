"""Pair coding: one row per pair (main and control of every fired quad) and one row per probe.

Timing is measured against first-shred observations from the slot stream, mapped into our clock frame the same way the
runner does it: per source, node timestamp + min(0, smallest receive delay seen). The observed first shred of a slot is
the earliest over sources.

  tau_ach   first send of the quad - observed first shred of slot0 - 1      (the planned tau's achieved value)
  off0      first send of the quad - observed first shred of slot0          (turn bins)
  dt_ach    write time of the later member - write time of the earlier member of a pair

Turn bins (by off0): pre < -100 ms <= handover < +50 ms <= s0 < slot 1 start <= s1 < slot 2 start <= s23.
"""

from __future__ import annotations

import gzip
import json
import math
from pathlib import Path

import pandas as pd

from .collect import read_jsonl

PAIRS = {"main": ("main_pref", "main_other"), "control": ("control_pref", "control_other")}
TURN_BINS = ["pre", "handover", "s0", "s1", "s23"]
TAU_TOLERANCE_MS = 100.0


def first_shred_frame(slot_rows: list[dict]) -> dict[int, float]:
    corr: dict[int, float] = {}
    for r in slot_rows:
        d = r["recv_ms"] - r["node_ms"]
        corr[r["src"]] = min(corr.get(r["src"], 0.0), d)
    out: dict[int, float] = {}
    for r in slot_rows:
        t = r["node_ms"] + corr[r["src"]]
        if r["slot"] not in out or t < out[r["slot"]]:
            out[r["slot"]] = t
    return out


def turn_bin(off0: float | None, slot1_start: float | None, slot2_start: float | None) -> str | None:
    if off0 is None or math.isnan(off0):
        return None
    if off0 < -100:
        return "pre"
    if off0 < 50:
        return "handover"
    if slot1_start is None or off0 < slot1_start:
        return "s0"
    if slot2_start is None or off0 < slot2_start:
        return "s1"
    return "s23"


def dt_tolerance(dt_planned: float) -> float:
    return 1.0 + 0.1 * dt_planned


def _send_ms(stream: dict) -> float | None:
    """When the bytes were handed to QUIC: the write time (streams are opened ahead since D-026)."""
    return stream.get("write_ms", stream.get("open_ms"))


def _order_key(o: dict) -> tuple:
    return (o["slot"], o["index_in_block"])


def code_run(run_dir: Path, targets: dict[str, dict]) -> tuple[pd.DataFrame, pd.DataFrame]:
    """(pairs, probes) for one collected run directory."""
    run_dir = Path(run_dir)
    quads = [q for q in read_jsonl(run_dir / "quads.jsonl") if q.get("status") == "fired"]
    streams = {s["signature"]: s for s in read_jsonl(run_dir / "streams.jsonl")}
    outcomes = {o["signature"]: o for o in read_jsonl(run_dir / "outcomes.jsonl")}
    shreds = first_shred_frame(read_jsonl(run_dir / "slots.jsonl.gz"))
    gossip = read_jsonl(run_dir / "gossip.jsonl.gz")
    pair_rows, probe_rows = [], []
    for q in quads:
        s0 = q["slot0"]
        t = targets.get(q["target_slot"], {})
        family = t.get("expected_client")
        by_role = {p["role"]: p for p in q["probes"]}
        first_send = q.get("first_send_ms")
        a_prev, a0 = shreds.get(s0 - 1), shreds.get(s0)
        a1, a2 = shreds.get(s0 + 1), shreds.get(s0 + 2)
        tau_ach = first_send - a_prev if first_send is not None and a_prev is not None else None
        off0 = first_send - a0 if first_send is not None and a0 is not None else None
        bin_ = turn_bin(off0, (a1 - a0) if a1 and a0 else None, (a2 - a0) if a2 and a0 else None)
        client_before, client_after = clients_around(gossip, q["identity"], q.get("fired_utc"))
        client_reason = None
        if client_before is not None and client_after is not None and client_before != client_after:
            client_reason = "client_transition"
        elif any(c is not None and c != family for c in (client_before, client_after)):
            client_reason = "client_not_family"
        common = {"quad_id": q["quad_id"], "run_id": q["run_id"], "session": q["session"], "target": q["target_slot"],
                  "identity": q["identity"], "family": family, "client_at_send": (q.get("view") or {}).get("client_id"),
                  "client_before": client_before, "client_after": client_after,
                  "ingress": (q.get("view") or {}).get("ingress_kind"), "slot0": s0,
                  "fee_scale": q.get("fee_scale"), "tau_ms": q["tau_ms"], "tau_ach": tau_ach, "off0": off0,
                  "turn_bin": bin_, "anchor": q.get("anchor"), "fired_utc": q.get("fired_utc"),
                  "client_ok": client_reason is None}
        for role, p in by_role.items():
            o = outcomes.get(p["signature"], {})
            s = streams.get(p["signature"], {})
            probe_rows.append({**common, "role": role, "probe": p["probe"], "signature": p["signature"],
                               "send_ms": _send_ms(s), "landed": bool(o.get("landed")),
                               "in_target_turn": bool(o.get("in_target_turn")),
                               "landed_with": o.get("landed_with"), "slot_of_turn": o.get("slot_of_turn"),
                               "slot": o.get("slot"), "index_in_block": o.get("index_in_block"),
                               "err": o.get("err"), "fee_ok": o.get("meta_fee") == o.get("fee_expected")
                               if o.get("landed") else None,
                               "block_cost_units": o.get("block_cost_units")})
        for kind, (pref_role, other_role) in PAIRS.items():
            if pref_role not in by_role or other_role not in by_role:
                continue
            ptype = q["main_type"] if kind == "main" else q.get("control_type", q.get("control_type_design"))
            dt_planned = q["dt_ms"] if kind == "main" else q["control_gap_ms"]
            pref, other = by_role[pref_role], by_role[other_role]
            sp, so = streams.get(pref["signature"], {}), streams.get(other["signature"], {})
            op, oo = outcomes.get(pref["signature"], {}), outcomes.get(other["signature"], {})
            tp, to = _send_ms(sp), _send_ms(so)
            if tp is not None and to is not None:
                pref_sent_first = tp <= to
                dt_ach = abs(tp - to)
            else:
                pref_sent_first = (q["main_first"] if kind == "main" else q["control_first"]) == "pref"
                dt_ach = None
            both_turn = bool(op.get("in_target_turn")) and bool(oo.get("in_target_turn"))
            pref_first = None
            if both_turn:
                pref_first = _order_key(op) < _order_key(oo)
            first_sent_first = None if pref_first is None else (pref_first == pref_sent_first)
            same_block = both_turn and op["slot"] == oo["slot"]
            loads = [x.get("block_cost_units") for x in (op, oo) if x.get("in_target_turn")]
            reasons = []
            if not both_turn:
                reasons.append("not_both_in_turn")
            if both_turn and (op.get("err") is not None or oo.get("err") is not None):
                reasons.append("error")
            fee_bad = any(x.get("meta_fee") != x.get("fee_expected") for x in (op, oo))
            if both_turn and fee_bad:
                reasons.append("fee_mismatch")
            if client_reason:
                reasons.append(client_reason)
            if tau_ach is None or abs(tau_ach - q["tau_ms"]) > TAU_TOLERANCE_MS:
                reasons.append("tau_out_of_tolerance")
            if dt_ach is None or abs(dt_ach - dt_planned) > dt_tolerance(dt_planned):
                reasons.append("dt_out_of_tolerance")
            pair_rows.append({**common, "pair": kind, "pair_type": ptype, "dt_planned": dt_planned, "dt_ach": dt_ach,
                              "pref_probe": pref["probe"], "other_probe": other["probe"],
                              "pref_sent_first": pref_sent_first, "pref_landed": bool(op.get("landed")),
                              "other_landed": bool(oo.get("landed")), "pref_in_turn": bool(op.get("in_target_turn")),
                              "other_in_turn": bool(oo.get("in_target_turn")), "both_in_turn": both_turn,
                              "same_block": same_block, "pref_first": pref_first, "first_sent_first": first_sent_first,
                              "load_cu": (sum(loads) / len(loads)) if loads else None,
                              "eligible": not reasons, "exclusion": ",".join(reasons)})
    return pd.DataFrame(pair_rows), pd.DataFrame(probe_rows)


def clients_around(gossip: list[dict], identity: str, fired_utc: str | None) -> tuple[str | None, str | None]:
    """clientId of the identity in the last gossip snapshot before and the first after the quad."""
    if not gossip or not fired_utc:
        return None, None
    before = after = None
    for g in gossip:
        view = next((v for v in g.get("views", []) if v.get("identity") == identity), None)
        if view is None:
            continue
        if g["utc"] <= fired_utc:
            before = view.get("client_id")
        elif after is None:
            after = view.get("client_id")
    return before, after


def code_all(raw_root: Path, targets_path: Path) -> tuple[pd.DataFrame, pd.DataFrame]:
    targets = {t["slot"]: t for t in json.loads(Path(targets_path).read_text())["targets"]}
    pairs, probes = [], []
    for d in sorted(Path(raw_root).iterdir()):
        if (d / "outcomes.jsonl").exists():
            a, b = code_run(d, targets)
            pairs.append(a)
            probes.append(b)
    if not pairs:
        return pd.DataFrame(), pd.DataFrame()
    return pd.concat(pairs, ignore_index=True), pd.concat(probes, ignore_index=True)


def read_gz_json(p: Path):
    return json.loads(gzip.decompress(Path(p).read_bytes()))

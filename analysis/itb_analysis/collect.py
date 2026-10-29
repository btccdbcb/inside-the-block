"""Collector: block excerpts and per-probe outcomes for the fired quads of one or more runs.

Landing is decided from blocks (getBlock, `maxSupportedTransactionVersion: 1`, rewards on), never from status calls
alone; getSignatureStatuses is only a cross-check and a pointer to blocks outside the archived window.

For each fired quad the archive holds:
  * the target's turn, slot0 .. slot0+3, in full (every transaction's signature prefix, costUnits, fee, error flag,
    signature count, writable account count and requested compute budget, plus the complete wire bytes and meta of our
    own probes),
  * the preceding leader's turn, slot0-4 .. slot0-1, as signature prefixes (in full if one of our probes landed there),
  * any other block one of our probes landed in, in full.

Signature prefixes are the first 4 bytes (8 hex chars): enough to show that none of our probes sits in a block
unaccounted for (the completeness check), small enough to keep the archive cloneable.

Output under <out>/<run_id>/: run.json, quads.jsonl, sends.jsonl, streams.jsonl, windows.jsonl, gossip.jsonl.gz,
slots.jsonl.gz, blocks/<slot>.json.gz, outcomes.jsonl.
"""

from __future__ import annotations

import base64
import gzip
import hashlib
import json
import shutil
from pathlib import Path

from .rpc import Rpc, redact
from .txdecode import b58decode, b58encode, decode, first_signature

PRE_SLOTS = 4
TURN_SLOTS = 4


def read_jsonl(p: Path) -> list[dict]:
    if not p.exists():
        return []
    op = gzip.open if p.suffix == ".gz" else open
    with op(p, "rt") as f:
        return [json.loads(line) for line in f if line.strip()]


def write_jsonl(p: Path, rows, compress: bool = False):
    p.parent.mkdir(parents=True, exist_ok=True)
    data = "".join(json.dumps(r, sort_keys=True, separators=(",", ":")) + "\n" for r in rows)
    if compress:
        p.write_bytes(gzip.compress(data.encode(), mtime=0))
    else:
        p.write_text(data)


def sig4(sig_bytes: bytes) -> str:
    return sig_bytes[:4].hex()


def leader_from_rewards(block: dict) -> str | None:
    for r in block.get("rewards") or []:
        if r.get("rewardType") == "Fee":
            return r["pubkey"]
    return None


def _budget(raw: bytes, meta: dict) -> tuple:
    """(signatures, writable accounts incl. lookup-table ones, requested CU limit, CU price in micro-lamports,
    requested loaded-data limit) of one transaction; None where not requested (or not decodable)."""
    try:
        tx = decode(raw)
    except (ValueError, IndexError):
        return None, None, None, None, None
    cb = tx.compute_budget()
    n_w = len(tx.writable()) + len((meta.get("loadedAddresses") or {}).get("writable") or [])
    return len(tx.signatures), n_w, cb["cu_limit"], cb["cu_price_micro"], cb["loaded_data"]


def excerpt_full(slot: int, block: dict, ours: set[bytes]) -> dict:
    """Columnar excerpt of a full block; our probes in full."""
    sigs, cus, fees, errs, probes = [], [], [], [], []
    budget = {k: [] for k in ("n_sig", "n_writable", "cu_limit", "cu_price", "loaded_data")}
    for i, t in enumerate(block["transactions"]):
        raw = base64.b64decode(t["transaction"][0])
        s = first_signature(raw)
        m = t["meta"] or {}
        sigs.append(sig4(s))
        cus.append(m.get("costUnits"))
        fees.append(m.get("fee"))
        errs.append(1 if m.get("err") is not None else 0)
        for k, v in zip(budget, _budget(raw, m), strict=True):
            budget[k].append(v)
        if s in ours:
            probes.append({"signature": b58encode(s), "index": i, "tx_b64": t["transaction"][0],
                           "version": t.get("version"), "meta": m})
    return {"slot": slot, "mode": "full", "parent_slot": block.get("parentSlot"), "block_time": block.get("blockTime"),
            "blockhash": block.get("blockhash"), "block_height": block.get("blockHeight"),
            "leader": leader_from_rewards(block), "n_tx": len(sigs),
            "sum_cost_units": sum(c for c in cus if c is not None), "sig4": sigs, "cost_units": cus, "fee": fees,
            "err": errs, **budget, "our_probes": probes}


def excerpt_sigs(slot: int, block: dict) -> dict:
    sigs = [sig4(b58decode(s)) for s in block.get("signatures", [])]
    return {"slot": slot, "mode": "signatures", "parent_slot": block.get("parentSlot"),
            "block_time": block.get("blockTime"), "blockhash": block.get("blockhash"),
            "leader": leader_from_rewards(block), "n_tx": len(sigs), "sig4": sigs, "our_probes": []}


class BlockStore:
    def __init__(self, rpc: Rpc, root: Path):
        self.rpc = rpc
        self.root = root
        root.mkdir(parents=True, exist_ok=True)

    def path(self, slot: int) -> Path:
        return self.root / f"{slot}.json.gz"

    def load(self, slot: int) -> dict | None:
        p = self.path(slot)
        return json.loads(gzip.decompress(p.read_bytes())) if p.exists() else None

    def save(self, ex: dict):
        self.path(ex["slot"]).write_bytes(gzip.compress(json.dumps(ex, sort_keys=True,
                                                                   separators=(",", ":")).encode(), mtime=0))

    def get(self, slot: int, full: bool, ours: set[bytes]) -> dict:
        """Excerpt of a slot (cached). {"slot", "mode": "skipped"} for a skipped slot."""
        ex = self.load(slot)
        if ex and (ex["mode"] in ("skipped", "full") or not full):
            return ex
        params = {"encoding": "base64", "transactionDetails": "full" if full else "signatures", "rewards": True,
                  "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}
        try:
            block = self.rpc.call("getBlock", [slot, params])
        except Exception as e:  # noqa: BLE001 - RpcError for skipped / missing slots
            code = getattr(e, "code", None)
            if code in (-32007, -32009, -32004):
                ex = {"slot": slot, "mode": "skipped", "rpc_error": code}
                self.save(ex)
                return ex
            raise
        ex = excerpt_full(slot, block, ours) if full else excerpt_sigs(slot, block)
        self.save(ex)
        return ex


def load_slot_observations(paths, lo: int, hi: int) -> list[dict]:
    rows = []
    for p in paths:
        for r in read_jsonl(Path(p)):
            if lo <= r["slot"] <= hi:
                rows.append(r)
    rows.sort(key=lambda r: (r["slot"], r["src"], r["recv_ms"]))
    return rows


def collect_run(run_dir: Path, slot_files, out_root: Path, rpc: Rpc, log=print) -> Path:
    run_dir = Path(run_dir)
    quads = [q for q in read_jsonl(run_dir / "quads.jsonl")]
    if not quads:
        raise SystemExit(f"{run_dir}: no quads.jsonl")
    run_id = quads[0]["run_id"]
    out = out_root / run_id
    out.mkdir(parents=True, exist_ok=True)
    for name in ("quads.jsonl", "sends.jsonl", "streams.jsonl", "windows.jsonl"):
        if (run_dir / name).exists():
            shutil.copyfile(run_dir / name, out / name)
    if (run_dir / "gossip.jsonl").exists():
        (out / "gossip.jsonl.gz").write_bytes(gzip.compress((run_dir / "gossip.jsonl").read_bytes(), mtime=0))
    fired = [q for q in quads if q.get("status") == "fired"]
    slot_files = list(slot_files) + ([run_dir / "slots.jsonl"] if (run_dir / "slots.jsonl").exists() else [])
    if fired:
        lo = min(q["slot0"] for q in fired) - 2 * PRE_SLOTS
        hi = max(q["slot0"] for q in fired) + 2 * TURN_SLOTS
        write_jsonl(out / "slots.jsonl.gz", load_slot_observations(slot_files, lo, hi), compress=True)
    live = any(q.get("live") for q in quads)

    # our probe signatures, per quad
    probes = []
    for q in fired:
        for p in q["probes"]:
            probes.append({"quad_id": q["quad_id"], "role": p["role"], "probe": p["probe"], "signature": p["signature"],
                           "payer": p["payer"], "to": p.get("to"), "fee_expected": p["fee"], "slot0": q["slot0"],
                           "identity": q["identity"], "prev_leader": q.get("prev_leader")})
    ours = {b58decode(p["signature"]) for p in probes}
    store = BlockStore(rpc, out / "blocks")

    # status cross-check (live runs only: a dry run sent nothing)
    status = {}
    if live and probes:
        sigs = [p["signature"] for p in probes]
        for s, st in zip(sigs, rpc.signature_statuses(sigs), strict=True):
            status[s] = st

    found: dict[str, tuple[int, dict]] = {}
    for n, q in enumerate(fired):
        s0 = q["slot0"]
        for slot in range(s0 - PRE_SLOTS, s0 + TURN_SLOTS):
            full = slot >= s0 or any((st or {}).get("slot") == slot for st in
                                     (status.get(p["signature"]) for p in q["probes"]))
            ex = store.get(slot, full, ours)
            for p in ex.get("our_probes", []):
                found[p["signature"]] = (slot, p)
        if n % 50 == 0:
            log(f"{run_id}: {n}/{len(fired)} quads collected")
    # probes that landed outside the archived windows (status says so)
    for p in probes:
        st = status.get(p["signature"])
        if st and p["signature"] not in found and st.get("slot") is not None:
            ex = store.get(st["slot"], True, ours)
            for pp in ex.get("our_probes", []):
                found[pp["signature"]] = (st["slot"], pp)

    outcomes = []
    for p in probes:
        s0 = p["slot0"]
        st = status.get(p["signature"])
        hit = found.get(p["signature"])
        row = {"signature": p["signature"], "quad_id": p["quad_id"], "role": p["role"], "probe": p["probe"],
               "landed": hit is not None, "status_slot": (st or {}).get("slot"),
               "status_err": (st or {}).get("err"), "fee_expected": p["fee_expected"]}
        if hit:
            slot, pp = hit
            ex = store.load(slot)
            leader = ex.get("leader")
            in_turn = s0 <= slot < s0 + TURN_SLOTS and leader == p["identity"]
            pre = s0 - PRE_SLOTS <= slot < s0 and (p["prev_leader"] is None or leader == p["prev_leader"])
            row.update({"slot": slot, "slot_of_turn": slot - s0 if s0 <= slot < s0 + TURN_SLOTS else None,
                        "index_in_block": pp["index"], "block_len": ex["n_tx"], "leader": leader,
                        "in_target_turn": in_turn,
                        "landed_with": "target" if in_turn else ("preceding" if pre else "other"),
                        "meta_fee": pp["meta"].get("fee"), "meta_cost_units": pp["meta"].get("costUnits"),
                        "cu_consumed": pp["meta"].get("computeUnitsConsumed"), "err": pp["meta"].get("err"),
                        "block_cost_units": ex.get("sum_cost_units")})
        outcomes.append(row)

    # completeness: no probe of ours in an archived block without a matching outcome, and status agrees with blocks
    problems = completeness(store, probes, outcomes)
    write_jsonl(out / "outcomes.jsonl", outcomes)
    meta = {"run_id": run_id, "live": live, "quads": len(quads), "fired": len(fired), "probes": len(probes),
            "landed": sum(o["landed"] for o in outcomes), "first_slot": min((q["slot0"] for q in fired), default=None),
            "last_slot": max((q["slot0"] for q in fired), default=None), "completeness_problems": problems,
            "rpc_hosts": [redact(u) for u in rpc.urls], "source_dir": run_dir.name}
    (out / "run.json").write_text(json.dumps(meta, indent=1, sort_keys=True) + "\n")
    log(f"{run_id}: {meta['fired']} quads, {meta['landed']}/{meta['probes']} probes landed, "
        f"{len(problems)} completeness problems")
    return out


def completeness(store: BlockStore, probes: list[dict], outcomes: list[dict]) -> list[str]:
    by_sig4 = {}
    for p in probes:
        by_sig4.setdefault(sig4(b58decode(p["signature"])), []).append(p["signature"])
    landed = {o["signature"]: o for o in outcomes if o["landed"]}
    problems = []
    for f in sorted(store.root.glob("*.json.gz")):
        ex = json.loads(gzip.decompress(f.read_bytes()))
        for s4 in ex.get("sig4", []):
            for sig in by_sig4.get(s4, []):
                o = landed.get(sig)
                if o is None or o["slot"] != ex["slot"]:
                    problems.append(f"{sig} appears in block {ex['slot']} but its outcome says "
                                    f"{'not landed' if o is None else 'slot ' + str(o['slot'])}")
    for o in outcomes:
        if o["status_slot"] is not None and (not o["landed"] or o["slot"] != o["status_slot"]):
            problems.append(f"{o['signature']}: status slot {o['status_slot']} but block scan says "
                            f"{o.get('slot') if o['landed'] else 'not landed'}")
    return problems


def sha256_bytes(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()

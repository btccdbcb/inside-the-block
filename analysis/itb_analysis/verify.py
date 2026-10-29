"""Verifier: re-checks every landing claim from the archived blocks (offline) and, with --rpc, against a public RPC.

Needs no keys and none of the sender's code: the probe library below is an independent copy of docs/PREREGISTRATION.md
§2, and transactions are decoded by txdecode.py. For each landed probe (all claims, or a seeded sample of N):

  1. the archived transaction bytes decode, every signature verifies, and the first one is the claimed signature;
  2. the bytes equal the signed bytes the sender logged before sending (sends.jsonl);
  3. the requested limits equal the planned probe at the quad's fee scale (CU limit, CU price or v1 priority fee,
     v1 loaded-data limit 64 MiB);
  4. the only value moved is the planned 1 lamport from the fee payer to the planned recipient;
  5. meta.fee = 5,000 + priority fee, meta.costUnits = the predicted value, no error;
  6. a probe counted in the target's turn sits in a block whose Fee-reward recipient is the target;
  7. (--rpc) the block fetched again from the RPC has this transaction, byte for byte, at the claimed index, and the
     same Fee-reward recipient.
For each probe claimed not landed:
  8. its signature prefix is in none of the archived blocks;
  9. (--rpc) getSignatureStatuses, searching the full history, returns nothing for it.
 10. (--rpc) for every quad sampled, the first block of the target's turn as archived (signature list, leader) equals
     the block fetched again.
Then every coded pair's order (pref_first) is recomputed from the verified (slot, index) of its two members.
"""

from __future__ import annotations

import base64
import gzip
import json
import random
import struct
from pathlib import Path

from .collect import leader_from_rewards, read_jsonl
from .txdecode import SYSTEM, b58decode, b58encode, decode, first_signature

LAMPORTS_PER_SIGNATURE = 5_000
V1_LOADED_DATA = 64 * 1024 * 1024
LEGACY = {"A": (1_000, 100_000), "B": (6_000, 25_000), "HI": (1_000, 300_000), "LO": (1_200, 80_000),
          "P": (1_000, 200_000), "Q": (4_000, 190_000), "X": (1_000, 10_000), "Z": (1_200, 500_000)}
V1_OF = {"A1": "A", "HI1": "HI"}
V1_CU_LIMIT = 1_003
SCALE = {"s1": 1, "s10": 10}
COST_UNITS = {"pair": 1_784, "self": 1_484, "v1": 1_481}


def priority_fee(limit: int, price_micro: int) -> int:
    return -(-limit * price_micro // 1_000_000)


def planned(probe: str, scale: str) -> dict:
    f = SCALE[scale]
    if probe in V1_OF:
        lim, price = LEGACY[V1_OF[probe]]
        return {"format": "v1", "cu_limit": V1_CU_LIMIT, "priority_fee": priority_fee(lim, price * f),
                "loaded_data": V1_LOADED_DATA}
    lim, price = LEGACY[probe]
    return {"format": "legacy", "cu_limit": lim, "cu_price_micro": price * f,
            "priority_fee": priority_fee(lim, price * f)}


def system_transfers(tx) -> list[tuple[str, str, int]]:
    out = []
    for ix in tx.instructions:
        if tx.accounts[ix.program_index] != SYSTEM:
            continue
        if len(ix.data) == 12 and struct.unpack_from("<I", ix.data, 0)[0] == 2:
            out.append((tx.accounts[ix.accounts[0]], tx.accounts[ix.accounts[1]],
                        struct.unpack_from("<Q", ix.data, 4)[0]))
        else:
            out.append(("?", "?", -1))
    return out


def check_probe(p: dict, quad: dict, ex_probe: dict, block: dict, sent_b64: str | None, outcome: dict) -> list[str]:
    errs = []
    raw = base64.b64decode(ex_probe["tx_b64"])
    tx = decode(raw)
    if b58encode(first_signature(raw)) != p["signature"] or tx.signature != p["signature"]:
        errs.append("signature mismatch")
    if not tx.signatures_valid():
        errs.append("signature does not verify")
    if sent_b64 is None:
        errs.append("no logged send")
    elif base64.b64decode(sent_b64) != raw:
        errs.append("landed bytes differ from the logged signed bytes")
    want = planned(p["probe"], quad.get("fee_scale", "s1"))
    if tx.format != want["format"]:
        errs.append(f"format {tx.format}, planned {want['format']}")
    cb = tx.compute_budget()
    for k in ("cu_limit", "priority_fee", "loaded_data", "cu_price_micro"):
        if k in want and cb.get(k) != want[k]:
            errs.append(f"{k} {cb.get(k)}, planned {want[k]}")
    payer = tx.accounts[0]
    if payer != p["payer"]:
        errs.append(f"fee payer {payer}, planned {p['payer']}")
    if system_transfers(tx) != [(p["payer"], p["to"], 1)]:
        errs.append(f"transfers {system_transfers(tx)}, planned 1 lamport {p['payer']} -> {p['to']}")
    meta = ex_probe["meta"]
    if meta.get("err") is not None:
        errs.append(f"error {meta.get('err')}")
    if meta.get("fee") != LAMPORTS_PER_SIGNATURE + want["priority_fee"]:
        errs.append(f"fee {meta.get('fee')}, formula {LAMPORTS_PER_SIGNATURE + want['priority_fee']}")
    main = p["role"].startswith("main")
    shape = "v1" if p["probe"] in V1_OF else ("self" if main and quad["main_type"] == "R_IND" else "pair")
    if meta.get("costUnits") != COST_UNITS[shape]:
        errs.append(f"costUnits {meta.get('costUnits')}, predicted {COST_UNITS[shape]}")
    if outcome.get("in_target_turn") and block.get("leader") != quad["identity"]:
        errs.append(f"counted in the target's turn but the block's leader is {block.get('leader')}")
    return errs


def check_rpc(rpc, slot: int, index: int, raw_b64: str, leader: str | None) -> list[str]:
    params = {"encoding": "base64", "transactionDetails": "full", "rewards": True,
              "maxSupportedTransactionVersion": 1, "commitment": "finalized"}
    block = rpc.call("getBlock", [slot, params])
    txs = block["transactions"]
    errs = []
    if index >= len(txs) or txs[index]["transaction"][0] != raw_b64:
        errs.append(f"RPC block {slot} does not hold this transaction at index {index}")
    if leader_from_rewards(block) != leader:
        errs.append(f"RPC block {slot} leader {leader_from_rewards(block)}, archive says {leader}")
    return errs


def check_rpc_block(rpc, ex: dict) -> list[str]:
    """Re-fetch an archived block (signatures only) and compare its signature list and Fee-reward leader."""
    params = {"encoding": "base64", "transactionDetails": "signatures", "rewards": True,
              "maxSupportedTransactionVersion": 1, "commitment": "finalized"}
    block = rpc.call("getBlock", [ex["slot"], params])
    errs = []
    if [b58decode(x)[:4].hex() for x in block.get("signatures", [])] != ex.get("sig4"):
        errs.append(f"RPC block {ex['slot']} signature list differs from the archive")
    if leader_from_rewards(block) != ex.get("leader"):
        errs.append(f"RPC block {ex['slot']} leader {leader_from_rewards(block)}, archive says {ex.get('leader')}")
    return errs


def _sig4(signature: str) -> str:
    return b58decode(signature)[:4].hex()


def verify(raw_root: Path, sample: int | None = None, seed: int = 0, rpc=None, log=print) -> dict:
    """Check every landing claim (or a seeded sample of N claims, landed and not landed alike)."""
    items = []
    for run in sorted(Path(raw_root).iterdir()):
        if not (run / "outcomes.jsonl").exists():
            continue
        quads = {q["quad_id"]: q for q in read_jsonl(run / "quads.jsonl") if q.get("status") == "fired"}
        sends = {s["signature"]: s.get("tx_b64") for s in read_jsonl(run / "sends.jsonl")}
        for o in read_jsonl(run / "outcomes.jsonl"):
            items.append((run, quads[o["quad_id"]], o, sends.get(o["signature"])))
    chosen = items if not sample or sample >= len(items) else random.Random(seed).sample(items, sample)
    failures, landed_checked = [], 0
    blocks: dict[tuple, dict] = {}
    archive_sig4: dict[Path, dict[str, int]] = {}

    def block(run, slot):
        if (run, slot) not in blocks:
            blocks[(run, slot)] = json.loads(gzip.decompress((run / "blocks" / f"{slot}.json.gz").read_bytes()))
        return blocks[(run, slot)]

    unlanded, turn_blocks = [], set()
    for run, quad, o, sent in chosen:
        if rpc is not None and (run, quad["slot0"]) not in turn_blocks:
            # 10. (--rpc) the first block of the target's turn, as archived, matches the RPC
            turn_blocks.add((run, quad["slot0"]))
            if (run / "blocks" / f"{quad['slot0']}.json.gz").exists():
                errs = check_rpc_block(rpc, block(run, quad["slot0"]))
                if errs:
                    failures.append({"run": run.name, "quad_id": quad["quad_id"], "errors": errs})
        if not o["landed"]:
            # 8. a probe claimed not landed sits in no archived block (signature prefix scan)
            if run not in archive_sig4:
                archive_sig4[run] = {}
                for f in (run / "blocks").glob("*.json.gz"):
                    ex = block(run, int(f.name.split(".")[0]))
                    for s4 in ex.get("sig4", []):
                        archive_sig4[run][s4] = ex["slot"]
            errs = []
            hit = archive_sig4[run].get(_sig4(o["signature"]))
            if hit is not None:
                errs.append(f"claimed not landed, but its signature prefix is in archived block {hit}")
            unlanded.append(o["signature"])
        else:
            landed_checked += 1
            b = block(run, o["slot"])
            ex = next((x for x in b["our_probes"] if x["signature"] == o["signature"]), None)
            p = next(x for x in quad["probes"] if x["signature"] == o["signature"])
            errs = ["not in the archived block"] if ex is None else check_probe(p, quad, ex, b, sent, o)
            if ex is not None and ex["index"] != o["index_in_block"]:
                errs.append(f"index {ex['index']} in block, outcome says {o['index_in_block']}")
            if b.get("sig4") and b["sig4"][o["index_in_block"]] != _sig4(o["signature"]):
                errs.append("block signature list does not have this signature at the claimed index")
            if ex is not None and rpc is not None:
                errs += check_rpc(rpc, o["slot"], ex["index"], ex["tx_b64"], b.get("leader"))
        if errs:
            failures.append({"run": run.name, "quad_id": quad["quad_id"], "signature": o["signature"], "errors": errs})
    if rpc is not None and unlanded:
        # 9. (--rpc) the RPC has no status for a probe claimed not landed (full transaction history searched)
        for sig, st in zip(unlanded, rpc.signature_statuses(unlanded), strict=True):
            if st is not None:
                failures.append({"signature": sig, "errors": [f"claimed not landed, RPC status slot {st.get('slot')}"]})
    log(f"verify: {len(chosen)} claims checked ({landed_checked} landed, {len(unlanded)} not landed)"
        f"{' against RPC' if rpc else ''}, {len(failures)} failed")
    return {"checked": len(chosen), "landed_checked": landed_checked, "unlanded_checked": len(unlanded),
            "turn_blocks_rechecked": len(turn_blocks), "of_claims": len(items), "sample": sample, "seed": seed,
            "rpc": rpc is not None,
            "failures": failures, "pass": not failures}


def verify_pairs(pairs_csv: Path, raw_root: Path) -> dict:
    """Recompute pref_first of every coded pair from the archived (slot, index) of its members."""
    import pandas as pd

    pos, sig = {}, {}
    for run in sorted(Path(raw_root).iterdir()):
        if not (run / "outcomes.jsonl").exists():
            continue
        for o in read_jsonl(run / "outcomes.jsonl"):
            if o["landed"]:
                pos[o["signature"]] = (o["slot"], o["index_in_block"])
        for q in read_jsonl(run / "quads.jsonl"):
            if q.get("status") == "fired":
                for p in q["probes"]:
                    sig[(q["quad_id"], p["role"])] = p["signature"]
    pairs = pd.read_csv(pairs_csv)
    bad = []
    for r in pairs[pairs.both_in_turn].itertuples():
        a = pos.get(sig.get((r.quad_id, f"{r.pair}_pref")))
        b = pos.get(sig.get((r.quad_id, f"{r.pair}_other")))
        if a is None or b is None or (a < b) != bool(r.pref_first):
            bad.append(f"{r.quad_id}/{r.pair}")
    return {"pairs_checked": int(pairs.both_in_turn.sum()), "mismatches": bad, "pass": not bad}

"""The pre-registration Memo, checked from the chain (REPORT §1, `itb-analysis memo-check`).

The live programme posts one Memo after the pilot and before the first campaign probe:

  inside-the-block prereg design.jsonl=<sha> campaign.json=<sha> PREREGISTRATION.md=<sha> pilot_decision=<sha>

This reads the Memo's signature from the ledger export, fetches the transaction from a public RPC and checks that
it landed without error, that its text names the sha256 of the committed files, that `campaign.json` records the same
pre-registration hash, which confirmatory-code files still have the hash recorded at the freeze, and that the Memo's
block came before the first campaign probe was sent and before the first slot of any campaign quad.
"""

from __future__ import annotations

import base64
import csv
import hashlib
import json
from pathlib import Path

from .txdecode import decode

MEMO_PROGRAMS = {"MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo"}
PREFIX = "inside-the-block prereg "
CAMPAIGN_SESSIONS = {"A", "B", "C", "D"}


def _sha(p: Path) -> str:
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def parse(text: str) -> dict[str, str]:
    if not text.startswith(PREFIX):
        raise ValueError(f"not a pre-registration memo: {text[:60]!r}")
    return dict(f.split("=", 1) for f in text[len(PREFIX):].split())


def memo_text(tx_b64: str) -> str:
    t = decode(base64.b64decode(tx_b64))
    texts = [bytes(ix.data).decode() for ix in t.instructions if t.accounts[ix.program_index] in MEMO_PROGRAMS]
    if len(texts) != 1:
        raise ValueError(f"expected one memo instruction, found {len(texts)}")
    return texts[0]


def first_campaign_probe(raw: Path) -> dict:
    """Earliest send and earliest turn among fired campaign quads (sessions A-D) in data/raw."""
    sends, slots = [], []
    for d in sorted(p for p in Path(raw).glob("*") if p.is_dir()):
        f = d / "quads.jsonl"
        if not f.exists():
            continue
        for line in f.read_text().splitlines():
            q = json.loads(line)
            if q.get("session") in CAMPAIGN_SESSIONS and q.get("status") == "fired":
                sends.append((q["first_send_ms"], q["quad_id"]))
                slots.append(q["slot0"])
    if not sends:
        return {}
    ms, qid = min(sends)
    return {"first_send_ms": ms, "quad_id": qid, "min_slot0": min(slots), "quads": len(sends)}


def check(ledger_csv: Path, repo: Path, pilot: Path, raw: Path, rpc, log=print) -> dict:
    rows = [r for r in csv.DictReader(Path(ledger_csv).open()) if r["kind"] == "memo"]
    landed = [r for r in rows if r["status"] == "landed"]
    problems = []
    if len(landed) != 1:
        problems.append(f"{len(landed)} landed memo rows in the ledger (expected 1; {len(rows)} in all)")
    if not landed:
        return {"pass": False, "problems": problems}
    sig = landed[0]["signature"]
    tx = rpc.call("getTransaction", [sig, {"encoding": "base64", "maxSupportedTransactionVersion": 1,
                                           "commitment": "finalized"}])
    if not tx or not tx.get("meta"):
        return {"pass": False, "signature": sig, "problems": problems + [f"memo {sig} not returned by the RPC"]}
    if tx["meta"].get("err") is not None:
        problems.append(f"memo transaction failed on chain: {tx['meta']['err']}")
    text = memo_text(tx["transaction"][0])
    fields = parse(text)

    campaign = json.loads((repo / "design" / "campaign.json").read_text())
    expected = {"design.jsonl": _sha(repo / "design" / "design.jsonl"),
                "campaign.json": _sha(repo / "design" / "campaign.json"),
                "PREREGISTRATION.md": _sha(repo / "docs" / "PREREGISTRATION.md"),
                "pilot_decision": _sha(pilot)}
    for k, v in expected.items():
        if fields.get(k) != v:
            problems.append(f"{k}: memo {fields.get(k)!r}, committed file {v}")
    payer = decode(base64.b64decode(tx["transaction"][0])).accounts[0]
    if payer != campaign.get("funder"):
        problems.append(f"memo fee payer {payer} is not the campaign funder {campaign.get('funder')}")
    if campaign.get("prereg_sha256") != expected["PREREGISTRATION.md"]:
        problems.append(f"campaign.json prereg_sha256 {campaign.get('prereg_sha256')!r} is not the committed "
                        f"PREREGISTRATION.md {expected['PREREGISTRATION.md']}")
    code = {f: {"frozen": h, "now": _sha(repo / f) if (repo / f).exists() else None}
            for f, h in (campaign.get("confirmatory_code_sha256") or {}).items()}
    if not code:
        problems.append("campaign.json records no confirmatory_code_sha256")

    first = first_campaign_probe(raw)
    memo_ms = tx["blockTime"] * 1000 if tx.get("blockTime") is not None else None
    if not first:
        problems.append("no fired campaign quad in data/raw")
    else:
        if tx["slot"] >= first["min_slot0"]:
            problems.append(f"memo slot {tx['slot']} is not before the first campaign turn {first['min_slot0']}")
        if memo_ms is None or memo_ms + 1000 > first["first_send_ms"]:
            problems.append(f"memo block time {memo_ms} ms is not before the first campaign send "
                            f"{first['first_send_ms']} ms")
    res = {"signature": sig, "slot": tx["slot"], "block_time": tx.get("blockTime"), "text": text,
           "fee_payer": payer, "expected": expected,
           "confirmatory_code": code, "code_changed_since_freeze": sorted(f for f, c in code.items()
                                                                          if c["frozen"] != c["now"]),
           "first_campaign_probe": first, "problems": problems, "pass": not problems}
    log(f"memo {sig} slot {tx['slot']}: {text}")
    if first:
        log(f"first campaign probe: quad {first['quad_id']} sent at {first['first_send_ms']} ms, "
            f"first campaign turn slot {first['min_slot0']}")
    for f in res["code_changed_since_freeze"]:
        log(f"confirmatory code changed since the freeze: {f} (a deviation, REPORT §9)")
    for p in problems:
        log(p)
    log("MEMO OK" if res["pass"] else "MEMO CHECK FAILED")
    return res

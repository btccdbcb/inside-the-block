import base64
import hashlib
import json

from itb_analysis import memo
from itb_analysis.txdecode import b58decode, b58encode

FUNDER = b58encode(bytes([1]) * 32)
MEMO_V2 = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"


def _compact(n: int) -> bytes:
    out = bytearray()
    while True:
        b, n = n & 0x7F, n >> 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return bytes(out)


def _memo_tx(text: str) -> str:
    data = text.encode()
    msg = (bytes([1, 0, 1, 2]) + b58decode(FUNDER) + b58decode(MEMO_V2) + bytes(32)
           + bytes([1, 1, 0]) + _compact(len(data)) + data)
    return base64.b64encode(bytes([1]) + bytes(64) + msg).decode()


class FakeRpc:
    def __init__(self, text, slot, block_time, err=None):
        self.tx = {"slot": slot, "blockTime": block_time, "transaction": [_memo_tx(text), "base64"],
                   "meta": {"err": err, "fee": 5000}}

    def call(self, method, params):
        assert method == "getTransaction" and params[0] == "M"
        return self.tx


def _sha(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def _setup(tmp_path, first_send_ms=2_000_000_000_000, slot0=500):
    (tmp_path / "design").mkdir()
    (tmp_path / "docs").mkdir()
    (tmp_path / "analysis/itb_analysis").mkdir(parents=True)
    prereg, code = b"rules", b"code"
    (tmp_path / "docs/PREREGISTRATION.md").write_bytes(prereg)
    (tmp_path / "analysis/itb_analysis/analysis.py").write_bytes(code)
    (tmp_path / "design/design.jsonl").write_text("{}\n")
    campaign = {"funder": FUNDER, "prereg_sha256": _sha(prereg),
                "confirmatory_code_sha256": {"analysis/itb_analysis/analysis.py": _sha(code)}}
    (tmp_path / "design/campaign.json").write_text(json.dumps(campaign))
    (tmp_path / "pilot_decision.json").write_text("{}")
    run = tmp_path / "raw/r1"
    run.mkdir(parents=True)
    quads = [{"quad_id": "p1", "session": "pilot", "status": "fired", "first_send_ms": 1, "slot0": 1},
             {"quad_id": "a1", "session": "A", "status": "fired", "first_send_ms": first_send_ms, "slot0": slot0},
             {"quad_id": "a0", "session": "A", "status": "skipped"}]
    (run / "quads.jsonl").write_text("".join(json.dumps(q) + "\n" for q in quads))
    head = "id,created_utc,kind,sub_budget,run_id,signature,fee_payer,fee_lamports,status,slot\n"
    (tmp_path / "ledger.csv").write_text(head + f"1,t,memo,setup,memo,M,{FUNDER},5000,landed,100\n")
    text = "inside-the-block prereg " + " ".join(
        f"{k}={_sha((tmp_path / p).read_bytes())}" for k, p in [
            ("design.jsonl", "design/design.jsonl"), ("campaign.json", "design/campaign.json"),
            ("PREREGISTRATION.md", "docs/PREREGISTRATION.md"), ("pilot_decision", "pilot_decision.json")])
    return text


def _check(tmp_path, rpc):
    return memo.check(tmp_path / "ledger.csv", tmp_path, tmp_path / "pilot_decision.json", tmp_path / "raw", rpc,
                      log=lambda *_: None)


def test_memo_ok(tmp_path):
    text = _setup(tmp_path)
    res = _check(tmp_path, FakeRpc(text, slot=100, block_time=1_900_000_000))
    assert res["pass"], res["problems"]
    assert res["first_campaign_probe"]["quad_id"] == "a1"
    assert res["code_changed_since_freeze"] == []


def test_memo_after_first_probe_or_wrong_hash(tmp_path):
    text = _setup(tmp_path, slot0=100)
    res = _check(tmp_path, FakeRpc(text, slot=100, block_time=2_100_000_000))
    assert not res["pass"] and len(res["problems"]) == 2
    (tmp_path / "docs/PREREGISTRATION.md").write_bytes(b"rules, edited")
    res = _check(tmp_path, FakeRpc(text, slot=10, block_time=1_900_000_000))
    assert any("PREREGISTRATION.md" in p for p in res["problems"]) and not res["pass"]


def test_code_change_is_reported_not_failed(tmp_path):
    text = _setup(tmp_path)
    (tmp_path / "analysis/itb_analysis/analysis.py").write_bytes(b"code, fixed")
    res = _check(tmp_path, FakeRpc(text, slot=100, block_time=1_900_000_000))
    assert res["pass"] and res["code_changed_since_freeze"] == ["analysis/itb_analysis/analysis.py"]

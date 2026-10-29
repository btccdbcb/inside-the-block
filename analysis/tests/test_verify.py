import gzip
import json
import shutil
from pathlib import Path

from itb_analysis.verify import planned, verify

FIXTURE = Path(__file__).parent / "fixtures" / "localnet-smoke"


def _copy(tmp_path):
    root = tmp_path / "raw"
    shutil.copytree(FIXTURE, root)
    return root, next(root.iterdir())


def _edit_block(run, edit):
    path = sorted((run / "blocks").glob("*.json.gz"), key=lambda p: -p.stat().st_size)[0]
    block = json.loads(gzip.decompress(path.read_bytes()))
    edit(block)
    path.write_bytes(gzip.compress(json.dumps(block).encode()))


def test_localnet_probes_verify():
    res = verify(FIXTURE, log=lambda *_: None)
    assert res["landed_checked"] == 16 and res["pass"], res["failures"]


def test_sample_is_seeded():
    a = verify(FIXTURE, sample=5, seed=7, log=lambda *_: None)
    assert a["checked"] == 5 and a["pass"]


def test_wrong_fee_is_caught(tmp_path):
    root, run = _copy(tmp_path)
    _edit_block(run, lambda b: b["our_probes"][0]["meta"].update(fee=b["our_probes"][0]["meta"]["fee"] + 1))
    res = verify(root, log=lambda *_: None)
    assert not res["pass"] and any("fee" in e for f in res["failures"] for e in f["errors"])


def test_altered_bytes_are_caught(tmp_path):
    import base64

    def flip(b):
        raw = bytearray(base64.b64decode(b["our_probes"][0]["tx_b64"]))
        raw[-1] ^= 1
        b["our_probes"][0]["tx_b64"] = base64.b64encode(bytes(raw)).decode()

    root, run = _copy(tmp_path)
    _edit_block(run, flip)
    res = verify(root, log=lambda *_: None)
    errors = [e for f in res["failures"] for e in f["errors"]]
    assert "signature does not verify" in errors


def test_wrong_leader_is_caught(tmp_path):
    root, run = _copy(tmp_path)
    lines = [json.loads(x) for x in (run / "outcomes.jsonl").read_text().splitlines()]
    for o in lines:
        if o["landed"]:
            o["in_target_turn"] = True
    (run / "outcomes.jsonl").write_text("".join(json.dumps(o) + "\n" for o in lines))
    _edit_block(run, lambda b: b.update(leader="11111111111111111111111111111111"))
    res = verify(root, log=lambda *_: None)
    assert any("leader" in e for f in res["failures"] for e in f["errors"])


def test_planned_fees():
    assert planned("A", "s1") == {"format": "legacy", "cu_limit": 1000, "cu_price_micro": 100_000, "priority_fee": 100}
    assert planned("HI1", "s10")["priority_fee"] == 3000
    assert planned("A1", "s1")["loaded_data"] == 64 * 1024 * 1024


def test_hidden_probe_is_caught(tmp_path):
    root, run = _copy(tmp_path)
    lines = [json.loads(x) for x in (run / "outcomes.jsonl").read_text().splitlines()]
    lines[0] = {k: lines[0][k] for k in ("signature", "quad_id", "role", "probe", "fee_expected")} | {"landed": False}
    (run / "outcomes.jsonl").write_text("".join(json.dumps(o) + "\n" for o in lines))
    res = verify(root, log=lambda *_: None)
    assert any("claimed not landed" in e for f in res["failures"] for e in f["errors"])

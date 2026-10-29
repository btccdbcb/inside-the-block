from itb_analysis.collect import BlockStore, completeness, sig4
from itb_analysis.txdecode import b58decode, b58encode

SIG_A = b58encode(bytes(range(64)))
SIG_B = b58encode(bytes(range(1, 65)))


def _store(tmp_path, blocks):
    store = BlockStore(rpc=None, root=tmp_path / "blocks")
    for slot, sigs in blocks.items():
        store.save({"slot": slot, "mode": "signatures", "sig4": [sig4(b58decode(s)) for s in sigs], "our_probes": []})
    return store


def test_completeness_passes_when_outcomes_match(tmp_path):
    store = _store(tmp_path, {10: ["1" * 64, SIG_A], 11: []})
    probes = [{"signature": SIG_A}, {"signature": SIG_B}]
    outcomes = [{"signature": SIG_A, "landed": True, "slot": 10, "status_slot": None},
                {"signature": SIG_B, "landed": False, "status_slot": None}]
    assert completeness(store, probes, outcomes) == []


def test_hidden_probe_fails_loudly(tmp_path):
    # SIG_B sits in archived block 11, but its outcome says it did not land
    store = _store(tmp_path, {10: [SIG_A], 11: [SIG_B]})
    probes = [{"signature": SIG_A}, {"signature": SIG_B}]
    outcomes = [{"signature": SIG_A, "landed": True, "slot": 10, "status_slot": None},
                {"signature": SIG_B, "landed": False, "status_slot": None}]
    problems = completeness(store, probes, outcomes)
    assert len(problems) == 1 and "block 11" in problems[0] and "not landed" in problems[0]


def test_status_disagreeing_with_blocks_fails(tmp_path):
    store = _store(tmp_path, {10: []})
    probes = [{"signature": SIG_A}]
    outcomes = [{"signature": SIG_A, "landed": False, "status_slot": 12}]
    assert "status slot 12" in completeness(store, probes, outcomes)[0]

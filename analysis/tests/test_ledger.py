import base64
import json

from itb_analysis import ledger
from itb_analysis.txdecode import b58encode

FUNDER, PAYER, OTHER, SWEEP = (b58encode(bytes([i]) * 32) for i in (1, 2, 3, 4))


def _tx(keys: list[str]) -> str:
    """A legacy transaction with these account keys and no instructions (enough for the decoder)."""
    from itb_analysis.txdecode import b58decode

    msg = bytes([1, 0, 0, len(keys)]) + b"".join(b58decode(k) for k in keys) + bytes(32) + bytes([0])
    return base64.b64encode(bytes([1]) + bytes(64) + msg).decode()


class FakeRpc:
    def __init__(self, txs, balances):
        self.txs, self.balances = txs, balances

    def each(self):
        return [self]

    def call(self, method, params):
        if method == "getSignaturesForAddress":
            newest_first = sorted(self.txs.items(), key=lambda kv: -kv[1]["slot"])
            return [{"signature": s, "slot": t["slot"]} for s, t in newest_first if params[0] in t["keys"]]
        if method == "getTransaction":
            t = self.txs[params[0]]
            return {"transaction": [_tx(t["keys"]), "base64"],
                    "meta": {"fee": t["fee"], "preBalances": t["pre"], "postBalances": t["post"]}}
        if method == "getBalance":
            return {"value": self.balances[params[0]]}
        raise KeyError(method)


def _setup(tmp_path, ledger_fee=5000, extra_tx=False):
    txs = {
        # fund: funder pays 5,000 and moves 100,000 to the payer
        "s1": {"slot": 11, "keys": [FUNDER, PAYER], "fee": 5000, "pre": [1_000_000, 0], "post": [895_000, 100_000]},
        # probe: payer pays 5,096 and moves 1 lamport to OTHER (not ours)
        "s2": {"slot": 12, "keys": [PAYER, OTHER], "fee": 5096, "pre": [100_000, 7], "post": [94_903, 8]},
        # sweep: payer sends the rest to SWEEP
        "s3": {"slot": 13, "keys": [PAYER, SWEEP], "fee": 5000, "pre": [94_903, 0], "post": [0, 89_903]},
    }
    if extra_tx:
        txs["s4"] = {"slot": 14, "keys": [FUNDER, SWEEP], "fee": 5000, "pre": [895_000, 89_903],
                     "post": [890_000, 89_903]}
    balances = {FUNDER: 890_000 if extra_tx else 895_000, PAYER: 0}
    (tmp_path / "live.log").write_text("\n".join(json.dumps(e) for e in [
        {"event": "account_registered", "pubkey": FUNDER, "role": "funder", "baseline": 1_000_000, "slot": 10},
        {"event": "account_registered", "pubkey": PAYER, "role": "payer-00", "baseline": 0, "slot": 10}]) + "\n")
    rows = [("1", "fund", "s1", FUNDER, ledger_fee), ("2", "probe", "s2", PAYER, 5096),
            ("3", "sweep", "s3", PAYER, 5000)]
    head = "id,created_utc,kind,sub_budget,run_id,signature,fee_payer,fee_lamports,status,slot\n"
    (tmp_path / "ledger.csv").write_text(head + "".join(f"{i},t,{k},b,r,{s},{p},{f},landed,0\n"
                                                        for i, k, s, p, f in rows))
    return FakeRpc(txs, balances)


def test_ledger_equals_chain(tmp_path):
    rpc = _setup(tmp_path)
    res = ledger.check(tmp_path / "ledger.csv", tmp_path / "live.log", rpc, log=lambda *_: None)
    assert res["totals"] == {"ledger": 15096, "chain": 15096, "balances": 15096}
    assert res["left_to_others"] == 89_904 and res["pass"]


def test_wrong_fee_and_unrecorded_spend_fail(tmp_path):
    rpc = _setup(tmp_path, ledger_fee=4000)
    res = ledger.check(tmp_path / "ledger.csv", tmp_path / "live.log", rpc, log=lambda *_: None)
    assert not res["pass"] and res["mismatches"]
    rpc = _setup(tmp_path, extra_tx=True)
    res = ledger.check(tmp_path / "ledger.csv", tmp_path / "live.log", rpc, log=lambda *_: None)
    assert not res["pass"] and res["unrecorded"] == ["s4"]


class ShortHistoryRpc(FakeRpc):
    """An endpoint that lists only recent signatures (slot >= 13) but serves every transaction."""

    def call(self, method, params):
        res = super().call(method, params)
        return [s for s in res if s["slot"] >= 13] if method == "getSignaturesForAddress" else res


class TwoEndpoints:
    def __init__(self, *clients):
        self.clients = clients

    def each(self):
        return list(self.clients)

    def call(self, method, params):
        return self.clients[0].call(method, params)


class NullOnceRpc(FakeRpc):
    """An endpoint that answers null the first time it is asked for s2."""

    def __init__(self, txs, balances):
        super().__init__(txs, balances)
        self.nulled = False

    def call(self, method, params):
        if method == "getTransaction" and params[0] == "s2" and not self.nulled:
            self.nulled = True
            return None
        return super().call(method, params)


def test_a_null_transaction_is_asked_again(tmp_path):
    full = _setup(tmp_path)
    res = ledger.check(tmp_path / "ledger.csv", tmp_path / "live.log", NullOnceRpc(full.txs, full.balances),
                       log=lambda *_: None)
    assert res["pass"] and not res["unavailable"]


def test_a_short_listing_on_one_endpoint_is_completed_by_the_other(tmp_path):
    full = _setup(tmp_path)
    short = ShortHistoryRpc(full.txs, full.balances)
    alone = ledger.check(tmp_path / "ledger.csv", tmp_path / "live.log", TwoEndpoints(short), log=lambda *_: None)
    assert not alone["pass"] and alone["balance_gaps"]
    both = ledger.check(tmp_path / "ledger.csv", tmp_path / "live.log", TwoEndpoints(short, full), log=lambda *_: None)
    assert both["pass"] and both["totals"]["chain"] == 15096

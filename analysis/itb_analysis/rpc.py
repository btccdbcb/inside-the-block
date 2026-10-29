"""Minimal JSON-RPC client over keyless public endpoints (stdlib only): rotation, pacing, retries for reads."""

from __future__ import annotations

import json
import os
import random
import time
import urllib.error
import urllib.request

DEFAULT_RPC = [
    "https://api.mainnet-beta.solana.com",
    "https://solana-rpc.publicnode.com",
    "https://public.rpc.solanavibestation.com",
]
USER_AGENT = "Mozilla/5.0 (X11; Linux x86_64) itb-study"


class RpcError(Exception):
    def __init__(self, code, message):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


class Rpc:
    def __init__(self, urls=None, min_gap_s=0.15, timeout=60):
        env = os.environ.get("ITB_RPC_URLS", "")
        self.urls = urls or [u.strip() for u in env.split(",") if u.strip()] or list(DEFAULT_RPC)
        self.min_gap_s = min_gap_s
        self.timeout = timeout
        self._last = {u: 0.0 for u in self.urls}
        self._i = 0

    def _post(self, url, body):
        wait = self._last[url] + self.min_gap_s - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        self._last[url] = time.monotonic()
        headers = {"Content-Type": "application/json", "User-Agent": USER_AGENT}
        req = urllib.request.Request(url, data=body, headers=headers)
        with urllib.request.urlopen(req, timeout=self.timeout) as r:
            return json.loads(r.read())

    def call(self, method, params, tries=8):
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        last = None
        for k in range(tries):
            url = self.urls[self._i % len(self.urls)]
            self._i += 1
            try:
                v = self._post(url, body)
            except (urllib.error.URLError, TimeoutError, OSError, json.JSONDecodeError) as e:
                last = e
                time.sleep(min(8.0, 0.5 * 2**k) * (0.5 + random.random()))
                continue
            if "error" in v:
                err = v["error"]
                code = err.get("code")
                # skipped / unavailable slots are answers, not failures
                if code in (-32007, -32009, -32004, -32001):
                    raise RpcError(code, err.get("message", ""))
                last = RpcError(code, err.get("message", ""))
                time.sleep(min(8.0, 0.5 * 2**k) * (0.5 + random.random()))
                continue
            return v["result"]
        raise RuntimeError(f"{method} failed on every try: {last}")

    def get_block(self, slot):
        """Full block (base64 transactions) or None if the slot was skipped / is unavailable."""
        try:
            return self.call("getBlock", [slot, {"encoding": "base64", "transactionDetails": "full", "rewards": True,
                                                 "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
        except RpcError as e:
            if e.code in (-32007, -32009, -32004):
                return None
            raise

    def signature_statuses(self, sigs):
        out = []
        for i in range(0, len(sigs), 200):
            r = self.call("getSignatureStatuses", [sigs[i:i + 200], {"searchTransactionHistory": True}])
            out.extend(r["value"])
        return out

    def slot_leaders(self, start, limit):
        return self.call("getSlotLeaders", [start, limit])

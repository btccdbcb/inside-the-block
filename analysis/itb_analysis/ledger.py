"""Ledger = chain, checked from the chain alone (REPORT §11, `itb-analysis ledger-check`).

Inputs: the live programme's ledger export (`data/live/ledger.csv`: one row per transaction it signed, with its fee and
status) and its log (`data/live/live.log`: every account it registered, with the balance it read at registration).
From a public RPC: for every registered account, every transaction after its registration slot
(`getSignaturesForAddress`, then `getTransaction`), and its current balance.

Three totals must agree to the lamport:

  ledger    sum of `fee_lamports` over ledger rows with status `landed`
  chain     sum of `meta.fee` over every distinct transaction whose fee payer is a registered account
  balances  balances at registration - current balances - lamports that left the registered accounts for other
            addresses (the sweep) + lamports that arrived from other addresses

and four listings must be empty: landed ledger rows missing on chain or with a different fee or fee payer, on-chain
transactions paid by a registered account that are not in the ledger, accounts whose balance at registration plus
their on-chain changes is not their current balance (a transaction the listing missed), and transactions the RPC
listed but did not return.
"""

from __future__ import annotations

import base64
import csv
import json
from pathlib import Path

from .txdecode import decode


def read_accounts(live_log: Path) -> dict[str, dict]:
    out = {}
    for line in Path(live_log).read_text().splitlines():
        if not line.startswith("{"):
            continue
        e = json.loads(line)
        if e.get("event") == "account_registered":
            out[e["pubkey"]] = {"role": e["role"], "baseline": int(e["baseline"]), "slot": int(e["slot"])}
    return out


def _signatures(rpc, address: str, after_slot: int) -> list[str]:
    sigs, before = [], None
    while True:
        opts = {"limit": 1000, "commitment": "finalized", **({"before": before} if before else {})}
        page = rpc.call("getSignaturesForAddress", [address, opts]) or []
        for s in page:
            if s["slot"] > after_slot:
                sigs.append(s["signature"])
        if len(page) < 1000 or page[-1]["slot"] <= after_slot:
            return sigs
        before = page[-1]["signature"]


def check(ledger_csv: Path, live_log: Path, rpc, log=print) -> dict:
    accounts = read_accounts(live_log)
    rows = list(csv.DictReader(Path(ledger_csv).open()))
    landed = {r["signature"]: r for r in rows if r["status"] == "landed"}

    sigs = set()
    for addr, a in accounts.items():
        found = _signatures(rpc, addr, a["slot"])
        log(f"ledger-check: {a['role']} {addr}: {len(found)} transactions")
        sigs.update(found)

    chain_fees, ext_in, ext_out = 0, 0, 0
    delta = {addr: 0 for addr in accounts}
    paid_by_us, mismatches, unavailable = {}, [], []
    for sig in sorted(sigs):
        tx = rpc.call("getTransaction", [sig, {"encoding": "base64", "maxSupportedTransactionVersion": 1,
                                               "commitment": "finalized"}])
        if not tx or not tx.get("meta"):
            unavailable.append(sig)
            continue
        meta = tx["meta"]
        keys = decode(base64.b64decode(tx["transaction"][0])).accounts
        keys += (meta.get("loadedAddresses") or {}).get("writable", []) + \
            (meta.get("loadedAddresses") or {}).get("readonly", [])
        moved = 0
        for i, k in enumerate(keys):
            if k in delta:
                d = meta["postBalances"][i] - meta["preBalances"][i]
                delta[k] += d
                moved += d
        fee = meta["fee"] if keys[0] in accounts else 0
        if fee:
            chain_fees += fee
            paid_by_us[sig] = {"fee": fee, "payer": keys[0]}
        external = moved + fee  # what crossed the boundary of the registered accounts, fees aside
        ext_in += max(external, 0)
        ext_out += max(-external, 0)

    for sig, r in landed.items():
        c = paid_by_us.get(sig)
        if c is None:
            mismatches.append(f"ledger row {r['id']} ({r['kind']}) {sig}: not found on chain")
        elif c["fee"] != int(r["fee_lamports"] or 0) or c["payer"] != r["fee_payer"]:
            mismatches.append(f"ledger row {r['id']} {sig}: ledger fee {r['fee_lamports']} payer {r['fee_payer']}, "
                              f"chain fee {c['fee']} payer {c['payer']}")
    unrecorded = sorted(set(paid_by_us) - set(landed))

    current = {addr: int(rpc.call("getBalance", [addr, {"commitment": "finalized"}])["value"]) for addr in accounts}
    gaps = [f"{accounts[a]['role']} {a}: registered {accounts[a]['baseline']} + changes {delta[a]} != now {current[a]}"
            for a in accounts if accounts[a]["baseline"] + delta[a] != current[a]]
    start, end = sum(a["baseline"] for a in accounts.values()), sum(current.values())
    totals = {"ledger": sum(int(r["fee_lamports"] or 0) for r in landed.values()), "chain": chain_fees,
              "balances": start - end - ext_out + ext_in}
    res = {"totals": totals, "start_balance": start, "end_balance": end, "left_to_others": ext_out,
           "arrived_from_others": ext_in, "accounts": len(accounts), "transactions": len(sigs),
           "ledger_rows_landed": len(landed), "mismatches": mismatches, "unrecorded": unrecorded,
           "balance_gaps": gaps, "unavailable": unavailable}
    res["pass"] = len(set(totals.values())) == 1 and not (mismatches or unrecorded or gaps or unavailable)
    log(f"ledger {totals['ledger']:,} · chain {totals['chain']:,} · balances {totals['balances']:,} lamports "
        f"({start:,} at registration − {end:,} now − {ext_out:,} out + {ext_in:,} in)")
    for line in (mismatches + [f"not in the ledger: {s}" for s in unrecorded] + gaps
                 + [f"transaction not returned by the RPC: {s}" for s in unavailable]):
        log(line)
    log("LEDGER = CHAIN" if res["pass"] else "LEDGER != CHAIN")
    return res

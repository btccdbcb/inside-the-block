#!/usr/bin/env python3
"""Resolve the target list against a fresh gossip/stake snapshot and pick one alternate per target.

Rule for alternates (fixed before any live data): same gossip clientId as the primary, staked, not already a
primary or alternate; prefer at least 300 leader turns per day, then a different TPU host from the primary;
among those, the highest stake. T5 and T11 have alternates named in advance. Writes design/targets.json. Stdlib only.

Usage: scripts/select_targets.py [--rpc URL] [--out design/targets.json]
"""
import argparse
import json
import urllib.request

PRIMARIES = [
    # (slot, identity or unique prefix, expected clientId, extended?)
    ("T1", "7QQGNm3ptwinipDCyaCF7jY5katgmFUu1ieP2f7nwLpE", "Agave", False),
    ("T2", "HEL1USMZKAL2odpNBj2oCjffnFGaYwmbGmyewGv1e2TU", "JitoLabs", False),
    ("T3", "C8Bey3LKVJHVqN6xPTeW8WJfUgFQAeGNBpT4Rp99JP1k", "JitoLabs", False),
    ("T4", "5Cchr1XGEg7dbBXByV5NY2ad8jfxAM7HA3x8D56rq9Ux", "AgaveBam", False),
    ("T5", "Fc6NNdS2j3Em", "AgaveBam", False),
    ("T6", "Fd7btgySsrjuo25CJCj7oE7VPMyezDhnx7pZkj2v69Nk", "HarmonicAgave", False),
    ("T7", "CAo1dCGYrB6N", "HarmonicAgave", False),
    ("T8", "JUPiTERrZqgf1jUyR7dSkhMx4Kn2qJyekWsg3LT1h4b", "HarmonicFiredancer", False),
    ("T9", "87CcUxpyR72Z", "HarmonicFiredancer", False),
    ("T10", "9eGrDohdNTAo61DRHyfMuqKWXqYnA3i254Wiszxe8FoY", "Firedancer", False),
    ("T11", "DNVZMSqeRH18", "Firedancer", False),
    ("T12", "9jxgosAfHgHzwnxsHw4RAZYaLVokMbnYtmiZBreynGFP", "FireBAM", False),
    ("T13", "ana2y2YvQ3ZPMwm6qhnN3nJoUSiT3qx5Pvetkq9xcfY", "Rakurai", False),
    ("X1", "9xcXe4WRAfbVoVgQmFD5xgFHioL9dD7c74pQZ7PeczT", "Raiku", True),
]
FIXED_ALTERNATES = {"T5": "DRpbCBMx", "T11": "JD549HsbJHeEKKUrKgg4Fj2iyv2RGjsV7NTZjZUrHybB"}
MIN_TURNS_PER_DAY = 300


def rpc(url, method, params=None):
    req = urllib.request.Request(url, data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method,
                                                       "params": params or []}).encode(),
                                 headers={"content-type": "application/json", "user-agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)["result"]


def host(addr):
    return addr.rsplit(":", 1)[0] if addr else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rpc", default="https://api.mainnet-beta.solana.com")
    ap.add_argument("--out", default="design/targets.json")
    a = ap.parse_args()
    nodes = {n["pubkey"]: n for n in rpc(a.rpc, "getClusterNodes")}
    va = rpc(a.rpc, "getVoteAccounts")
    stake = {}
    for v in va["current"]:
        stake[v["nodePubkey"]] = stake.get(v["nodePubkey"], 0) + v["activatedStake"]
    total = sum(stake.values())
    epoch = rpc(a.rpc, "getEpochInfo")

    def resolve(prefix):
        hits = [k for k in nodes if k.startswith(prefix)]
        if len(hits) != 1:
            raise SystemExit(f"prefix {prefix}: {len(hits)} matches")
        return hits[0]

    def turns_per_day(k):
        return stake.get(k, 0) / total * epoch["slotsInEpoch"] / 4 * (86400 / (epoch["slotsInEpoch"] * 0.2))

    used = set()
    out = []
    for slot, pref, client, ext in PRIMARIES:
        k = resolve(pref)
        used.add(k)
        n = nodes[k]
        if n.get("clientId") != client:
            print(f"WARNING {slot} {k}: clientId now {n.get('clientId')} (expected {client})")
        out.append({"slot": slot, "identity": k, "expected_client": client, "extended": ext})
    fixed = {slot: resolve(pref) for slot, pref in FIXED_ALTERNATES.items()}
    used.update(fixed.values())
    for t in out:
        if t["extended"]:
            t["alternate"] = None
            continue
        if t["slot"] in fixed:
            alt = fixed[t["slot"]]
        else:
            prim_host = host(nodes[t["identity"]].get("tpuQuic"))
            cands = [k for k, n in nodes.items() if n.get("clientId") == t["expected_client"] and k not in used
                     and stake.get(k, 0) > 0 and n.get("tpuQuic")]
            # prefer >= 300 turns/day, then a different TPU host, then stake
            cands.sort(key=lambda k: (turns_per_day(k) < MIN_TURNS_PER_DAY,
                                      host(nodes[k].get("tpuQuic")) == prim_host, -stake.get(k, 0)))
            alt = cands[0] if cands else None
        if alt:
            used.add(alt)
        t["alternate"] = alt
    for t in out:
        for k in [t["identity"], t["alternate"]]:
            if not k:
                continue
            n = nodes.get(k, {})
            print(f'{t["slot"]:4} {k:44} {n.get("clientId", "?"):19} {n.get("version", "?"):10} {n.get("tpuQuic")} '
                  f'{stake.get(k, 0) / total * 100:.2f}% {turns_per_day(k):.0f} turns/day')
    doc = {"comment": "Targets frozen with the design. 'alternate' replaces the primary if the pilot rule says so "
                      "(docs/PREREGISTRATION.md). Client labels are re-read from gossip at every turn.",
           "selected_at_epoch": epoch["epoch"], "targets": out}
    with open(a.out, "w") as f:
        json.dump(doc, f, indent=1, sort_keys=True)
        f.write("\n")


if __name__ == "__main__":
    main()

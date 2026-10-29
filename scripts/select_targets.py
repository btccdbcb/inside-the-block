#!/usr/bin/env python3
"""Pick the targets and one alternate per target from a fresh gossip/stake snapshot, by a fixed rule.

Rule (fixed before any live data):
  * Client families and target slots: T1 Agave; T2-T3 JitoLabs; T4-T5 AgaveBam; T6-T7 HarmonicAgave;
    T8-T9 HarmonicFiredancer; T10-T11 Firedancer; T12 FireBAM; T13 Rakurai; X1 Raiku (extended).
  * Eligible: gossip clientId of the family, activated stake > 0, a tpuQuic address, a completed QUIC handshake
    (step 2 below), and not listed in the --exclude file (identities or `prefix:` lines).
  * Reachable at its turns: a validator that a mainnet dry run (--dry-run-quads) tried at least 3 times and never
    fired a quad at (QUIC refused or closed at its turn) is not eligible. Validators the dry run never tried stay
    eligible; the pilot rule swaps any target that still turns out unreachable.
  * Within a family, eligible validators are ranked by stake. Primaries are taken in rank order, preferring a TPU
    host not already used by a target of the same family; alternates are the next ones by the same rule.

Three steps (the handshake check runs in the Rust binary):
  1. scripts/select_targets.py --exclude FILE --candidates-out cand.txt
  2. itb quic-ping --identities-file cand.txt --json > quic.jsonl
  3. scripts/select_targets.py --exclude FILE --quic quic.jsonl --out design/targets.json

targets.json records each target's stake share and expected leader turns per day (4 slots per turn, slot time from
recent performance samples); the design generator sizes each target's rows from it. Stdlib only.
"""
import argparse
import json
import urllib.request

FAMILIES = [
    # (client, target slots, extended)
    ("Agave", ["T1"], False),
    ("JitoLabs", ["T2", "T3"], False),
    ("AgaveBam", ["T4", "T5"], False),
    ("HarmonicAgave", ["T6", "T7"], False),
    ("HarmonicFiredancer", ["T8", "T9"], False),
    ("Firedancer", ["T10", "T11"], False),
    ("FireBAM", ["T12"], False),
    ("Rakurai", ["T13"], False),
    ("Raiku", ["X1"], True),
]
CANDIDATES_PER_FAMILY = 8


def rpc(url, method, params=None):
    req = urllib.request.Request(url, data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method,
                                                       "params": params or []}).encode(),
                                 headers={"content-type": "application/json", "user-agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)["result"]


def host(addr):
    return addr.rsplit(":", 1)[0] if addr else None


def read_exclusions(path):
    ids, prefixes = set(), []
    if not path:
        return ids, prefixes
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if line.startswith("prefix:"):
                prefixes.append(line[len("prefix:"):])
            else:
                ids.add(line)
    return ids, prefixes


def read_quic(path):
    """identity -> handshakes completed (from `itb quic-ping --json`)."""
    ok = {}
    with open(path) as f:
        for line in f:
            if line.strip():
                r = json.loads(line)
                ok[r["identity"]] = max(ok.get(r["identity"], 0), r["ok"])
    return ok


def read_unreachable(paths, min_attempts=3):
    """Identities a dry run tried at least min_attempts times without firing once."""
    tried, fired = {}, set()
    for path in paths or []:
        with open(path) as f:
            for line in f:
                if not line.strip():
                    continue
                q = json.loads(line)
                tried[q["identity"]] = tried.get(q["identity"], 0) + 1
                if q.get("status") == "fired":
                    fired.add(q["identity"])
    return {k for k, n in tried.items() if n >= min_attempts and k not in fired}


def pick(ranked, nodes, n, used_hosts):
    """n identities from `ranked`, preferring hosts not in used_hosts (updated in place)."""
    out = []
    pool = list(ranked)
    while len(out) < n and pool:
        fresh = [k for k in pool if host(nodes[k].get("tpuQuic")) not in used_hosts]
        k = (fresh or pool)[0]
        pool.remove(k)
        used_hosts.add(host(nodes[k].get("tpuQuic")))
        out.append(k)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rpc", default="https://api.mainnet-beta.solana.com")
    ap.add_argument("--exclude", help="identities never targeted (one per line, `prefix:` allowed)")
    ap.add_argument("--candidates-out", help="step 1: write the ranked eligible candidates (before the handshake)")
    ap.add_argument("--quic", help="step 3: JSON lines from `itb quic-ping --json`")
    ap.add_argument("--dry-run-quads", nargs="*", help="quads.jsonl files of mainnet dry runs (reachability)")
    ap.add_argument("--out", default="design/targets.json")
    a = ap.parse_args()

    nodes = {n["pubkey"]: n for n in rpc(a.rpc, "getClusterNodes")}
    va = rpc(a.rpc, "getVoteAccounts")
    stake = {}
    for v in va["current"]:
        stake[v["nodePubkey"]] = stake.get(v["nodePubkey"], 0) + v["activatedStake"]
    total = sum(stake.values())
    epoch = rpc(a.rpc, "getEpochInfo")
    perf = rpc(a.rpc, "getRecentPerformanceSamples", [30])
    slot_s = sum(p["samplePeriodSecs"] for p in perf) / sum(p["numSlots"] for p in perf)
    ex_ids, ex_prefixes = read_exclusions(a.exclude)
    unreachable = read_unreachable(a.dry_run_quads)
    for k in sorted(unreachable):
        print(f"not reachable at its turns in the dry run: {k}")

    def excluded(k):
        return k in ex_ids or any(k.startswith(p) for p in ex_prefixes)

    def turns_per_day(k):
        return stake.get(k, 0) / total * 86400 / slot_s / 4

    quic = read_quic(a.quic) if a.quic else None
    ranked = {}
    for client, _, _ in FAMILIES:
        ks = [k for k, n in nodes.items() if n.get("clientId") == client and stake.get(k, 0) > 0
              and n.get("tpuQuic") and not excluded(k) and k not in unreachable]
        ks.sort(key=lambda k: (-stake[k], k))
        ranked[client] = ks[:CANDIDATES_PER_FAMILY] if quic is None else [k for k in ks if quic.get(k, 0) > 0]

    if a.candidates_out:
        with open(a.candidates_out, "w") as f:
            for client, _, _ in FAMILIES:
                for k in ranked[client]:
                    f.write(k + "\n")
        print(f"wrote {sum(len(v) for v in ranked.values())} candidates to {a.candidates_out}")
        return
    if quic is None:
        raise SystemExit("step 3 needs --quic (handshake results)")

    out = []
    for client, slots, ext in FAMILIES:
        hosts = set()
        primaries = pick(ranked[client], nodes, len(slots), hosts)
        rest = [k for k in ranked[client] if k not in primaries]
        alternates = [] if ext else pick(rest, nodes, len(slots), hosts)
        if len(primaries) < len(slots):
            raise SystemExit(f"{client}: only {len(primaries)} eligible validators for {len(slots)} targets")
        for i, slot in enumerate(slots):
            k = primaries[i]
            alt = alternates[i] if i < len(alternates) else None
            out.append({"slot": slot, "identity": k, "expected_client": client, "extended": ext, "alternate": alt,
                        "stake_share": round(stake[k] / total, 6), "turns_per_day": round(turns_per_day(k), 1)})
    for t in out:
        for k in [t["identity"], t["alternate"]]:
            if not k:
                continue
            n = nodes[k]
            print(f'{t["slot"]:4} {k:44} {n.get("clientId"):19} {n.get("version", "?"):12} {n.get("tpuQuic")} '
                  f'{stake.get(k, 0) / total * 100:.2f}% {turns_per_day(k):.0f} turns/day')
    doc = {"comment": "Targets frozen with the design (rule in scripts/select_targets.py). 'alternate' replaces the "
                      "primary if the pilot rule says so (docs/PREREGISTRATION.md). Client labels are re-read from "
                      "gossip at every turn.",
           "selected_at_epoch": epoch["epoch"], "slot_seconds": round(slot_s, 4), "targets": out}
    with open(a.out, "w") as f:
        json.dump(doc, f, indent=1, sort_keys=True)
        f.write("\n")


if __name__ == "__main__":
    main()

"""Command line entry point: itb-analysis <command>."""

import argparse
import json
import shutil
import sys
from pathlib import Path

CONFIRMATORY_CODE = ("outcomes.py", "stats.py", "analysis.py")


def cmd_design(a):
    from .design import Allocation, balance_table, write_design

    out = Path(a.dir)
    out.mkdir(parents=True, exist_ok=True)
    if a.targets and Path(a.targets).resolve() != (out / "targets.json").resolve():
        shutil.copyfile(a.targets, out / "targets.json")
    alloc = Allocation()
    if a.allocation:
        cfg = json.loads(Path(a.allocation).read_text())
        for k, v in cfg.items():
            setattr(alloc, k, tuple(v) if k == "smoke_targets" else v)
    extra = {}
    if a.prereg:
        import hashlib
        import subprocess

        extra["prereg_sha256"] = hashlib.sha256(Path(a.prereg).read_bytes()).hexdigest()
        tree = subprocess.run(["git", "rev-parse", "HEAD:analysis"], capture_output=True, text=True, check=True)
        dirty = subprocess.run(["git", "status", "--porcelain", "--", "analysis", a.prereg], capture_output=True,
                               text=True, check=True).stdout.strip()
        if dirty:
            raise SystemExit(f"freeze needs analysis/ and {a.prereg} committed:\n{dirty}")
        extra["analysis_git_tree"] = tree.stdout.strip()
        code = Path(__file__).parent
        extra["confirmatory_code_sha256"] = {f"analysis/itb_analysis/{n}": hashlib.sha256((code / n).read_bytes())
                                             .hexdigest() for n in CONFIRMATORY_CODE}
    camp = write_design(out, alloc, seed=a.seed, extra=extra)
    rows = [json.loads(line) for line in (out / "design.jsonl").read_text().splitlines()]
    print(json.dumps({k: camp[k] for k in ("rows", "seed", "design_sha256", "targets_sha256")}, indent=2))
    if a.show_balance:
        for name, tab in balance_table(rows).items():
            print(f"\n{name}\n{tab.to_string()}")
    return 0


def cmd_collect(a):
    from .collect import collect_run
    from .rpc import Rpc

    rpc = Rpc(urls=a.rpc.split(",") if a.rpc else None)
    for d in a.runs:
        collect_run(Path(d), a.slots or [], Path(a.out), rpc)
    return 0


def cmd_power(a):
    from .power import run

    run(Path(a.design), Path(a.out), a.scenarios.split(","), a.n_sim, a.n_perm, a.seed)
    return 0


def cmd_verify(a):
    from .rpc import Rpc
    from .verify import verify, verify_pairs

    rpc = Rpc(urls=a.rpc.split(",")) if a.rpc else None
    res = verify(Path(a.raw), sample=a.sample, seed=a.seed, rpc=rpc)
    if a.pairs:
        res["pairs"] = verify_pairs(Path(a.pairs), Path(a.raw))
        res["pass"] = res["pass"] and res["pairs"]["pass"]
    if a.out:
        Path(a.out).parent.mkdir(parents=True, exist_ok=True)
        Path(a.out).write_text(json.dumps(res, indent=1, sort_keys=True) + "\n")
    for f in res["failures"][:20]:
        print(json.dumps(f))
    print("VERIFY PASS" if res["pass"] else "VERIFY FAIL")
    return 0 if res["pass"] else 1


def cmd_ledger_check(a):
    from .ledger import check
    from .rpc import Rpc

    res = check(Path(a.ledger), Path(a.live_log), Rpc(urls=a.rpc.split(",") if a.rpc else None))
    if a.out:
        Path(a.out).parent.mkdir(parents=True, exist_ok=True)
        Path(a.out).write_text(json.dumps(res, indent=1, sort_keys=True) + "\n")
    return 0 if res["pass"] else 1


def cmd_reproduce(a):
    from .reproduce import reproduce

    return reproduce(Path(a.repo), write=a.write, n_perm=a.n_perm)


def cmd_power_report(a):
    from .power_report import render

    render(Path(a.power), Path(a.out))
    return 0


def cmd_analyse(a):
    from .analysis import run
    from .outcomes import code_all

    pairs, probes = code_all(Path(a.raw), Path(a.targets))
    res = run(pairs, probes, n_perm=a.n_perm, seed=a.seed)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    pairs.to_csv(out / "pairs.csv", index=False)
    probes.to_csv(out / "probes.csv", index=False)
    (out / "results.json").write_text(json.dumps(res, indent=1, sort_keys=True, default=str) + "\n")
    from . import explore

    ex = explore.run(pairs, probes, explore.load_blocks(Path(a.raw)))
    (out / "explore.json").write_text(json.dumps(ex, indent=1, sort_keys=True, default=str) + "\n")
    print(f"{len(pairs)} pairs ({int(pairs.eligible.sum()) if len(pairs) else 0} eligible), {len(probes)} probes")
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(prog="itb-analysis")
    sub = p.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("design", help="generate design/design.jsonl and design/campaign.json")
    d.add_argument("--dir", default="design")
    d.add_argument("--targets", default=None, help="copy this targets.json into --dir first")
    d.add_argument("--allocation", default=None, help="JSON with Allocation fields (from the power study)")
    d.add_argument("--seed", type=int, default=None, help="default: fresh OS entropy (recorded in campaign.json)")
    d.add_argument("--show-balance", action="store_true")
    d.add_argument("--prereg", default=None, help="freeze: record this file's SHA-256 and the analysis/ git tree")
    d.set_defaults(func=cmd_design)
    c = sub.add_parser("collect", help="fetch blocks for fired quads, write excerpts and outcomes")
    c.add_argument("runs", nargs="+", help="run directories (with quads.jsonl)")
    c.add_argument("--slots", nargs="*", help="slot clock files (jsonl) covering the runs")
    c.add_argument("--out", default="data/raw")
    c.add_argument("--rpc", default=None, help="comma-separated RPC URLs (default: ITB_RPC_URLS or public pool)")
    c.set_defaults(func=cmd_collect)
    w = sub.add_parser("power", help="operating characteristics of the analysis on simulated campaigns")
    w.add_argument("--design", default="design")
    w.add_argument("--out", default="results/power.json")
    w.add_argument("--scenarios", default="null,expected,load,weak")
    w.add_argument("--n-sim", type=int, default=200)
    w.add_argument("--n-perm", type=int, default=1999)
    w.add_argument("--seed", type=int, default=20261105)
    w.set_defaults(func=cmd_power)
    v = sub.add_parser("verify", help="re-check landing claims from archived blocks (and a public RPC with --rpc)")
    v.add_argument("raw", help="collected runs (output of collect)")
    v.add_argument("--sample", type=int, default=None, help="check a seeded sample of N landed probes")
    v.add_argument("--seed", type=int, default=0)
    v.add_argument("--rpc", default=None, help="comma-separated RPC URLs: re-fetch every checked block")
    v.add_argument("--pairs", default=None, help="pairs.csv from analyse: recompute every pair's order")
    v.add_argument("--out", default=None)
    v.set_defaults(func=cmd_verify)
    lc = sub.add_parser("ledger-check", help="ledger = chain to the lamport, from the chain alone")
    lc.add_argument("--ledger", default="data/live/ledger.csv")
    lc.add_argument("--live-log", default="data/live/live.log")
    lc.add_argument("--rpc", default=None, help="comma-separated RPC URLs (default: public mainnet)")
    lc.add_argument("--out", default=None, help="write the result as JSON")
    lc.set_defaults(func=cmd_ledger_check)
    x = sub.add_parser("reproduce", help="recompute every committed result and figure, report differences")
    x.add_argument("--repo", default="..", help="repository root")
    x.add_argument("--write", action="store_true", help="write the outputs into the repository instead of comparing")
    x.add_argument("--n-perm", type=int, default=10_000)
    x.set_defaults(func=cmd_reproduce)
    r = sub.add_parser("power-report", help="render docs/POWER.md from results/power.json")
    r.add_argument("--power", default="results/power.json")
    r.add_argument("--out", default="docs/POWER.md")
    r.set_defaults(func=cmd_power_report)
    y = sub.add_parser("analyse", help="code pairs; run the pre-registered and the exploratory analysis")
    y.add_argument("raw", help="collected runs (output of collect)")
    y.add_argument("--targets", default="design/targets.json")
    y.add_argument("--out", default="results")
    y.add_argument("--n-perm", type=int, default=10_000)
    y.add_argument("--seed", type=int, default=0)
    y.set_defaults(func=cmd_analyse)
    a = p.parse_args(sys.argv[1:] if argv is None else argv)
    return a.func(a)


if __name__ == "__main__":
    sys.exit(main())

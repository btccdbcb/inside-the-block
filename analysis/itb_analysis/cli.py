"""Command line entry point: itb-analysis <command>."""

import argparse
import json
import shutil
import sys
from pathlib import Path


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
    camp = write_design(out, alloc, seed=a.seed)
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


def main(argv=None):
    p = argparse.ArgumentParser(prog="itb-analysis")
    sub = p.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("design", help="generate design/design.jsonl and design/campaign.json")
    d.add_argument("--dir", default="design")
    d.add_argument("--targets", default=None, help="copy this targets.json into --dir first")
    d.add_argument("--allocation", default=None, help="JSON with Allocation fields (from the power study)")
    d.add_argument("--seed", type=int, default=None, help="default: fresh OS entropy (recorded in campaign.json)")
    d.add_argument("--show-balance", action="store_true")
    d.set_defaults(func=cmd_design)
    c = sub.add_parser("collect", help="fetch blocks for fired quads, write excerpts and outcomes")
    c.add_argument("runs", nargs="+", help="run directories (with quads.jsonl)")
    c.add_argument("--slots", nargs="*", help="slot clock files (jsonl) covering the runs")
    c.add_argument("--out", default="data/raw")
    c.add_argument("--rpc", default=None, help="comma-separated RPC URLs (default: ITB_RPC_URLS or public pool)")
    c.set_defaults(func=cmd_collect)
    a = p.parse_args(sys.argv[1:] if argv is None else argv)
    return a.func(a)


if __name__ == "__main__":
    sys.exit(main())

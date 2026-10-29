"""The tables of REPORT.md (and README.md), rendered from the committed data and results.

A document holds hand-written text around two kinds of generated parts:

    <!-- table:NAME -->            a table, rendered by t_NAME below; its first line names the source file
    ...
    <!-- /table:NAME -->

    <!-- v:PATH|FORMAT -->0.77<!-- /v -->     one number in the prose: PATH is a dotted path into the loaded results
                                               (results.P1.JitoLabs, explore.mechanism..., n.fees_total) and FORMAT
                                               one of prop, p, int, or a Python format spec such as .2f

`render_doc` replaces every generated part. `reproduce` re-renders the committed REPORT.md and README.md and compares
them byte for byte, so a table or a number in the text that no longer matches the data shows up as a difference.
"""

from __future__ import annotations

import csv
import gzip
import json
import re
from collections import defaultdict
from pathlib import Path

from .analysis import EXTENDED, FAMILIES

PANELS = FAMILIES + EXTENDED
BLOCK = re.compile(r"(<!-- table:([a-z_0-9]+) -->\n)(.*?)(<!-- /table:\2 -->)", re.S)
VALUE = re.compile(r"(<!-- v:([^|\s>]+)(?:\|([^\s>]+))? -->)(.*?)(<!-- /v -->)", re.S)


# --- formatting -------------------------------------------------------------------------------------------------------

def _num(x, nd=2) -> str:
    return "—" if x is None else f"{x:.{nd}f}"


def _p(x) -> str:
    if x is None:
        return "—"
    return "< 0.001" if x < 0.001 else f"{x:.3f}"


def _prop(d: dict | None) -> str:
    if not d or not d.get("n"):
        return "—"
    lo, hi = d["ci95"]
    return f"{d['p']:.2f} [{lo:.2f}, {hi:.2f}] ({d['k']}/{d['n']})"


def _table(head: list[str], rows: list[list[str]], source: str) -> str:
    out = [f"*Source: {source}.*", "", "| " + " | ".join(head) + " |", "|" + "---|" * len(head)]
    out += ["| " + " | ".join(str(c) for c in r) + " |" for r in rows]
    if not rows:
        out.append("| " + " | ".join(["no data"] + [""] * (len(head) - 1)) + " |")
    return "\n".join(out) + "\n"


# --- context ----------------------------------------------------------------------------------------------------------

def _read_json(p: Path):
    return json.loads(p.read_text()) if p.exists() else None


def _jsonl(p: Path) -> list[dict]:
    if not p.exists():
        return []
    opener = gzip.open if p.suffix == ".gz" else open
    with opener(p, "rt") as f:
        return [json.loads(line) for line in f if line.strip()]


def load(repo: Path, results: Path) -> dict:
    repo, results = Path(repo), Path(results)
    ctx = {name: _read_json(results / f"{name}.json") for name in ("results", "explore", "timing", "verify")}
    ctx["power"] = _read_json(repo / "results" / "power.json")
    ctx["raw"] = sorted(d for d in (repo / "data" / "raw").glob("*") if d.is_dir())
    ctx["dry"] = sorted(d for d in (repo / "data" / "dry_runs").glob("*") if d.is_dir())
    ledger = repo / "data" / "live" / "ledger.csv"
    ctx["ledger"] = list(csv.DictReader(ledger.open())) if ledger.exists() else []
    ctx["campaign"] = _read_json(repo / "design" / "campaign.json")
    ctx["memo"] = _read_json(repo / "data" / "live" / "memo_check.json")
    ctx["ledger_check"] = _read_json(repo / "data" / "live" / "ledger_check.json")
    ctx["n"] = summary(ctx)
    return ctx


def summary(ctx) -> dict:
    """Totals quoted in the prose: campaign runs (data/raw) and the ledger."""
    quads = [q for d in ctx["raw"] for q in _jsonl(d / "quads.jsonl")]
    fired = [q for q in quads if q.get("status") == "fired"]
    metas = [m for m in (_read_json(d / "run.json") for d in ctx["raw"]) if m]
    times = sorted(q["fired_utc"] for q in fired if q.get("fired_utc"))
    fees = [int(r["fee_lamports"] or 0) for r in ctx["ledger"] if r["status"] == "landed"]
    return {"runs": len(metas), "quads_fired": len(fired), "quads_attempted": len(quads),
            "targets": len({q["target_slot"] for q in fired}), "identities": len({q["identity"] for q in fired}),
            "probes": sum(m.get("probes", 0) for m in metas), "landed": sum(m.get("landed", 0) for m in metas),
            "first_slot": min((m["first_slot"] for m in metas if m.get("first_slot")), default=None),
            "last_slot": max((m["last_slot"] for m in metas if m.get("last_slot")), default=None),
            "first_utc": times[0] if times else None, "last_utc": times[-1] if times else None,
            "fees_total": sum(fees), "transactions": len(ctx["ledger"])}


# --- tables -----------------------------------------------------------------------------------------------------------

def t_runs(ctx) -> str:
    rows = []
    for d, kind in [(d, "dry run") for d in ctx["dry"]] + [(d, "live") for d in ctx["raw"]]:
        quads = _jsonl(d / "quads.jsonl")
        if not quads:
            continue
        fired = [q for q in quads if q.get("status") == "fired"]
        times = sorted(q[k] for q in quads for k in ("decided_utc", "fired_utc") if q.get(k))
        slots = sorted(q["slot0"] for q in fired if q.get("slot0"))
        meta = _read_json(d / "run.json") or {}
        outs = _jsonl(d / "outcomes.jsonl")
        fees = sum(o.get("meta_fee") or 0 for o in outs if o.get("landed"))
        span = "—"
        if times:
            a, b = times[0], times[-1]
            span = f"{a[:10]} {a[11:16]} → " + (b[11:16] if a[:10] == b[:10] else f"{b[5:10]} {b[11:16]}")
        rows.append([d.name, kind, span,
                     f"{slots[0]}–{slots[-1]}" if slots else "—", len({q['target_slot'] for q in fired}),
                     f"{len(fired)}/{len(quads)}", meta.get("probes", 4 * len(fired)) if kind == "live" else "—",
                     meta.get("landed", "—") if kind == "live" else "—", f"{fees:,}" if kind == "live" else "—",
                     (d / "COMMIT").read_text().strip() if (d / "COMMIT").exists() else "—"])
    return _table(["run", "kind", "UTC, first → last quad", "slot0 range", "targets", "quads fired / attempted",
                   "probes", "landed", "fees (lamports)", "sender commit"], rows,
                  "data/raw/*/ and data/dry_runs/*/ (quads.jsonl, run.json, outcomes.jsonl)")


def t_timing(ctx) -> str:
    rows = []
    for name, g in (ctx["timing"] or {}).items():
        if not g:
            continue
        a, b = g["a_exec_within_50ms"], g["b_pre_turn_before_slot0"]
        pred = g["anchor_error_ms"].get("predicted", {})
        rows.append([name.replace("_", " "), g["fired"], f"{a['share']:.3f} ({a['n']})",
                     "pass" if a["pass"] else "FAIL", f"{b['share']:.3f} ({b['n']})", "pass" if b["pass"] else "FAIL",
                     " / ".join(_num(x, 1) for x in pred.get("abs_p50_p90", [])) or "—"])
    return _table(["runs", "quads fired", "(a) sent within 50 ms of plan", "G2a", "(b) pre-turn sends before slot 0",
                   "G2b", "predicted anchor, absolute error p50 / p90 (ms)"], rows, "results/timing.json (timing.py)")


def t_verdicts(ctx) -> str:
    v = (ctx["results"] or {}).get("verdicts", {})
    tests = ["P1", "P2", "S1", "S2-C1", "S2-C2", "S2-C3", "S3"]
    rows = [[fam] + [v[fam].get(t, "—") for t in tests] for fam in FAMILIES if fam in v]
    return _table(["family"] + tests, rows, "results/results.json → verdicts (PREREGISTRATION §5 wording)")


def t_primary(ctx) -> str:
    r = ctx["results"] or {}
    if not r:
        return _table(["test"], [], "results/results.json")
    hp = r["holm_primary"]
    rows = []
    for fam in FAMILIES:
        p1, p2 = r["P1"][fam], r["P2"][fam]
        rows.append([fam, _prop(p1), _p(p1.get("p_value")), _p(hp.get(f"P1:{fam}")), p2.get("n_quads", 0),
                     _p(p2.get("land_p_value")), _p(hp.get(f"P2:{fam}"))])
    return _table(["family", "P1: F (first-sent first), 95 % CI", "P1 p", "P1 Holm p", "P2 quads", "P2 p",
                   "P2 Holm p"], rows, "results/results.json → P1, P2, P3, holm_primary")


def t_load(ctx) -> str:
    r = ctx["results"] or {}
    if not r:
        return _table(["test"], [], "results/results.json")
    rows = []
    for name, d in [("pooled (P3)", r["P3"]["pooled"])] + list(r["P3"]["by_family"].items()):
        if not d.get("n"):
            continue
        ci = d.get("ci90") or [None, None]
        rows.append([name, d["n"], _num(d.get("beta"), 3), f"[{_num(ci[0], 3)}, {_num(ci[1], 3)}]",
                     _p(d.get("p_value")), _p(r["holm_primary"].get("P3")) if name.startswith("pooled") else "",
                     d.get("verdict", "descriptive") if name.startswith("pooled") else "descriptive"])
    return _table(["R pairs", "n", "β (logit per 10M CU)", "90 % CI", "p", "Holm p", "verdict"], rows,
                  "results/results.json → P3 (equivalence margin ±0.4)")


def t_secondary(ctx) -> str:
    r = ctx["results"] or {}
    if not r:
        return _table(["test"], [], "results/results.json")
    hs = r["holm_secondary"]
    rows = []
    for fam in FAMILIES:
        s1, s2 = r["S1"][fam], r["S2"][fam]
        rows.append([fam, _prop(s1), _p(hs.get(f"S1:{fam}"))]
                    + [f"{_prop(s2[c])}; Holm p {_p(hs.get(f'S2-{c}:{fam}'))}" for c in ("C1", "C2", "C3")]
                    + [_p(hs.get(f"S3:{fam}"))])
    return _table(["family", "S1: P(HI first) pre-turn", "S1 Holm p", "S2 C1", "S2 C2", "S2 C3", "S3 Holm p"], rows,
                  "results/results.json → S1, S2, P2 (leak), holm_secondary")


def t_gates(ctx) -> str:
    r = ctx["results"] or {}
    rows = []
    for name, g in (r.get("G1") or {}).items():
        if isinstance(g, dict):
            rows.append([f"G1 {name}", f"C1 {_prop(g.get('C1'))}", f"C2 {_prop(g.get('C2'))}",
                         "pass" if g.get("pass") else "fail"])
    for t, g in (r.get("G3") or {}).items():
        rows.append([f"G3 {t}", f"preferred only in turn: {g['pref_only']}", f"other only: {g['other_only']}",
                     f"McNemar p {_p(g['p_value'])}"])
    return _table(["gate", "", "", "result"], rows, "results/results.json → G1, G3")


def t_sensitivity(ctx) -> str:
    r = (ctx["results"] or {}).get("sensitivity") or {}
    rows = []
    for name, s in r.items():
        for fam in PANELS:
            t = s["tables"].get(fam, {})
            if any((t.get(k) or {}).get("n") for k in t):
                rows.append([name, fam, _prop(t.get("P1_F")), _prop(t.get("S1_hi_first_pre"))])
    return _table(["analysis", "family", "F", "P(HI first) pre-turn"], rows,
                  "results/results.json → sensitivity (PREREGISTRATION §4 a, b, c)")


def t_per_target(ctx) -> str:
    d = ((ctx["results"] or {}).get("descriptive") or {}).get("per_target", {})
    order = sorted(d, key=lambda t: (t[0], int(t[1:]) if t[1:].isdigit() else 0))
    rows = [[t, d[t]["family"], d[t]["quads"], _prop(d[t]["in_turn"]), d[t]["eligible_pairs"], _prop(d[t]["F"]),
             _prop(d[t]["R_hi_first"])] for t in order]
    return _table(["target", "family", "quads", "probes in the target's turn", "eligible pairs", "F",
                   "R: HI first"], rows, "results/results.json → descriptive.per_target")


def t_validators(ctx) -> str:
    """One row per measured validator: what gossip said at send time, and its headline numbers (README)."""
    seen = defaultdict(lambda: defaultdict(int))
    for d in ctx["raw"]:
        for q in _jsonl(d / "quads.jsonl"):
            if q.get("status") == "fired" and q.get("view"):
                v = q["view"]
                key = (q["identity"], v.get("client_id"), v.get("version"), v.get("ingress_kind"))
                seen[q["target_slot"]][key] += 1
    d = ((ctx["results"] or {}).get("descriptive") or {}).get("per_target", {})
    rows = []
    for t in sorted(seen, key=lambda t: (t[0], int(t[1:]) if t[1:].isdigit() else 0)):
        views = sorted(seen[t].items(), key=lambda kv: -kv[1])
        ident, client, _, ingress = views[0][0]
        versions = ", ".join(sorted({k[2] for k, _ in views if k[2]}))
        v = d.get(t, {})
        rows.append([t, f"[{ident[:4]}…{ident[-4:]}](https://explorer.solana.com/address/{ident})", client, versions,
                     ingress, _prop(v.get("F")), _prop(v.get("R_hi_first")), _prop(v.get("in_turn"))])
    return _table(["slot", "validator", "client (gossip)", "versions seen", "ingress", "F, equal keys",
                   "R: higher key first", "probes in its turn"], rows,
                  "gossip views in data/raw/*/quads.jsonl; results/results.json → descriptive.per_target")


def t_v1(ctx) -> str:
    d = (ctx["results"] or {}).get("descriptive") or {}
    rows = []
    if d.get("v1_in_turn"):
        rows.append(["in the target's turn: v1 probes vs legacy probes of the same quads",
                     _prop(d["v1_in_turn"]["v1"]), _prop(d["v1_in_turn"]["legacy_same_quads"])])
    for fam in PANELS:
        a, b = (d.get("V_EQ_v1_first") or {}).get(fam), (d.get("V_HI_hi1_first") or {}).get(fam)
        if (a and a["n"]) or (b and b["n"]):
            rows.append([fam, _prop(a), _prop(b)])
    return _table(["", "V_EQ: v1 member first (equal keys)", "V_HI: v1 high-key member first"], rows,
                  "results/results.json → descriptive (v1 arm)")


def t_rrr(ctx) -> str:
    r = (ctx["results"] or {}).get("RRR") or {}
    dts = sorted({int(k) for v in r.values() for k in v})
    rows = [[fam] + [_prop(r[fam].get(str(dt))) for dt in dts] for fam in PANELS if r.get(fam)]
    return _table(["family"] + [f"Δt = {dt} ms" for dt in dts], rows, "results/results.json → RRR")


def t_mechanism(ctx) -> str:
    m = (ctx["explore"] or {}).get("mechanism") or {}
    rows = []
    for fam in PANELS:
        f = m.get(fam)
        if not f or not isinstance(f.get("fit"), dict):
            continue
        fits = f["fit"]["models"]
        cells = []
        for k in ("arrival", "drain", "batch", "constant"):
            params = ", ".join(f"{p} = {_num(v, 2)}" for p, v in fits[k]["params"].items())
            cells.append(f"{_num(fits[k]['akaike_weight'])} ({params})")
        boot = f["fit"].get("bootstrap", {}).get("ci95", {})
        ci = "; ".join(f"{p} [{_num(lo, 2)}, {_num(hi, 2)}]" for p, (lo, hi) in boot.items()) or "—"
        rec = f["fit"].get("recovery", {}).get("share_correct", {}).get(f["fit"]["best"])
        rows.append([fam, f["n"], f["fit"]["best"]] + cells + [ci, _num(rec)])
    return _table(["family", "pairs", "lowest AIC", "arrival: weight (s ms)", "drain: weight (a, h ms)",
                   "batch: weight (a, B ms)", "constant: weight (a)", "best model: 95 % bootstrap CI",
                   "recovery of that model at this n"], rows,
                  "results/explore.json → mechanism (exploratory)")


def t_whole_block(ctx) -> str:
    w = (ctx["explore"] or {}).get("whole_block") or {}
    rows = []
    for fam in PANELS:
        f = w.get(fam)
        if not f:
            continue
        q = lambda d: f"{_num(d.get('median'))} [{_num(d.get('p25'))}, {_num(d.get('p75'))}]" if d.get("n") else "—"  # noqa: E731
        rows.append([fam, f["blocks"], q(f["price_tau"]), q(f["priority_tau"]),
                     f"{f['n_tx'].get('median', 0):.0f}", f"{(f['cost_units'].get('median') or 0) / 1e6:.1f}M"])
    return _table(["family", "blocks", "sortedness by CU price, median [IQR]", "by approx. priority, median [IQR]",
                   "transactions per block (median)", "cost units per block (median)"], rows,
                  "results/explore.json → whole_block (exploratory; +1 = sorted high to low)")


def t_insertion(ctx) -> str:
    w = (ctx["explore"] or {}).get("insertion") or {}
    rows = []
    for fam in PANELS:
        for k, d in sorted((w.get(fam) or {}).get("by_slot_of_turn", {}).items()):
            if d.get("n"):
                rows.append([fam, k, d["n"], f"{_num(d['median'])} [{_num(d['p25'])}, {_num(d['p75'])}]"])
    return _table(["family", "slot of turn", "probes", "position in block (0 first, 1 last), median [IQR]"], rows,
                  "results/explore.json → insertion (exploratory)")


def t_hour(ctx) -> str:
    h = (ctx["explore"] or {}).get("hour") or {}
    rows = [[fam, s, _prop(p)] for fam in ["all"] + PANELS for s, p in sorted((h.get(fam) or {}).get("by_session",
                                                                                                    {}).items())]
    return _table(["family", "session", "probes in the target's turn"], rows,
                  "results/explore.json → hour (exploratory)")


def t_operators(ctx) -> str:
    o = (ctx["explore"] or {}).get("operators") or {}
    rows = []
    for fam, f in o.items():
        hom = f.get("in_turn_homogeneity") or {}
        for t, v in f["targets"].items():
            rows.append([fam, t, _prop(v["in_turn"]), _prop(v["F"]), _num(f.get("icc_in_turn"), 3),
                         _num(f.get("icc_F"), 3), _p(hom.get("p_value"))])
    return _table(["family", "target", "in the target's turn", "F", "ICC in-turn", "ICC F",
                   "χ² p (equal in-turn landing)"], rows,
                  "results/explore.json → operators (exploratory)")


def t_between(ctx) -> str:
    b = (ctx["explore"] or {}).get("between") or {}
    rows = [[fam, f["pairs"], _prop(f["adjacent"]), _num(f["gap"].get("median"), 0), _num(f["gap"].get("p90"), 0)]
            for fam, f in b.items()]
    return _table(["family", "pairs in one block", "adjacent", "transactions between (median)", "p90"], rows,
                  "results/explore.json → between (exploratory)")


def t_verify(ctx) -> str:
    v = ctx["verify"] or {}
    rows = [["claims checked offline", v.get("checked", 0)], ["failures", len(v.get("failures", []))],
            ["pair orders recomputed", (v.get("pairs") or {}).get("checked", "—")],
            ["result", "PASS" if v.get("pass") else "FAIL"]]
    return _table(["", ""], rows, "results/verify.json (`itb-analysis verify`, docs/VERIFY.md)")


def t_cost(ctx) -> str:
    led = ctx["ledger"]
    agg = defaultdict(lambda: [0, 0, 0])
    for r in led:
        a = agg[(r["sub_budget"], r["kind"])]
        a[0] += 1
        if r["status"] == "landed":
            a[1] += 1
            a[2] += int(r["fee_lamports"] or 0)
    rows = [[sb, kind, n, landed, f"{fee:,}"] for (sb, kind), (n, landed, fee) in sorted(agg.items())]
    total = sum(a[2] for a in agg.values())
    if rows:
        rows.append(["total", "", sum(a[0] for a in agg.values()), sum(a[1] for a in agg.values()),
                     f"{total:,} ({total / 1e9:.6f} SOL)"])
    return _table(["sub-budget", "kind", "transactions", "landed", "fees paid (lamports)"], rows,
                  "data/live/ledger.csv (the live service's ledger)")


def _tx(sig: str) -> str:
    return f"[{sig[:6]}…{sig[-4:]}](https://explorer.solana.com/tx/{sig})"


def _utc_s(t: int | None) -> str:
    import datetime as dt

    return "—" if t is None else dt.datetime.fromtimestamp(t, dt.timezone.utc).strftime("%Y-%m-%d %H:%M:%S")


def _utc_ms(ms: float | None) -> str:
    return "—" if ms is None else _utc_s(int(ms // 1000))


def t_memo(ctx) -> str:
    m = ctx["memo"]
    if not m:
        return _table(["", ""], [], "data/live/memo_check.json (`itb-analysis memo-check`)")
    exp, first = m.get("expected") or {}, m.get("first_campaign_probe") or {}
    text = dict(f.split("=", 1) for f in m["text"].split()[2:]) if m.get("text") else {}
    rows = [["Memo transaction", f"{_tx(m['signature'])}, slot {m['slot']:,}, {_utc_s(m.get('block_time'))} UTC"]]
    rows += [[f"sha256 of {k}", f"`{v[:16]}…` " + ("= the Memo" if text.get(k) == v else "≠ the Memo")]
             for k, v in exp.items()]
    if first:
        rows.append(["first campaign send", f"{_utc_ms(first['first_send_ms'])} UTC (quad {first['quad_id']})"])
        rows.append(["first campaign turn", f"slot {first['min_slot0']:,}"])
    changed = m.get("code_changed_since_freeze") or []
    rows.append(["confirmatory code changed since the freeze", ", ".join(f"`{c}`" for c in changed) or "none"])
    rows.append(["result", "PASS" if m.get("pass") else "FAIL: " + "; ".join(m.get("problems", []))])
    return _table(["", ""], rows, "data/live/memo_check.json (`itb-analysis memo-check`, public mainnet RPC)")


def t_ledger_check(ctx) -> str:
    c = ctx["ledger_check"]
    if not c:
        return _table(["", "lamports"], [], "data/live/ledger_check.json (`itb-analysis ledger-check`)")
    t = c["totals"]
    rows = [["ledger: landed fees in data/live/ledger.csv", f"{t['ledger']:,}"],
            ["chain: `meta.fee` of every transaction paid by my accounts", f"{t['chain']:,}"],
            ["balances: at registration − now − sent to others + received", f"{t['balances']:,}"],
            ["balances at registration", f"{c['start_balance']:,}"],
            ["balances now", f"{c['end_balance']:,}"],
            ["sent to other addresses (the sweep)", f"{c['left_to_others']:,}"],
            ["received from other addresses", f"{c['arrived_from_others']:,}"],
            ["accounts / transactions found on chain", f"{c['accounts']} / {c['transactions']:,}"],
            ["mismatches, unrecorded, balance gaps, not returned",
             " · ".join(str(len(c[k])) for k in ("mismatches", "unrecorded", "balance_gaps", "unavailable"))],
            ["result", "LEDGER = CHAIN" if c.get("pass") else "LEDGER ≠ CHAIN"]]
    return _table(["", "lamports"], rows,
                  "data/live/ledger_check.json (`itb-analysis ledger-check`, public mainnet RPC)")


def t_money(ctx) -> str:
    led, c = ctx["ledger"], ctx["ledger_check"] or {}
    cap = (ctx["campaign"] or {}).get("cap_lamports")
    if not led:
        return _table(["", ""], [], "data/live/ledger.csv, data/live/ledger_check.json")
    fees = sum(int(r["fee_lamports"] or 0) for r in led if r["status"] == "landed")
    sweeps = [r for r in led if r["kind"] == "sweep" and r["status"] == "landed"]
    rows = [["fees paid", f"{fees:,} lamports ({fees / 1e9:.6f} SOL) of the {cap:,} cap"],
            ["ledger = chain", ("yes, to the lamport" if c.get("pass") else "NO") if c else "not checked yet"],
            ["sweep", f"{len(sweeps)} transaction{'s' * (len(sweeps) != 1)}; the last {_tx(sweeps[-1]['signature'])}"
             if sweeps else "not yet"]]
    return _table(["", ""], rows, "data/live/ledger.csv, data/live/ledger_check.json")


TABLES = {name[2:]: fn for name, fn in globals().items() if name.startswith("t_") and callable(fn)}


def value(ctx: dict, path: str, fmt: str | None) -> str:
    x = ctx
    for part in path.split("."):
        if not isinstance(x, dict) or part not in x:
            raise KeyError(f"no value at {path!r}")
        x = x[part]
    if fmt == "prop":
        return _prop(x)
    if fmt == "p":
        return _p(x)
    if fmt == "int":
        return f"{x:,}"
    return format(x, fmt) if fmt else str(x)


def render_doc(text: str, ctx: dict) -> str:
    def table(m):
        name = m.group(2)
        if name not in TABLES:
            raise KeyError(f"unknown table {name!r}")
        return m.group(1) + TABLES[name](ctx) + m.group(4)

    def val(m):
        return m.group(1) + value(ctx, m.group(2), m.group(3)) + m.group(5)

    return VALUE.sub(val, BLOCK.sub(table, text))

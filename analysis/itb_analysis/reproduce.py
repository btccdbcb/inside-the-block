"""Recompute every committed result from the committed data and compare, byte for byte (`docker compose up`).

Inputs (repository root): data/raw/<run>/ (collected campaign runs), data/dry_runs/<run>/ (the mainnet dry runs,
for gate G2), design/targets.json. Outputs, each compared with its committed copy:

  results/pairs.csv, results/probes.csv, results/results.json   the pre-registered analysis (analyse)
  results/explore.json                                           the exploratory analyses (explore.py)
  results/timing.json                                            gate G2 and skip reasons (timing.py)
  results/verify.json                                            every landing claim re-checked offline (verify.py)
  figures/*.svg                                                  figures.py
  docs/POWER.md                                                  rendered from the committed results/power.json
  REPORT.md, README.md                                           their table blocks re-rendered (report.py)

Prints one line per differing or missing file and "<n> differences". With --write, the outputs replace the committed
copies instead (how they were made in the first place).
"""

from __future__ import annotations

import filecmp
import json
import shutil
import tempfile
from pathlib import Path

N_PERM = 10_000
SEED = 0


def _runs(root: Path) -> list[Path]:
    return sorted(d for d in root.iterdir() if d.is_dir()) if root.exists() else []


def build(repo: Path, out: Path, n_perm: int = N_PERM, log=print) -> list[Path]:
    """Write every output under out/ (same relative paths as in the repository); return their relative paths."""
    from . import figures
    from .analysis import run
    from .outcomes import code_all
    from .timing import g2
    from .verify import verify, verify_pairs

    raw, dry = repo / "data" / "raw", repo / "data" / "dry_runs"
    res_dir, fig_dir = out / "results", out / "figures"
    res_dir.mkdir(parents=True, exist_ok=True)
    fig_dir.mkdir(parents=True, exist_ok=True)
    written = []

    def dump(name: str, obj):
        (res_dir / name).write_text(json.dumps(obj, indent=1, sort_keys=True, default=str) + "\n")
        written.append(Path("results") / name)

    pairs, probes = code_all(raw, repo / "design" / "targets.json") if _runs(raw) else ([], [])
    if len(pairs):
        pairs.to_csv(res_dir / "pairs.csv", index=False, lineterminator="\n")
        probes.to_csv(res_dir / "probes.csv", index=False, lineterminator="\n")
        written += [Path("results/pairs.csv"), Path("results/probes.csv")]
        log(f"analyse: {len(pairs)} pairs ({int(pairs.eligible.sum())} eligible), {len(probes)} probes")
        dump("results.json", run(pairs, probes, n_perm=n_perm, seed=SEED))
        from . import explore

        dump("explore.json", explore.run(pairs, probes, explore.load_blocks(raw)))
    campaign_runs = [d for d in _runs(raw) if (d / "quads.jsonl").exists()]
    dump("timing.json", {"dry_run": g2(_runs(dry)) if _runs(dry) else None,
                         "campaign": g2(campaign_runs) if campaign_runs else None})
    v = verify(raw, log=log) if campaign_runs else {"checked": 0, "pass": True, "failures": []}
    if len(pairs):
        v["pairs"] = verify_pairs(res_dir / "pairs.csv", raw)
        v["pass"] = v["pass"] and v["pairs"]["pass"]
    dump("verify.json", v)
    for name in figures.render_all(res_dir, fig_dir):
        written.append(Path("figures") / name)
    if (repo / "results" / "power.json").exists():
        from .power_report import render

        render(repo / "results" / "power.json", out / "docs" / "POWER.md")
        written.append(Path("docs/POWER.md"))
    from . import report

    ctx = report.load(repo, res_dir)
    for doc in ("REPORT.md", "README.md"):
        text = (repo / doc).read_text() if (repo / doc).exists() else ""
        if "<!-- table:" in text:
            (out / doc).write_text(report.render_doc(text, ctx))
            written.append(Path(doc))
    return written


def reproduce(repo: Path, write: bool = False, n_perm: int = N_PERM, log=print) -> int:
    repo = Path(repo).resolve()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        written = build(repo, out, n_perm=n_perm, log=log)
        if write:
            for rel in written:
                (repo / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(out / rel, repo / rel)
            log(f"wrote {len(written)} files")
            return 0
        diffs = []
        for rel in written:
            if not (repo / rel).exists():
                diffs.append(f"missing in the repository: {rel}")
            elif not filecmp.cmp(out / rel, repo / rel, shallow=False):
                diffs.append(f"differs: {rel}")
        committed = {p.relative_to(repo) for d in ("figures",) if (repo / d).exists() for p in (repo / d).glob("*.svg")}
        diffs += [f"not produced: {rel}" for rel in sorted(committed - set(written))]
        for d in diffs:
            log(d)
        log(f"{len(diffs)} differences")
        return 0 if not diffs else 1

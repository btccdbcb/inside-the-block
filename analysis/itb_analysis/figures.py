"""Figures from results/results.json, each in a light and a dark version (SVG, byte-stable across runs).

  turn-map-{light,dark}.svg   per family: share of probes landing in the target's turn, and with the preceding leader,
                              against the send offset tau (P2, S3), with 95 % intervals
  order-{light,dark}.svg      per family: F on equal keys (P1) and P(HI first) for pre-turn R pairs (S1), 95 % intervals
  race-{light,dark}.svg       per family: race-reversal rate against the gap dt (descriptive), 95 % intervals

Colours: the first two slots of a categorical palette checked for colour-vision deficiency and contrast against the
GitHub light (#ffffff) and dark (#0d1117) page backgrounds.
"""

from __future__ import annotations

import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

from .analysis import EXTENDED, FAMILIES, TAU_LEVELS  # noqa: E402

THEMES = {
    "light": {"surface": "#ffffff", "text": "#0b0b0b", "muted": "#52514e", "grid": "#e4e3df",
              "s1": "#2a78d6", "s2": "#eb6834"},
    "dark": {"surface": "#0d1117", "text": "#f0efec", "muted": "#c3c2b7", "grid": "#30363d",
             "s1": "#3987e5", "s2": "#d95926"},
}
PANELS = FAMILIES + EXTENDED


def _style(theme: dict):
    plt.rcParams.update({
        "svg.hashsalt": "inside-the-block", "svg.fonttype": "none", "font.family": "sans-serif",
        "font.sans-serif": ["DejaVu Sans", "Helvetica", "Arial"], "font.size": 9,
        "figure.facecolor": theme["surface"], "axes.facecolor": theme["surface"], "savefig.facecolor": theme["surface"],
        "text.color": theme["text"], "axes.labelcolor": theme["muted"], "xtick.color": theme["muted"],
        "ytick.color": theme["muted"], "axes.edgecolor": theme["grid"], "axes.grid": True, "grid.color": theme["grid"],
        "grid.linewidth": 0.75, "axes.spines.top": False, "axes.spines.right": False, "lines.linewidth": 2,
        "lines.solid_capstyle": "round", "lines.solid_joinstyle": "round",
    })


def _ci(p: dict):
    if not p or not p.get("n"):
        return None
    lo, hi = p["ci95"]
    return p["p"], p["p"] - lo, hi - p["p"]


def _series(ax, xs, props, color, surface, offset=0.0):
    pts = [(x + offset, c) for x, c in zip(xs, map(_ci, props), strict=True) if c is not None]
    if not pts:
        return
    x = [a for a, _ in pts]
    y = [c[0] for _, c in pts]
    err = [[c[1] for _, c in pts], [c[2] for _, c in pts]]
    ax.errorbar(x, y, yerr=err, color=color, linewidth=2, elinewidth=1, capsize=0, marker="o", markersize=6,
                markeredgecolor=surface, markeredgewidth=1.5, zorder=3)


def _grid(n):
    cols = 3
    rows = (n + cols - 1) // cols
    return rows, cols


def _save(fig, path: Path):
    fig.savefig(path, format="svg", metadata={"Date": None, "Creator": None}, bbox_inches="tight")
    plt.close(fig)


def turn_map(res: dict, theme: dict, path: Path):
    _style(theme)
    rows, cols = _grid(len(PANELS))
    fig, axes = plt.subplots(rows, cols, figsize=(9, 2.3 * rows), sharex=True, sharey=True)
    for ax, fam in zip(axes.flat, PANELS, strict=False):
        prof = res["P2"].get(fam, {}).get("profile", {})
        land = [prof.get(str(t), {}).get("land") for t in TAU_LEVELS]
        leak = [prof.get(str(t), {}).get("leak") for t in TAU_LEVELS]
        _series(ax, TAU_LEVELS, land, theme["s1"], theme["surface"])
        _series(ax, TAU_LEVELS, leak, theme["s2"], theme["surface"], offset=12)
        ax.set_title(fam + (" (extended)" if fam in EXTENDED else ""), loc="left", fontsize=9, color=theme["text"])
        ax.set_ylim(-0.03, 1.03)
        ax.set_yticks([0, 0.5, 1])
    for ax in list(axes.flat)[len(PANELS):]:
        ax.set_visible(False)
    fig.supxlabel("send offset τ (ms after the first shred of the slot before the target's turn)", color=theme["muted"],
                  fontsize=9)
    from matplotlib.lines import Line2D

    keys = [Line2D([], [], color=theme["s1"], marker="o", label="lands in the target's turn"),
            Line2D([], [], color=theme["s2"], marker="o", label="lands with the preceding leader")]
    fig.legend(handles=keys, loc="upper left", ncol=2, frameon=False, bbox_to_anchor=(0.0, 1.04),
               labelcolor=theme["text"])
    fig.supylabel("share of probes (95 % interval)", color=theme["muted"], fontsize=9)
    fig.tight_layout()
    _save(fig, path)


def order(res: dict, theme: dict, path: Path):
    _style(theme)
    fig, axes = plt.subplots(1, 2, figsize=(9, 3.6), sharey=True)
    ys = list(range(len(PANELS)))[::-1]
    for ax, (title, ref, get) in zip(axes, (
            ("Equal keys: first-sent member lands first (F)", 0.5, lambda f: res["P1"].get(f)),
            ("R pairs sent before the turn: high key first", 0.75, lambda f: res["S1"].get(f))), strict=True):
        ax.axvline(ref, color=theme["muted"], linewidth=1, zorder=1)
        for y, fam in zip(ys, PANELS, strict=True):
            c = _ci(get(fam))
            if c is None:
                ax.text(0.02, y, "no data", va="center", fontsize=8, color=theme["muted"])
                continue
            ax.errorbar([c[0]], [y], xerr=[[c[1]], [c[2]]], color=theme["s1"], elinewidth=2, capsize=0, marker="o",
                        markersize=6, markeredgecolor=theme["surface"], markeredgewidth=1.5, zorder=3)
            ax.text(1.04, y, f"n={get(fam)['n']}", va="center", fontsize=8, color=theme["muted"],
                    transform=ax.get_yaxis_transform())
        ax.set_xlim(0, 1)
        ax.set_title(title, loc="left", fontsize=9, color=theme["text"])
        ax.set_xlabel("share of eligible pairs (95 % interval)")
    axes[0].set_yticks(ys)
    axes[0].set_yticklabels(PANELS)
    fig.tight_layout()
    _save(fig, path)


def race(res: dict, theme: dict, path: Path):
    _style(theme)
    rows, cols = _grid(len(PANELS))
    fig, axes = plt.subplots(rows, cols, figsize=(9, 2.3 * rows), sharex=True, sharey=True)
    for ax, fam in zip(axes.flat, PANELS, strict=False):
        r = res.get("RRR", {}).get(fam, {})
        dts = sorted(int(k) for k in r)
        _series(ax, dts, [r[str(d)] for d in dts], theme["s1"], theme["surface"])
        ax.axhline(0.5, color=theme["muted"], linewidth=1, zorder=1)
        ax.set_title(fam + (" (extended)" if fam in EXTENDED else ""), loc="left", fontsize=9, color=theme["text"])
        ax.set_ylim(-0.03, 1.03)
        ax.set_yticks([0, 0.5, 1])
        ax.set_xscale("symlog", linthresh=2)
        ax.set_xticks([0, 2, 10, 25, 50, 100])
        ax.set_xticklabels(["0", "2", "10", "25", "50", "100"])
    for ax in list(axes.flat)[len(PANELS):]:
        ax.set_visible(False)
    fig.supxlabel("gap Δt: the low-key member was sent this many ms earlier", color=theme["muted"], fontsize=9)
    fig.supylabel("high key lands first (95 % interval)", color=theme["muted"], fontsize=9)
    fig.tight_layout()
    _save(fig, path)


def render_all(res_dir: Path, fig_dir: Path) -> list[str]:
    path = Path(res_dir) / "results.json"
    if not path.exists():
        return []
    res = json.loads(path.read_text())
    fig_dir = Path(fig_dir)
    fig_dir.mkdir(parents=True, exist_ok=True)
    names = []
    for mode, theme in THEMES.items():
        for name, fn in (("turn-map", turn_map), ("order", order), ("race", race)):
            fn(res, theme, fig_dir / f"{name}-{mode}.svg")
            names.append(f"{name}-{mode}.svg")
    return names

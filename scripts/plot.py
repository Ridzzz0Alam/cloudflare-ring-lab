"""results/*.csv -> figures/*.png (2x resolution, white background).

    .venv/bin/python scripts/plot.py

Each chart reads the latest run of its experiment (results/<name>_<timestamp>.csv).
"""
import glob
import re
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

ROOT = Path(__file__).resolve().parent.parent
FIG = ROOT / "figures"

# Reference palette (dataviz skill, light mode): fixed categorical order.
BLUE, ORANGE, AQUA, VIOLET = "#2a78d6", "#eb6834", "#1baf7a", "#6250d6"
RAMP = ["#86b6ef", "#3987e5", "#1c5cab", "#0d366b"]  # ordinal blue, light -> dark
INK, INK2, MUTED, GRID = "#0b0b0b", "#52514e", "#8a8984", "#e6e5e1"

plt.rcParams.update({
    "figure.dpi": 100, "savefig.dpi": 200, "savefig.facecolor": "white", "figure.facecolor": "white",
    "font.size": 12, "axes.titlesize": 13, "axes.labelsize": 12, "axes.edgecolor": MUTED,
    "axes.labelcolor": INK2, "xtick.color": INK2, "ytick.color": INK2, "axes.titlecolor": INK,
    "axes.grid": True, "grid.color": GRID, "grid.linewidth": 0.8, "axes.spines.top": False,
    "axes.spines.right": False, "lines.linewidth": 2, "legend.frameon": False, "axes.axisbelow": True,
})


def latest(name):
    files = [f for f in glob.glob(str(ROOT / f"results/{name}_*.csv"))
             if re.search(rf"{name}_\d{{8}}T\d{{6}}Z\.csv$", f)]
    if not files:
        raise SystemExit(f"no results for {name}: run the matching example first")
    return pd.read_csv(sorted(files)[-1])


def save(fig, name, note=None):
    if note:
        fig.text(0.01, -0.02, note, fontsize=9, color=MUTED, ha="left", va="top")
    fig.savefig(FIG / f"{name}.png", bbox_inches="tight", pad_inches=0.15)
    plt.close(fig)
    print("wrote", f"figures/{name}.png")


def pct_axis(ax, axis="y"):
    a = ax.yaxis if axis == "y" else ax.xaxis
    a.set_major_formatter(matplotlib.ticker.FuncFormatter(lambda v, _: f"{v:g}%"))

    def minor(v, _):  # on log axes, label only the 2s and 5s between decades
        if v <= 0:
            return ""
        lead = round(v / 10 ** np.floor(np.log10(v)))
        return f"{v:g}%" if lead in (2, 5) else ""
    a.set_minor_formatter(matplotlib.ticker.FuncFormatter(minor))


# --- E1 ---------------------------------------------------------------------
def e1():
    df = latest("e1_balance")
    fig, axes = plt.subplots(1, 3, figsize=(13, 4.2), sharey=True)
    for ax, n in zip(axes, [10, 100, 1000]):
        d = df[df.n == n]
        crc, rnd = d[d.variant == "crc"], d[d.variant == "random"]
        ax.plot(crc.k, crc.theory_cv * 100, color=INK2, ls="--", lw=1.5, label="theory √((N−1)/(Nk+1))")
        ax.plot(crc.k, crc.cv_rms * 100, "o", color=BLUE, ms=8, mec="white", mew=1.5, label="CRC32 chain (crate)")
        ax.plot(rnd.k, rnd.cv_rms * 100, "s", color=ORANGE, ms=6, mec="white", mew=1, label="uniform random points")
        ax.set_xscale("log"); ax.set_yscale("log")
        ax.set_title(f"N = {n} servers"); ax.set_xlabel("points per server (k)")
        pct_axis(ax)
        k160 = crc[crc.k == 160].iloc[0]
        ax.annotate(f"k=160: {k160.cv_rms*100:.1f}%", (160, k160.cv_rms * 100), xytext=(8, 14),
                    textcoords="offset points", color=INK2, fontsize=10)
    axes[0].set_ylabel("CV of server load")
    axes[0].legend(loc="lower left", fontsize=10)
    fig.suptitle("Load imbalance falls like 1/√k, exactly as the formula predicts", x=0.01, ha="left", color=INK)
    save(fig, "e1_cv_vs_k", f"30 rings per point, exact arc lengths. CV = std/mean of server shares.")

    # Overload: what CV means in practice.
    ks = [1, 16, 160, 1000, 10000]
    fig, ax = plt.subplots(figsize=(9, 4.2))
    w = 0.38
    x = np.arange(len(ks))
    for i, (n, c) in enumerate([(100, BLUE), (1000, ORANGE)]):
        d = df[(df.n == n) & (df.variant == "crc")].set_index("k").loc[ks]
        bars = ax.bar(x + (i - 0.5) * (w + 0.02), (d.max_ratio_mean - 1) * 100, w, color=c, label=f"N = {n}")
        for b, v in zip(bars, (d.max_ratio_mean - 1) * 100):
            ax.text(b.get_x() + b.get_width() / 2, v * 1.08, f"+{v:.0f}%" if v >= 10 else f"+{v:.1f}%",
                    ha="center", fontsize=9.5, color=INK2)
    ax.set_yscale("log"); pct_axis(ax)
    ax.set_xticks(x, [f"k = {k:,}" for k in ks])
    ax.set_ylabel("busiest server vs fair share")
    ax.legend(loc="upper right")
    ax.set_title("How overloaded is the busiest server? (average over 30 rings)", loc="left")
    save(fig, "e1_overload")


# --- E2 ---------------------------------------------------------------------
def e2():
    df = latest("e2_collisions")
    fig, axes = plt.subplots(1, 2, figsize=(13, 4.6), sharey=True)
    for ax, n in zip(axes, [256, 2048]):
        d = df[df.n == n]
        v = {name: g.set_index("k") for name, g in d.groupby("variant")}
        k = v["crc"].index
        ax.plot(k, v["crc"].theory_cv * 100, color=INK2, ls="--", lw=1.5, label="theory (no collisions)")
        ax.plot(k, v["crc"].theory_cv_with_tie_bias * 100, color=MUTED, ls=":", lw=2,
                label="theory + tie-break bias q/√12")
        ax.plot(k, v["crc"].cv * 100, "o-", color=BLUE, ms=8, mec="white", mew=1.5,
                label="pingora-ketama V2 (lowest address wins)")
        ax.plot(k, v["crc_hashed_tie"].cv * 100, "s-", color=AQUA, ms=7, mec="white", mew=1,
                label="same ring, fair (hashed) tie-break")
        ax.set_xscale("log"); ax.set_yscale("log"); pct_axis(ax)
        ax.set_title(f"N = {n} servers"); ax.set_xlabel("points per server (k)")
        ax.axvspan(1e4, 1e5, color="#fff3bf", alpha=0.6, lw=0)
    axes[0].set_ylabel("CV of server load")
    axes[1].text(1.05e4, 3.6, "where Cloudflare saw\nthe error rise", fontsize=9.5, color=INK2, va="top")
    axes[0].legend(loc="lower left", fontsize=9.5)
    fig.suptitle("At high k, 32-bit collisions + a deterministic tie-break make error grow again",
                 x=0.01, ha="left", color=INK)
    save(fig, "e2_collisions", "Exact arc lengths; 3 rings per point up to 50M points, 1 ring above.")


# --- E3 ---------------------------------------------------------------------
def e3():
    df = latest("e3_weights_servers")
    df = df[df.rep == 0]
    fig, axes = plt.subplots(1, 2, figsize=(12, 4.8), sharex=True, sharey=True)
    for ax, ver in zip(axes, ["V1(160)", "V2(16)"]):
        d = df[df.version == ver]
        lim = max(d.expected_share.max(), d.actual_share.max()) * 100 * 1.08
        ax.plot([0, lim], [0, lim], color=INK2, ls="--", lw=1.2, label="actual = expected")
        for w, c in zip([1, 2, 4, 8], RAMP):
            g = d[d.weight == w]
            ax.scatter(g.expected_share * 100, g.actual_share * 100, s=46, color=c, edgecolor="white",
                       linewidth=1.2, label=f"weight {w}", zorder=3)
        ax.set_title(f"{ver} points per weight unit")
        ax.set_xlabel("expected share (weight / total)"); pct_axis(ax); pct_axis(ax, "x")
    axes[0].set_ylabel("actual share of the ring")
    axes[0].legend(loc="upper left", fontsize=10)
    fig.suptitle("Weights work: share tracks weight, with the most scatter for the lightest servers",
                 x=0.01, ha="left", color=INK)
    save(fig, "e3_weights", "One random fleet of 100 servers, weights drawn from {1, 2, 4, 8}.")


# --- E4 ---------------------------------------------------------------------
def e4():
    df = latest("e4_memory")
    d = df[(df.n == 2048) & (df.weight == 10)].copy()
    d["label"] = d.version + "(" + d.point_multiple.astype(str) + ")"
    fig, ax = plt.subplots(figsize=(9, 4.4))
    x = np.arange(len(d)); w = 0.38
    for i, (col, c, lab) in enumerate([("final_bytes", BLUE, "ring after build"),
                                       ("peak_bytes", ORANGE, "peak while building")]):
        bars = ax.bar(x + (i - 0.5) * (w + 0.02), d[col] / 1e6, w, color=c, label=lab)
        for b, v in zip(bars, d[col] / 1e6):
            ax.text(b.get_x() + b.get_width() / 2, v + 0.6, f"{v:.1f} MB", ha="center", fontsize=10, color=INK2)
    ax.set_xticks(x, d.label)
    ax.set_ylabel("heap (MB, dhat)")
    ax.legend(loc="upper right")
    ax.set_title("2048 servers × weight 10: V2 stores 6 B/point (−25%), but peaks at ~12 B/point while sorting",
                 loc="left", fontsize=12)
    save(fig, "e4_memory")


# --- E5 ---------------------------------------------------------------------
def e5():
    df = latest("e5_tradeoff")
    fig, axes = plt.subplots(1, 2, figsize=(13, 4.6))
    titles = {"weight1": "every server weight 1", "storage": "storage-style weights (400–850, avg ≈ 625)"}
    for ax, fl in zip(axes, ["weight1", "storage"]):
        for n, c in [(100, BLUE), (1000, ORANGE)]:
            d = df[(df.fleet == fl) & (df.n == n)].sort_values("k")
            ax.plot(d.bytes_per_server_v2, d.max_overload_mean * 100, "o-", color=c, ms=6, mec="white",
                    label=f"N = {n}")
            for k in (16, 160):
                r = d[d.k == k].iloc[0]
                off = (6, 8) if n == 1000 else (8, -16)
                ax.annotate(f"{k}/unit: +{r.max_overload_mean*100:.1f}%", (r.bytes_per_server_v2, r.max_overload_mean * 100),
                            xytext=off, textcoords="offset points", fontsize=9.5, color=c)
        ax.set_xscale("log"); ax.set_yscale("log"); pct_axis(ax)
        ax.set_xlabel("ring memory per server (bytes, V2 at 6 B/point)")
        ax.set_title(titles[fl])
    axes[0].set_ylabel("busiest server vs fair share")
    axes[0].legend(loc="upper right")
    fig.suptitle("Memory vs balance: a 90% cut is cheap with big weights, costly with weight 1",
                 x=0.01, ha="left", color=INK)
    save(fig, "e5_tradeoff", "Average over 30 random fleets; exact arc lengths.")


# --- E6 ---------------------------------------------------------------------
def e6():
    df = latest("e6_churn")
    d = df[df.n == 100]
    order = ["remove 1 server", "add 1 server", "V1(160) → V2(160)", "step 160 → 80", "step 80 → 40",
             "step 40 → 16", "V1(160) → V2(16)", "naive hash % N, add 1"]
    d = d.set_index("scenario").loc[order].iloc[::-1]
    colors = [ORANGE if "naive" in s else (VIOLET if "V2(16)" in s or "step" in s else BLUE) for s in d.index]
    fig, ax = plt.subplots(figsize=(9.5, 4.6))
    bars = ax.barh(np.arange(len(d)), d.moved_frac * 100, color=colors, height=0.62)
    for b, v in zip(bars, d.moved_frac * 100):
        ax.text(v + 1, b.get_y() + b.get_height() / 2, f"{v:.2f}%" if v < 10 else f"{v:.1f}%", va="center",
                fontsize=10, color=INK2)
    ax.set_yticks(np.arange(len(d)), d.index)
    ax.set_xlim(0, 110); pct_axis(ax, "x")
    ax.set_xlabel("keys that change server (N = 100, 1M keys)")
    ax.set_title("Consistent hashing moves ~1/N of keys; cutting points moves ~90%", loc="left")
    ax.grid(axis="y", visible=False)
    save(fig, "e6_churn")


# --- E7 ---------------------------------------------------------------------
def e7():
    a = latest("e7_bench"); b = latest("e7_bench_rayon1")
    a = a[a.group == "build"]; b = b[b.group == "build"]
    sizes = ["100x1", "253x10", "2048x10"]
    fig, axes = plt.subplots(1, 3, figsize=(13, 4))
    for ax, size in zip(axes, sizes):
        rows = []
        for ver in ["V1(160)", "V2(160)", "V2(16)"]:
            rows.append((ver, a[(a.version == ver) & (a["size"] == size)].median_ns.iloc[0] / 1e6,
                         b[(b.version == ver) & (b["size"] == size)].median_ns.iloc[0] / 1e6))
        x = np.arange(3); w = 0.38
        for i, (idx, c, lab) in enumerate([(2, BLUE, "1 thread"), (1, ORANGE, "4 threads (rayon)")]):
            vals = [r[idx] for r in rows]
            bars = ax.bar(x + (i - 0.5) * (w + 0.02), vals, w, color=c, label=lab)
            for bb, v in zip(bars, vals):
                ax.text(bb.get_x() + bb.get_width() / 2, v * 1.02, f"{v:.2f}" if v < 10 else f"{v:.0f}",
                        ha="center", fontsize=9, color=INK2)
        ax.set_xticks(x, [r[0] for r in rows])
        servers, weight = size.split("x")
        ax.set_title(f"{servers} servers × weight {weight}")
    axes[0].set_ylabel("ring build time (ms, median)")
    fig.legend(*axes[0].get_legend_handles_labels(), loc="upper right", ncol=2, fontsize=10,
               bbox_to_anchor=(0.99, 0.0))
    fig.suptitle("Build time: V2's sort only wins with threads and big rings; fewer points always wins",
                 x=0.01, ha="left", color=INK)
    save(fig, "e7_build_time", "criterion medians; V1 ignores RAYON_NUM_THREADS (it uses std's sort).")


if __name__ == "__main__":
    FIG.mkdir(exist_ok=True)
    for f in (e1, e2, e3, e4, e5, e6, e7):
        f()

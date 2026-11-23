#!/usr/bin/env python3
"""Summarise a soak.csv from tests/device/soak.sh (scripts/e2e.sh soak) as Markdown.

usage: scripts/soak-report.py .cache/e2e/<ts>/soak.csv [--plot out.png]

Per process (by name): PSS / anonymous PSS / RSS / THP / GPU at the end of the baseline,
after warm-up (first quarter of the activity phase), at the end of activity, at the end
of the final idle phase, and the peak; the least-squares slope of anonymous PSS over the
last three quarters of the activity phase (MiB/h, the "sustained growth" figure); CPU
use per phase; and pid changes (restarts). Units: MiB unless noted.
"""
import csv
import sys
from collections import defaultdict

HZ = 100  # USER_HZ on Linux/aarch64


def slope(points):
    """Least-squares slope of (t, v) points, per hour."""
    n = len(points)
    if n < 3:
        return float("nan")
    mt = sum(t for t, _ in points) / n
    mv = sum(v for _, v in points) / n
    den = sum((t - mt) ** 2 for t, _ in points)
    return sum((t - mt) * (v - mv) for t, v in points) / den * 3600 if den else float("nan")


def main():
    path = sys.argv[1]
    rows = list(csv.DictReader(open(path)))
    by = defaultdict(list)
    for r in rows:
        for k in ("t", "pid", "rss_kb", "pss_kb", "pss_anon_kb", "private_kb", "swap_kb", "thp_kb", "gpu_kb", "cpu_ticks"):
            r[k] = int(r[k])
        by[r["proc"]].append(r)

    act = [r["t"] for r in rows if r["phase"] == "activity"]
    a0, a1 = min(act), max(act)
    warm = a0 + (a1 - a0) / 4
    end = max(r["t"] for r in rows)
    print(f"Samples: {len(rows)} rows, duration {end // 60} min "
          f"(activity {a0 // 60}–{a1 // 60} min, warm-up until {int(warm) // 60} min)\n")

    def at(samples, t):  # sample closest to time t
        return min(samples, key=lambda r: abs(r["t"] - t))

    def last(samples, phase):  # last sample of a phase (a stopped run may lack "final")
        s = [r for r in samples if r["phase"] == phase] or [samples[-1]]
        return s[-1]

    mib = lambda kb: f"{kb / 1024:.1f}"
    print("| Process | metric | baseline | after warm-up | end of activity | final idle | peak | sustained slope (MiB/h) |")
    print("|---|---|---|---|---|---|---|---|")
    order = ["quickshell", "phoc", "squeekboard", "pocketd", "pipewire", "wireplumber", "pipewire-pulse"]
    for name in order:
        s = by.get(name)
        if not s:
            continue
        pids = sorted({r["pid"] for r in s}, key=lambda p: min(r["t"] for r in s if r["pid"] == p))
        for metric in ("pss_kb", "pss_anon_kb", "rss_kb", "thp_kb", "gpu_kb"):
            b, w, e, f = last(s, "baseline"), at(s, warm), last(s, "activity"), last(s, "final")
            sustained = [(r["t"], r[metric] / 1024) for r in s if r["phase"] == "activity" and r["t"] >= warm]
            label = f"{name} (pids {len(pids)})" if metric == "pss_kb" else ""
            print(f"| {label} | {metric[:-3]} | {mib(b[metric])} | {mib(w[metric])} | {mib(e[metric])} | "
                  f"{mib(f[metric])} | {mib(max(r[metric] for r in s))} | {slope(sustained):+.1f} |")
    sysr = by["system"]
    for metric, label in (("rss_kb", "used (Total−Available)"), ("swap_kb", "swap used"), ("thp_kb", "AnonHugePages")):
        b, w, e, f = last(sysr, "baseline"), at(sysr, warm), last(sysr, "activity"), last(sysr, "final")
        sustained = [(r["t"], r[metric] / 1024) for r in sysr if r["phase"] == "activity" and r["t"] >= warm]
        print(f"| system | {label} | {mib(b[metric])} | {mib(w[metric])} | {mib(e[metric])} | {mib(f[metric])} | "
              f"{mib(max(r[metric] for r in sysr))} | {slope(sustained):+.1f} |")

    print("\nCPU (% of one core, average per phase; restarts reset the tick counter and are excluded):\n")
    print("| Process | baseline | activity | final idle |")
    print("|---|---|---|---|")
    for name in order:
        s = by.get(name)
        if not s:
            continue
        cells = []
        for phase in ("baseline", "activity", "final"):
            ps = [r for r in s if r["phase"] == phase]
            ticks = secs = 0
            for a, b in zip(ps, ps[1:]):
                if a["pid"] == b["pid"] and b["t"] > a["t"]:
                    ticks += b["cpu_ticks"] - a["cpu_ticks"]
                    secs += b["t"] - a["t"]
            cells.append(f"{100 * ticks / HZ / secs:.1f}" if secs else "–")
        print(f"| {name} | " + " | ".join(cells) + " |")

    if "--plot" in sys.argv:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        out = sys.argv[sys.argv.index("--plot") + 1]
        fig, axes = plt.subplots(2, 1, figsize=(9, 6), sharex=True)
        for name in order[:4]:
            s = by.get(name)
            if s:
                axes[0].plot([r["t"] / 60 for r in s], [r["pss_anon_kb"] / 1024 for r in s], label=name)
        axes[0].set_ylabel("anonymous PSS (MiB)")
        axes[0].legend(fontsize=8)
        axes[1].plot([r["t"] / 60 for r in sysr], [r["rss_kb"] / 1024 for r in sysr], label="system used")
        axes[1].set_ylabel("system used (MiB)")
        axes[1].set_xlabel("minutes")
        for ax in axes:
            ax.axvspan(a0 / 60, a1 / 60, color="0.92", zorder=0)
            ax.grid(alpha=0.3)
        fig.tight_layout()
        fig.savefig(out, dpi=90)


if __name__ == "__main__":
    main()

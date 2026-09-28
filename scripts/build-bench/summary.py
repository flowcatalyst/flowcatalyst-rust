#!/usr/bin/env python3
"""Median and spread of bench.sh results, per label / scenario / subject.

    scripts/build-bench/summary.py [target/build-bench/results.csv] [--md]

Columns: runs, then median [min-max] of cargo_s (cargo's own build time), cpu_s,
compile_s, link_s, run_s, fc-platform's unit time and front end, and the
median 1-minute load average before each run.
"""
import csv
import statistics
import sys
from collections import OrderedDict

path = next((a for a in sys.argv[1:] if not a.startswith("--")), "target/build-bench/results.csv")
md = "--md" in sys.argv
groups = OrderedDict()
for r in csv.DictReader(open(path)):
    groups.setdefault((r["label"], r["scenario"], r["subject"]), []).append(r)


def stat(rows, col):
    vals = [float(r[col]) for r in rows if r.get(col) not in (None, "")]
    if not vals:
        return ""
    med = statistics.median(vals)
    if len(vals) == 1:
        return f"{med:.1f}"
    return f"{med:.1f} [{min(vals):.1f}–{max(vals):.1f}]"


cols = [("cargo_s", "build s"), ("cpu_s", "cpu s"), ("compile_s", "compile s"), ("link_s", "link s"), ("run_s", "run s"),
        ("platform_s", "fc-platform s"), ("platform_frontend_s", "fc-platform front end s"),
        ("load1_before", "load")]
head = ["label", "scenario", "subject", "runs"] + [c[1] for c in cols]
if md:
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
for (label, scen, subj), rows in groups.items():
    vals = [label, scen, subj, str(len(rows))] + [stat(rows, c) for c, _ in cols]
    if md:
        print("| " + " | ".join(vals) + " |")
    else:
        print("  ".join(vals))

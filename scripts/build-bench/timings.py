#!/usr/bin/env python3
"""Key numbers from cargo --timings HTML reports.

    scripts/build-bench/timings.py <report.html>... [--top N] [--crates fc-]

For each report: total time, dirty/total units, the N slowest units, and every
unit whose name starts with --crates (default "fc-") with its front-end /
codegen split (cargo >= 1.9x records "sections" per unit) and start offset.
"""
import json
import re
import sys

args = sys.argv[1:]
if args and args[0] == "--csv":
    # One line for bench.sh: cargo's own total (excludes time blocked on a
    # lock), then the named unit's duration and front-end time (summed over
    # every unit of that crate: lib, test, bin).
    path, crate = args[1], args[2]
    s = open(path).read()
    i = s.index("const UNIT_DATA = ") + len("const UNIT_DATA = ")
    units = json.loads(s[i:s.index("];", i) + 1])
    total = re.search(r"<td>Total time:</td>\s*<td>([\d.]+)s", s)
    mine = [u for u in units if u["name"] == crate]
    dur = sum(u["duration"] for u in mine)
    fe = sum(e - b for u in mine for n, sp in (u.get("sections") or []) if n == "frontend" for b, e in [(sp["start"], sp["end"])])
    print(f"{total.group(1) if total else ''},{dur:.1f},{fe:.1f}")
    sys.exit(0)
top = 12
prefix = "fc-"
files = []
i = 0
while i < len(args):
    if args[i] == "--top":
        top = int(args[i + 1])
        i += 2
    elif args[i] == "--crates":
        prefix = args[i + 1]
        i += 2
    else:
        files.append(args[i])
        i += 1


def load(path):
    s = open(path).read()
    i = s.index("const UNIT_DATA = ") + len("const UNIT_DATA = ")
    j = s.index("];", i) + 1
    units = json.loads(s[i:j])
    total = re.search(r"<td>Total time:</td>\s*<td>([\d.]+)s", s)
    dirty = re.search(r"<td>Dirty units:</td>\s*<td>(\d+)</td>", s)
    tot_units = re.search(r"<td>Total units:</td>\s*<td>(\d+)</td>", s)
    return units, float(total.group(1)) if total else None, dirty and dirty.group(1), tot_units and tot_units.group(1)


def split(u):
    fe = cg = None
    for name, span in u.get("sections") or []:
        d = span["end"] - span["start"]
        if name == "frontend":
            fe = d
        elif name == "codegen":
            cg = d
    return fe, cg


def label(u):
    t = u.get("target", "").strip()
    return f"{u['name']} {t}".strip()


for f in files:
    units, total, dirty, tot = load(f)
    print(f"== {f}\n   total {total}s, dirty units {dirty}/{tot}")
    print(f"   slowest {top}:")
    for u in sorted(units, key=lambda u: -u["duration"])[:top]:
        fe, cg = split(u)
        extra = f" (frontend {fe:.1f}s, codegen {cg:.1f}s)" if fe is not None and cg is not None else ""
        print(f"     {u['duration']:7.1f}s  {label(u)}{extra}  @{u['start']:.1f}s")
    ours = [u for u in units if u["name"].startswith(prefix)]
    if ours:
        print(f"   {prefix}* units:")
        for u in sorted(ours, key=lambda u: u["start"]):
            fe, cg = split(u)
            extra = f" (frontend {fe:.1f}s, codegen {cg:.1f}s)" if fe is not None and cg is not None else ""
            print(f"     {u['duration']:7.1f}s  {label(u)}{extra}  @{u['start']:.1f}s")

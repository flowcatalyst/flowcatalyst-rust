#!/usr/bin/env python3
"""Inventory of #[async_trait] traits: which are used as `dyn` and which only generically.

    scripts/build-bench/async_trait_inventory.py [root] [--crate crates/fc-platform] [--list]

A trait defined under #[async_trait] and named anywhere in the workspace as
`dyn Trait` (Arc<dyn ..>, Box<dyn ..>, &dyn ..) must stay object-safe: native
`async fn` in traits is not dyn-compatible, so those keep async_trait (or
become hand-written boxed-future traits). The rest can move to native
`async fn` in traits (Rust 1.75+), with `+ Send` bounds where callers spawn
(return-type notation is still unstable, so a trait whose futures must be
Send is written with `fn f(..) -> impl Future<Output = T> + Send`).

Counts: attribute sites (`#[async_trait]` on trait definitions and on impls),
per defining crate, split dyn / generic-only / external (defined outside the
scanned crates, e.g. a dependency's trait).
"""
import os
import re
import sys
from collections import defaultdict

args = sys.argv[1:]
root = "."
crate_filter = None
show_list = False
i = 0
while i < len(args):
    if args[i] == "--crate":
        crate_filter = args[i + 1]
        i += 2
    elif args[i] == "--list":
        show_list = True
        i += 1
    else:
        root = args[i]
        i += 1

SKIP = ("/target", "/node_modules", "/.git", "/frontend", "/clients")
files = []
for d, dirs, fs in os.walk(root):
    if any(s in d for s in SKIP):
        dirs[:] = []
        continue
    for f in fs:
        if f.endswith(".rs"):
            files.append(os.path.join(d, f))


def crate_of(path):
    parts = os.path.relpath(path, root).split(os.sep)
    for k in ("src", "tests", "benches", "examples"):
        if k in parts:
            return os.sep.join(parts[: parts.index(k)])
    return parts[0]


ATTR = r"#\[(?:async_trait::)?async_trait(?:\([^)]*\))?\]"
defs = {}  # (crate, trait) -> (crate, file, line, method_count)
impls = defaultdict(list)  # (crate, trait) -> [(crate, file, line)]
dyn_uses = defaultdict(set)  # (crate, trait) -> {crate}
raw_impls = []  # (trait name, crate, file, line)
texts = {}
for p in files:
    try:
        s = open(p, encoding="utf-8").read()
    except (UnicodeDecodeError, OSError):
        continue
    texts[p] = s
    for m in re.finditer(ATTR + r"\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?trait\s+(\w+)", s):
        name = m.group(1)
        body_start = s.find("{", m.end())
        depth, j = 1, body_start + 1
        while j < len(s) and depth:
            depth += {"{": 1, "}": -1}.get(s[j], 0)
            j += 1
        n_async = len(re.findall(r"\basync\s+fn\b", s[body_start:j]))
        defs[(crate_of(p), name)] = (crate_of(p), p, s.count("\n", 0, m.start()) + 1, n_async)
    for m in re.finditer(ATTR + r"\s*impl(?:<[^{]*?>)?\s+([\w:]+)(?:<[^{]*?>)?\s+for\s+", s):
        name = m.group(1).split("::")[-1]
        raw_impls.append((name, crate_of(p), p, s.count("\n", 0, m.start()) + 1))

# An impl belongs to the definition in its own crate when there is one (the
# same trait name is defined in several crates, e.g. UseCase in fc-platform
# and fc-sdk), else to the only definition with that name, else external.
for name, c, p, ln in raw_impls:
    key = (c, name)
    if key not in defs:
        cands = [k for k in defs if k[1] == name]
        key = cands[0] if len(cands) == 1 else ("(external)", name)
    impls[key].append((c, p, ln))

for p, s in texts.items():
    c = crate_of(p)
    for key in set(defs) | set(impls):
        name = key[1]
        if re.search(r"\bdyn\s+(?:[\w:]+::)?" + re.escape(name) + r"\b", s):
            # A same-named trait from another crate only counts where it is in scope.
            if key[0] == c or c not in {k[0] for k in defs if k[1] == name}:
                dyn_uses[key].add(c)

rows = []
for key in sorted(set(defs) | set(impls)):
    d = defs.get(key)
    crate, name = key
    if crate_filter and crate != crate_filter and not any(ic == crate_filter for ic, _, _ in impls[key]):
        continue
    kind = "external" if not d else ("dyn" if dyn_uses[key] else "generic")
    rows.append((crate, kind, name, 1 if d else 0, len(impls[key]), d[3] if d else 0,
                 sorted(dyn_uses[key]), d[1] + ":" + str(d[2]) if d else ""))

by = defaultdict(lambda: defaultdict(lambda: [0, 0, 0]))
for crate, kind, name, nd, ni, _, _, _ in rows:
    b = by[crate][kind]
    b[0] += 1
    b[1] += nd
    b[2] += ni
print("crate                          kind      traits  def-attrs  impl-attrs")
tot = defaultdict(lambda: [0, 0, 0])
for crate in sorted(by):
    for kind in ("dyn", "generic", "external"):
        if kind in by[crate]:
            t, nd, ni = by[crate][kind]
            tot[kind][0] += t
            tot[kind][1] += nd
            tot[kind][2] += ni
            print(f"{crate:30} {kind:9} {t:6} {nd:10} {ni:11}")
print("-" * 70)
for kind in ("dyn", "generic", "external"):
    t, nd, ni = tot[kind]
    print(f"{'total':30} {kind:9} {t:6} {nd:10} {ni:11}   ({nd + ni} attribute sites)")
if show_list:
    print("\ncrate | kind | trait | async fns | impls | dyn used in | defined at")
    for crate, kind, name, nd, ni, nfn, dyn_in, at in rows:
        print(f"{crate} | {kind} | {name} | {nfn} | {ni} | {','.join(dyn_in)} | {at}")

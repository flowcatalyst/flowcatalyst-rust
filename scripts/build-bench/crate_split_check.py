#!/usr/bin/env python3
"""Check a proposed crate split of fc-platform against its module graph.

    scripts/build-bench/module_graph.py crates/fc-platform/src --json graph.json >/dev/null
    scripts/build-bench/crate_split_check.py graph.json scripts/build-bench/crate-split.txt [--all]

The split file (see crate-split.txt) declares crates in dependency order and
assigns graph nodes, whole files, and individual item paths to them:

    crate <name> [: <dep> <dep> ...]     declares a crate and its direct deps
    mod   <crate> <node> <node> ...      graph nodes (module_graph.py's names)
    file  <crate> <glob> <glob> ...      source files (relative to src/), e.g. */routes.rs;
                                         a file without wildcards also moves its
                                         items (its module path becomes a move)
    move  <crate> <path> <path> ...      item paths after `crate::` (prefix match,
                                         `role::entity::permissions`), canonical
                                         (root re-exports are resolved)
    lines <crate> <file>:<from>-<to>     part of a file moves (source side only)
    invert <path> <path> ...             references to these items are broken by
                                         inverting the dependency (a trait or a
                                         parameter): listed, not counted
    # comment

Every reference in the graph is checked: a file in crate A referencing an
item that lands in crate B is fine when A == B or B is a (transitive) dep of
A; otherwise it is a violation, i.e. a cycle or a missing dependency the
split has to break. Output: lines per crate, then violations grouped by
(from crate, to crate, referenced item), with counts and example sites.
"""
import fnmatch
import json
import os
import sys
from collections import defaultdict

graph_path, split_path = sys.argv[1], sys.argv[2]
show_all = "--all" in sys.argv
g = json.load(open(graph_path))
SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../crates/fc-platform/src")

crates, deps, node_crate, file_rules, moves = [], {}, {}, [], []
line_rules, inverted = [], []
for raw in open(split_path):
    line = raw.split("#", 1)[0].strip()
    if not line:
        continue
    kw, rest = line.split(None, 1)
    if kw == "crate":
        name, _, ds = rest.partition(":")
        name = name.strip()
        crates.append(name)
        deps[name] = ds.split()
    elif kw == "mod":
        c, *ns = rest.split()
        for n in ns:
            node_crate[n] = c
    elif kw == "file":
        c, *gl = rest.split()
        file_rules += [(gl_, c) for gl_ in gl]
        # A moved file takes its items with it: its module path is a move too.
        for gl_ in gl:
            if "*" not in gl_:
                parts = gl_[:-3].split("/")
                if parts[-1] == "mod":
                    parts = parts[:-1]
                moves.append(("::".join(parts), c))
    elif kw == "move":
        c, *ps = rest.split()
        moves += [(p, c) for p in ps]
    elif kw == "lines":
        c, spec = rest.split()
        f, _, rng = spec.rpartition(":")
        a, b = rng.split("-")
        line_rules.append((f, int(a), int(b), c))
    elif kw == "invert":
        inverted += rest.split()
    else:
        sys.exit(f"unknown keyword {kw!r}")

closure = {}


def reach(c):
    if c not in closure:
        s = set()
        for d in deps[c]:
            s |= {d} | reach(d)
        closure[c] = s
    return closure[c]


def crate_of_file(f, node, line=None):
    if line is not None:
        for lf, a, b, c in line_rules:
            if lf == f and a <= line <= b:
                return c
    for gl, c in file_rules:
        if fnmatch.fnmatch(f, gl):
            return c
    return node_crate.get(node)


def crate_of_path(path, node):
    best = None
    for p, c in moves:
        if path == p or path.startswith(p + "::"):
            if best is None or len(p) > len(best[0]):
                best = (p, c)
    return best[1] if best else node_crate.get(node)


unassigned = sorted(n for n in g["lines"] if n not in node_crate)
if unassigned:
    print("unassigned nodes:", ", ".join(unassigned))

# Lines per crate: nodes, with file rules moving whole files.
lines = defaultdict(int)
file_lines = {}
for d, _, fs in os.walk(SRC):
    for f in fs:
        if f.endswith(".rs"):
            p = os.path.join(d, f)
            file_lines[os.path.relpath(p, SRC)] = open(p).read().count("\n")
moved = defaultdict(int)
for f, n in file_lines.items():
    for gl, c in file_rules:
        if fnmatch.fnmatch(f, gl):
            moved[c] += n
            parts = f[:-3].split(os.sep)
            if parts[-1] == "mod":
                parts = parts[:-1]
            node = parts[0] + "::" + parts[1] if parts[0] == "shared" and len(parts) > 1 else parts[0]
            lines[node_crate.get(node, "?")] -= n
            break
for n, k in g["lines"].items():
    lines[node_crate.get(n, "?")] += k
for c, k in moved.items():
    lines[c] += k
print("## Lines per crate")
for c in crates:
    print(f"{lines[c]:>8}  {c}  (deps: {' '.join(deps[c]) or '-'})")

viol = defaultdict(lambda: {"n": 0, "sites": []})
inv_hits = defaultdict(list)
for e in g["edges"]:
    for ref in e["refs"]:
        site, path = ref.split(" ", 1)
        f, _, ln = site.rpartition(":")
        src_c = crate_of_file(f, e["from"], int(ln))
        dst_c = crate_of_path(path, e["to"])
        if src_c is None or dst_c is None or src_c == dst_c or dst_c in reach(src_c):
            continue
        inv = next((p for p in inverted if path == p or path.startswith(p + "::")), None)
        if inv:
            inv_hits[inv].append(f"{site} [{src_c} -> {dst_c}]")
            continue
        head = "::".join(path.split("::")[:3])
        v = viol[(src_c, dst_c, head)]
        v["n"] += 1
        v["sites"].append(site)

if inverted:
    print("\n## Inverted (a trait or a parameter replaces the reference)")
    for p in inverted:
        hits = inv_hits.get(p, [])
        print(f"   {len(hits):4}  {p}   e.g. {', '.join(sorted(set(hits))[:2])}")

by_pair = defaultdict(list)
for (a, b, head), v in viol.items():
    by_pair[(a, b)].append((head, v))
print(f"\n## Violations: {sum(v['n'] for v in viol.values())} references, "
      f"{len(viol)} distinct items, {len(by_pair)} crate pairs")
for (a, b), items in sorted(by_pair.items(), key=lambda kv: -sum(v["n"] for _, v in kv[1])):
    print(f"\n{a} -> {b}: {sum(v['n'] for _, v in items)} refs")
    for head, v in sorted(items, key=lambda x: -x[1]["n"])[: None if show_all else 12]:
        sites = sorted(set(v["sites"]))
        print(f"   {v['n']:4}  {head}   e.g. {', '.join(sites[:3])}")


# ── Coherence: inherent impls and the orphan rule ──────────────────────────
# A split is only legal if every `impl Type` lives in the crate that defines
# Type, and every `impl Trait for Type` has the trait or some type in it
# (self type or a trait type argument) local to the impl's crate. Checked by
# name (last path segment); names defined more than once are skipped.
import re  # noqa: E402

defs = defaultdict(set)
impl_sites = []
for f in file_lines:
    src = open(os.path.join(SRC, f)).read()
    parts = f[:-3].split(os.sep)
    if parts[-1] == "mod":
        parts = parts[:-1]
    node = "<root>" if parts == ["lib"] else (
        parts[0] + "::" + parts[1] if parts[0] == "shared" and len(parts) > 1 else parts[0])
    fc = crate_of_file(f, node)
    modpath = "::".join(parts)
    for m in re.finditer(r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|union|type)\s+(\w+)", src, re.M):
        name = m.group(1)
        c = crate_of_path(modpath + "::" + name, node) if moves else fc
        # an item moved by path (e.g. UserScope) lands in the move's crate
        for p, mc in moves:
            if p == name or (modpath + "::" + name).startswith(p):
                c = mc
        defs[name].add(c)
    for m in re.finditer(r"^[ \t]*(?:unsafe\s+)?impl\b(?:\s*<[^{]*?>)?\s+([^{]+?)\s*(?:where\b[^{]*)?\{", src, re.M):
        head = m.group(1).strip()
        trait, _, selfty = head.partition(" for ")
        if not selfty:
            trait, selfty = "", head
        ln = src.count("\n", 0, m.start()) + 1
        impl_sites.append((f, ln, crate_of_file(f, node, ln), trait.strip(), selfty.strip()))


def names_in(t):
    return re.findall(r"\b([A-Z]\w*)\b", t)


def last_name(t):
    t = re.sub(r"<.*", "", t.replace("&", "").replace("dyn ", "").replace("mut ", "")).strip()
    return t.split("::")[-1]


GENERIC_PARAMS = {"T", "U", "E", "S", "R", "B", "F", "C", "K", "V", "Self"}
bad = []
for f, ln, fc, trait, selfty in impl_sites:
    if fc is None:
        continue
    self_name = last_name(selfty)
    self_defs = defs.get(self_name, set())
    if not trait:
        if len(self_defs) == 1 and fc not in self_defs:
            bad.append((f, ln, fc, f"impl {selfty}", f"inherent impl of a type in {next(iter(self_defs))}"))
        continue
    trait_name = last_name(trait)
    trait_defs = defs.get(trait_name, set())
    if fc in trait_defs:
        continue
    local = [n for n in names_in(trait) + names_in(selfty) if n not in GENERIC_PARAMS and fc in defs.get(n, set())]
    if local:
        continue
    known = [n for n in names_in(trait) + names_in(selfty) if defs.get(n)]
    if not known:
        continue  # nothing of ours involved (e.g. a macro-generated or foreign impl)
    where = {n: sorted(defs[n]) for n in known}
    bad.append((f, ln, fc, f"impl {trait} for {selfty}", f"orphan: nothing local ({where})"))

print(f"\n## Coherence violations (inherent impls / orphan rule): {len(bad)}")
for f, ln, fc, what, why in bad:
    print(f"   {f}:{ln} [{fc}] {what[:90]} -- {why[:160]}")

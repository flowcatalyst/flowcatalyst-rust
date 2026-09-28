#!/usr/bin/env python3
"""The module dependency graph inside a crate, from its `crate::` paths.

    scripts/build-bench/module_graph.py [crate_src_dir] [--json out.json] [--dot out.dot]
        [--split shared,auth] [--edges-for a,b]

A node is a top-level module of the crate (`src/<m>/` or `src/<m>.rs`), except
the modules named in --split (default: `shared`), which are split one level
further (`shared::database`, `shared::middleware`, ...) because they are grab
bags. Items the crate root re-exports (`pub use x::y::Z;` in lib.rs, the inline
`pub mod repository { ... }`) are resolved to the module they come from.

References are every `crate::<module>` path in the code (use trees included)
and `super::` paths that climb out of a split module. Comments and string
literals are stripped first. References from `#[cfg(test)] mod ... { }` blocks
are counted separately ("test" edges): they don't constrain the crate split in
the same way (a test can move with the module or into an integration test).

Output: lines per node, the edge list with weights and an example site, the
strongly connected components (the cycles a crate split has to break), and
for each non-trivial SCC the edges inside it sorted by weight (the cheapest
edges to cut first).
"""
import json
import os
import re
import sys
from collections import defaultdict

args = sys.argv[1:]
opts = {}
pos = []
i = 0
while i < len(args):
    if args[i].startswith("--"):
        opts[args[i][2:]] = args[i + 1]
        i += 2
    else:
        pos.append(args[i])
        i += 1
SRC = pos[0] if pos else "crates/fc-platform/src"
SPLIT = set(opts.get("split", "shared").split(",")) if opts.get("split", "shared") else set()


def strip_comments_and_strings(s):
    out = []
    i, n = 0, len(s)
    while i < n:
        c = s[i]
        if s.startswith("//", i):
            j = s.find("\n", i)
            i = n if j < 0 else j
        elif s.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if s.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif s.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    if s[i] == "\n":
                        out.append("\n")
                    i += 1
        elif c == "r" and re.match(r'r#*"', s[i:i + 10]) and (i == 0 or not (s[i - 1].isalnum() or s[i - 1] == "_")):
            m = re.match(r'r(#*)"', s[i:])
            close = '"' + m.group(1)
            j = s.find(close, i + len(m.group(0)))
            j = n if j < 0 else j + len(close)
            out.append('""' + "\n" * s.count("\n", i, j))
            i = j
        elif c == '"':
            j = i + 1
            while j < n and s[j] != '"':
                j += 2 if s[j] == "\\" else 1
            out.append('""' + "\n" * s.count("\n", i, j))
            i = j + 1
        elif c == "'" and re.match(r"'(\\.|[^\\'])'", s[i:i + 4]):
            m = re.match(r"'(\\.|[^\\'])'", s[i:])
            out.append("' '")
            i += len(m.group(0))
        else:
            out.append(c)
            i += 1
    return "".join(out)


def split_test_blocks(code):
    """Return (non_test_code, test_code): `#[cfg(test)] mod x { ... }` bodies go to test."""
    main, test = [], []
    pos = 0
    for m in re.finditer(r"#\[cfg\(test\)\]\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{", code):
        if m.start() < pos:
            continue
        main.append(code[pos:m.start()])
        test.append("\n" * code.count("\n", pos, m.start()))
        depth, j = 1, m.end()
        while j < len(code) and depth:
            if code[j] == "{":
                depth += 1
            elif code[j] == "}":
                depth -= 1
            j += 1
        test.append(code[m.start():j])
        main.append("\n" * code.count("\n", m.start(), j))
        pos = j
    main.append(code[pos:])
    return "".join(main), "".join(test)


def module_path(path):
    rel = os.path.relpath(path, SRC)[:-3].split(os.sep)
    if rel[-1] == "mod":
        rel = rel[:-1]
    if rel == ["lib"]:
        return []
    return rel


def node_of(mod):
    if not mod:
        return "<root>"
    if mod[0] in SPLIT and len(mod) > 1 and mod[1] != "*":
        return mod[0] + "::" + mod[1]
    return mod[0]


files = []
for d, _, fs in os.walk(SRC):
    for f in fs:
        if f.endswith(".rs"):
            files.append(os.path.join(d, f))

top_modules = set()
for p in files:
    mp = module_path(p)
    if mp:
        top_modules.add(mp[0])

# Root re-exports: name -> node.
lib_src = strip_comments_and_strings(open(os.path.join(SRC, "lib.rs")).read())
root_names = {}
root_paths = {}  # re-exported name -> its canonical path (`UserScope` -> `principal::entity::UserScope`)


def use_tree_leaves(prefix, tree):
    """Expand `a::{b, c::{d, e as f}}` into (path, leaf_name) pairs."""
    tree = tree.strip()
    res = []
    m = re.match(r"^([\w:]*?)(::)?\{(.*)\}$", tree, re.S)
    if m:
        base = prefix + ([x for x in m.group(1).split("::") if x])
        depth, cur, parts = 0, "", []
        for ch in m.group(3):
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            if ch == "," and depth == 0:
                parts.append(cur)
                cur = ""
            else:
                cur += ch
        parts.append(cur)
        for part in parts:
            if part.strip():
                res += use_tree_leaves(base, part)
        return res
    tree = re.sub(r"\s+as\s+\w+$", "", tree)
    segs = [x for x in tree.split("::") if x]
    full = prefix + segs
    return [(full, full[-1] if full else "")]


# Inline modules in lib.rs (e.g. `pub mod repository { ... }`) are nodes of their own.
inline_mods = {}
for m in re.finditer(r"pub mod (\w+)\s*\{", lib_src):
    if lib_src[:m.start()].count("{") != lib_src[:m.start()].count("}"):
        continue  # nested inside another inline module
    depth, j = 1, m.end()
    while depth:
        if lib_src[j] == "{":
            depth += 1
        elif lib_src[j] == "}":
            depth -= 1
        j += 1
    inline_mods[m.group(1)] = lib_src[m.end():j - 1]
lib_outer = lib_src
for name, body in inline_mods.items():
    lib_outer = lib_outer.replace(body, "")

for m in re.finditer(r"pub use\s+([^;]+);", lib_outer):
    for path, leaf in use_tree_leaves([], m.group(1)):
        if path and path[0] in top_modules:
            root_names[leaf] = node_of(path)
            root_paths[leaf] = "::".join(path)
# #[macro_export] macros land at the crate root.
for p in files:
    s = open(p).read()
    for m in re.finditer(r"#\[macro_export\]\s*macro_rules!\s*(\w+)", s):
        root_names[m.group(1) + "!"] = node_of(module_path(p))

lines = defaultdict(int)
edges = defaultdict(lambda: {"main": 0, "test": 0, "sites": defaultdict(int), "refs": []})


inline_paths = {}  # `repository::X` -> canonical path, from the inline modules' `pub use`
for _name, _body in inline_mods.items():
    for _m in re.finditer(r"pub use\s+crate::([^;]+);", _body):
        for _path, _leaf in use_tree_leaves([], _m.group(1)):
            inline_paths[_name + "::" + _leaf] = "::".join(_path)


def canonical(path):
    segs = path.split("::")
    if len(segs) >= 2 and "::".join(segs[:2]) in inline_paths:
        return "::".join([inline_paths["::".join(segs[:2])]] + segs[2:])
    if segs and segs[0] in root_paths and segs[0] not in top_modules:
        return "::".join([root_paths[segs[0]]] + segs[1:])
    return path


def add_edge(src, dst, kind, site, line=0, path=""):
    if src == dst or dst is None:
        return
    path = canonical(path)
    e = edges[(src, dst)]
    e[kind] += 1
    e["sites"][site] += 1
    if kind == "main":
        e["refs"].append(f"{site}:{line} {path}")


def resolve(segs, cur_mod):
    """Resolve a path (list of segments after `crate`) to a node."""
    if not segs:
        return None
    head = segs[0]
    if head in top_modules:
        return node_of(segs)
    if head in inline_mods:
        return "<root>::" + head
    if head in root_names:
        return root_names[head]
    if head + "!" in root_names:
        return root_names[head + "!"]
    return "<root>"


def scan(code, cur_mod, src_node, kind, site):
    # use trees: `use crate::...;` and `pub use crate::...;`
    for m in re.finditer(r"\buse\s+(crate|super(?:::super)*|self)\s*::\s*([^;]+);", code):
        base = m.group(1)
        if base == "crate":
            prefix = []
        elif base == "self":
            prefix = list(cur_mod)
        else:
            ups = base.count("super")
            prefix = list(cur_mod)[: max(0, len(cur_mod) - ups)]
        ln = code.count("\n", 0, m.start()) + 1
        for path, _ in use_tree_leaves(prefix, m.group(2)):
            add_edge(src_node, resolve(path, cur_mod), kind, site, ln, "::".join(path))
    code_wo_use = re.sub(r"\buse\s+[^;]+;", lambda m: "\n" * m.group(0).count("\n"), code)
    for m in re.finditer(r"\bcrate::((?:\w+::)*\w+)", code_wo_use):
        ln = code_wo_use.count("\n", 0, m.start()) + 1
        add_edge(src_node, resolve(m.group(1).split("::"), cur_mod), kind, site, ln, m.group(1))
    for m in re.finditer(r"\b((?:super::)+)((?:\w+::)*\w+)", code_wo_use):
        ups = m.group(1).count("super")
        prefix = list(cur_mod)[: max(0, len(cur_mod) - ups)]
        ln = code_wo_use.count("\n", 0, m.start()) + 1
        path = prefix + m.group(2).split("::")
        add_edge(src_node, resolve(path, cur_mod), kind, site, ln, "::".join(path))
    # Crate-root macros used unqualified.
    for name, node in root_names.items():
        if name.endswith("!") and re.search(r"\b" + re.escape(name[:-1]) + r"!", code_wo_use):
            add_edge(src_node, node, kind, site)


for p in sorted(files):
    raw = open(p).read()
    mp = module_path(p)
    # A file's own module path: for mod.rs / lib.rs it is the directory module.
    src_node = node_of(mp)
    rel = os.path.relpath(p, SRC)
    is_test_file = bool(re.search(r"(_tests?|tests)\.rs$", p)) and "#[cfg(test)]" not in raw[:0]
    code = strip_comments_and_strings(raw)
    if rel == "lib.rs":
        # The inline modules are nodes; lib.rs itself only re-exports.
        for name, body in inline_mods.items():
            n = "<root>::" + name
            lines[n] += body.count("\n")
            scan(body, [name], n, "main", rel)
        lines["<root>"] += raw.count("\n") - sum(b.count("\n") for b in inline_mods.values())
        continue
    lines[src_node] += raw.count("\n")
    main, test = split_test_blocks(code)
    # A cfg(test) file (`#[cfg(test)] mod foo_tests;` in the parent) is all test code.
    parent_dir = os.path.dirname(p)
    stem = os.path.basename(p)[:-3]
    parent_candidates = [os.path.join(parent_dir, "mod.rs"), parent_dir + ".rs", os.path.join(SRC, "lib.rs")]
    cfg_test_file = False
    for pc in parent_candidates:
        if os.path.exists(pc) and re.search(r"#\[cfg\(test\)\]\s*(pub(\([^)]*\))?\s+)?mod\s+" + stem + r"\s*;", open(pc).read()):
            cfg_test_file = True
    if cfg_test_file:
        main, test = "", code
    scan(main, mp, src_node, "main", rel)
    scan(test, mp, src_node, "test", rel)

nodes = sorted(set(lines) | {a for a, _ in edges} | {b for _, b in edges})


def sccs(kind):
    adj = defaultdict(set)
    for (a, b), e in edges.items():
        if e[kind] or (kind == "all" and (e["main"] or e["test"])):
            adj[a].add(b)
    index, low, on, stack, out = {}, {}, set(), [], []
    counter = [0]
    sys.setrecursionlimit(10000)

    def strong(v):
        index[v] = low[v] = counter[0]
        counter[0] += 1
        stack.append(v)
        on.add(v)
        for w in adj[v]:
            if w not in index:
                strong(w)
                low[v] = min(low[v], low[w])
            elif w in on:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            comp = []
            while True:
                w = stack.pop()
                on.discard(w)
                comp.append(w)
                if w == v:
                    break
            out.append(sorted(comp))

    for v in nodes:
        if v not in index:
            strong(v)
    return out


main_sccs = [c for c in sccs("main") if len(c) > 1]

if "json" in opts:
    json.dump(
        {
            "lines": lines,
            "edges": [
                {"from": a, "to": b, "main": e["main"], "test": e["test"], "sites": dict(e["sites"]), "refs": e["refs"]}
                for (a, b), e in sorted(edges.items())
            ],
            "sccs": main_sccs,
        },
        open(opts["json"], "w"),
        indent=1,
    )

if "dot" in opts:
    with open(opts["dot"], "w") as f:
        f.write("digraph fc_platform {\n  rankdir=LR; node [shape=box, fontsize=10];\n")
        for (a, b), e in sorted(edges.items()):
            if e["main"]:
                f.write(f'  "{a}" -> "{b}" [label="{e["main"]}"];\n')
        f.write("}\n")

print(f"# Module graph of {SRC}\n")
print(f"{len(nodes)} nodes, {sum(1 for e in edges.values() if e['main'])} non-test edges, "
      f"{sum(lines.values())} lines\n")
print("## Lines per node")
for n in sorted(lines, key=lambda x: -lines[x]):
    print(f"{lines[n]:>7}  {n}")
print("\n## Non-trivial strongly connected components (non-test edges)")
for c in main_sccs:
    print(f"- {len(c)} nodes, {sum(lines[x] for x in c)} lines: {', '.join(c)}")
print("\n## Edges (non-test), from -> to: refs [test refs] (top sites)")
for (a, b), e in sorted(edges.items(), key=lambda kv: (kv[0][0], -kv[1]["main"])):
    if not e["main"]:
        continue
    sites = sorted(e["sites"].items(), key=lambda kv: -kv[1])[:3]
    print(f"{a} -> {b}: {e['main']} [{e['test']}]  " + ", ".join(f"{s}({k})" for s, k in sites))
if "edges-for" in opts:
    want = set(opts["edges-for"].split(","))
    print("\n## Edges between", ", ".join(sorted(want)))
    for (a, b), e in sorted(edges.items()):
        if a in want and b in want and e["main"]:
            print(f"{a} -> {b}: {e['main']}  " + ", ".join(f"{s}({k})" for s, k in sorted(e["sites"].items(), key=lambda kv: -kv[1])[:6]))

#!/usr/bin/env python3
"""Rewrite `crate::` paths after files move between the fc-platform crates.

The split (docs/plans/build-speed-2026-09-28.md, section 6) keeps every
module at its old path inside its new crate: `principal/api.rs` is
`crate::principal::api` in fc-platform-iam as it was in fc-platform. So all
the `crates/fc-platform*/src` trees together form one namespace, the old
crate's. A moved file's `crate::x::y` still names the same thing; what
changes is which crate owns it. This script rewrites each such path to
`fc_platform_<owner>::x::y` when the owner is another crate, and to a path
that resolves inside the file's own crate otherwise (a root re-export such
as `crate::Principal` becomes `crate::principal::entity::Principal` when
the crate has no such re-export).

  split_rewrite.py [--apply] [--only <crate>...] [--files <glob>...]

Resolution: every `pub use` in every crate is an alias (`mfa::MfaRepository`
-> `mfa::repository::MfaRepository`; `role::entity::permissions` ->
`permissions` in fc-platform-core); a path is followed through the aliases
to where its item is defined; the owner is the crate whose file defines
the longest module prefix, or, for a module split across crates (its
`mod.rs` in several), the crate whose copy defines the next segment.

`super::` paths are not touched (a file that moves without its parent
module needs them checked by hand). `pub(in crate::…)` and `$crate` are left
alone.
"""
import argparse
import os
import re
import sys
from collections import defaultdict

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
CRATES_DIR = os.path.join(ROOT, "crates")
IDENT = r"[A-Za-z_][A-Za-z0-9_]*"


def crate_dirs():
    out = {}
    for d in sorted(os.listdir(CRATES_DIR)):
        if d == "fc-platform" or d.startswith("fc-platform-"):
            if d == "fc-platform-jwks":
                continue
            src = os.path.join(CRATES_DIR, d, "src")
            if os.path.isdir(src):
                out[d.replace("-", "_")] = src
    return out


def module_of(rel):
    """Module path of a file relative to src/ ('' for lib.rs)."""
    rel = rel[:-3]
    parts = rel.split("/")
    if parts[-1] in ("mod", "lib", "main"):
        parts = parts[:-1]
    return "::".join(parts)


def strip_comments_keep_len(s):
    """Blank out comments (so definitions in comments don't count); keep offsets."""
    out = list(s)
    i, n = 0, len(s)
    while i < n:
        if s.startswith("//", i):
            j = s.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif s.startswith("/*", i):
            j = s.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif s[i] == '"':
            j = i + 1
            while j < n and s[j] != '"':
                if s[j] == "\\":
                    j += 1
                j += 1
            i = j + 1
        else:
            i += 1
    return "".join(out)


# ── use-tree parsing ─────────────────────────────────────────────────────


def parse_tree(s, i=0):
    """Parse a use tree starting at s[i]. Returns (list of leaves, end).

    A leaf is (path segments, alias or None); `self` and `*` are segments.
    """
    leaves = []
    segs = []
    n = len(s)

    def skip_ws(j):
        while j < n and s[j] in " \t\r\n":
            j += 1
        return j

    i = skip_ws(i)
    while True:
        i = skip_ws(i)
        if i < n and s[i] == "{":
            i += 1
            while True:
                i = skip_ws(i)
                if i < n and s[i] == "}":
                    i += 1
                    break
                sub, i = parse_tree(s, i)
                for p, a in sub:
                    leaves.append((segs + p, a))
                i = skip_ws(i)
                if i < n and s[i] == ",":
                    i += 1
            return leaves, i
        if i < n and s[i] == "*":
            leaves.append((segs + ["*"], None))
            return leaves, i + 1
        m = re.compile(IDENT).match(s, i)
        if not m:
            return leaves, i
        segs.append(m.group(0))
        i = m.end()
        j = skip_ws(i)
        if s.startswith("::", j):
            i = j + 2
            continue
        # alias?
        m2 = re.compile(r"\s+as\s+(" + IDENT + ")").match(s, i)
        if m2:
            leaves.append((segs, m2.group(1)))
            return leaves, m2.end()
        leaves.append((segs, None))
        return leaves, i


def render_tree(leaves):
    """Render leaves (segments, alias) as one tree, nesting shared prefixes:
    `a::{b::{C, D}, E as F}`."""

    def build(items):
        # items: list of (segs, alias); group by first segment
        order, groups, ends = [], {}, []
        for segs, alias in items:
            if len(segs) == 1:
                ends.append((segs[0], alias))
                continue
            head = segs[0]
            if head not in groups:
                groups[head] = []
                order.append(head)
            groups[head].append((segs[1:], alias))
        parts = []
        for name, alias in ends:
            parts.append(f"{name} as {alias}" if alias else name)
        for head in order:
            sub = groups[head]
            inner = build(sub)
            if len(inner) == 1:
                parts.append(f"{head}::{inner[0]}")
            else:
                parts.append(f"{head}::{{{', '.join(inner)}}}")
        return parts

    parts = build(leaves)
    if len(parts) == 1:
        return parts[0]
    return "{" + ", ".join(parts) + "}"


def match_brace(code, open_i):
    depth = 0
    for j in range(open_i, len(code)):
        ch = code[j]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return j
    return len(code)


def inline_modules(code, mod):
    """[(start, end, module path)] for every inline `mod x { … }`."""
    out = []
    for m in re.finditer(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+(" + IDENT + r")\s*\{", code):
        out.append((m.end() - 1, match_brace(code, m.end() - 1), m.group(1)))
    res = []
    for st, en, name in out:
        parents = [n for (s2, e2, n) in out if s2 < st and en <= e2]
        path = "::".join(([mod] if mod else []) + parents + [name])
        res.append((st, en, path))
    return res


def module_at(ranges, mod, pos):
    best = None
    for st, en, path in ranges:
        if st <= pos <= en and (best is None or st > best[0]):
            best = (st, path)
    return best[1] if best else mod


# ── the namespace ────────────────────────────────────────────────────────


class Namespace:
    def __init__(self):
        self.crates = crate_dirs()
        # module path -> set of crates that have a file (or inline mod) for it
        self.module_crates = defaultdict(set)
        # (crate, module, rel) -> file content
        self.files = {}
        # (crate, module) -> names defined there (not re-exported)
        self.real = defaultdict(set)
        # (crate, module, name) -> target V-path (list of segments)
        self.aliases = {}
        # (crate, module) -> list of target module V-paths (`pub use x::*`)
        self.globs = defaultdict(list)
        for c, src in self.crates.items():
            for dp, _, fs in os.walk(src):
                for f in fs:
                    if not f.endswith(".rs"):
                        continue
                    p = os.path.join(dp, f)
                    rel = os.path.relpath(p, src).replace(os.sep, "/")
                    mod = module_of(rel)
                    self.files[(c, mod, rel)] = open(p).read()
                    self.module_crates[mod].add(c)
        codes = {k: strip_comments_keep_len(v) for k, v in self.files.items()}
        for (c, mod, rel), code in codes.items():
            self._scan_defs(c, mod, code)
        for (c, mod, rel), code in codes.items():
            self._scan_uses(c, mod, code)

    def _scan_defs(self, c, filemod, code):
        ranges = inline_modules(code, filemod)
        for st, en, path in ranges:
            self.module_crates[path].add(c)
        for m in re.finditer(
            r"(?m)^[ \t]*(?:#\[[^\]]*\][ \t]*)?(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|const\s+|unsafe\s+)*"
            r"(?:struct|enum|trait|fn|mod|const|static|type|union)\s+(" + IDENT + ")",
            code,
        ):
            mod = module_at(ranges, filemod, m.start())
            self.real[(c, mod)].add(m.group(1))
        for m in re.finditer(r"macro_rules!\s+(" + IDENT + ")", code):
            self.real[(c, module_at(ranges, filemod, m.start()))].add(m.group(1))
            # #[macro_export] macros live at the crate root
            self.real[(c, "")].add(m.group(1))

    def _scan_uses(self, c, filemod, code):
        ranges = inline_modules(code, filemod)
        for m in re.finditer(r"(?m)^[ \t]*pub(?:\([^)]*\))?\s+use\s+", code):
            mod = module_at(ranges, filemod, m.start())
            leaves, end = parse_tree(code, m.end())
            for segs, alias in leaves:
                target = self._resolve_use_path(c, mod, segs)
                if target is None:
                    continue
                if segs[-1] == "*":
                    self.globs[(c, mod)].append(target[:-1])
                    continue
                if segs[-1] == "self":
                    target = target[:-1]
                    name = alias or segs[-2]
                else:
                    name = alias or segs[-1]
                self.aliases.setdefault((c, mod, name), target)

    def _resolve_use_path(self, c, mod, segs):
        """A `use` path in module `mod` of crate `c` as a V-path, or None if
        it leaves the platform (another crate, std)."""
        if not segs:
            return None
        head = segs[0]
        if head == "crate":
            return segs[1:]
        if head in self.crates:
            return segs[1:]
        if head == "self":
            return (mod.split("::") if mod else []) + segs[1:]
        if head == "super":
            parts = mod.split("::") if mod else []
            k = 0
            while k < len(segs) and segs[k] == "super":
                parts = parts[:-1]
                k += 1
            return parts + segs[k:]
        sub = f"{mod}::{head}" if mod else head
        if sub in self.module_crates or head in self.real[(c, mod)]:
            return (mod.split("::") if mod else []) + segs
        return None

    def _split_prefix(self, vpath):
        """(k, mod): the longest prefix of vpath that is a module."""
        for k in range(len(vpath), -1, -1):
            mod = "::".join(vpath[:k])
            if mod in self.module_crates:
                return k, mod
        return 0, ""

    def canon(self, vpath, depth=0):
        """Follow `pub use` aliases until the path names a definition."""
        if depth > 25 or not vpath:
            return vpath
        k, mod = self._split_prefix(vpath)
        if k == len(vpath):
            return vpath
        name = vpath[k]
        crates = sorted(self.module_crates.get(mod, set()))
        if any(name in self.real[(c, mod)] for c in crates):
            return vpath
        for c in crates:
            key = (c, mod, name)
            if key in self.aliases:
                return self.canon(self.aliases[key] + vpath[k + 1 :], depth + 1)
        for c in crates:
            for g in self.globs.get((c, mod), []):
                cand = self.canon(g + vpath[k:], depth + 1)
                if cand != g + vpath[k:] or self.owner(cand) is not None:
                    return cand
        return vpath

    def owner(self, vpath):
        """The crate that defines what a canonical V-path names."""
        k, mod = self._split_prefix(vpath)
        crates = self.module_crates.get(mod, set())
        if len(crates) == 1:
            return next(iter(crates))
        if k == len(vpath):
            return None
        name = vpath[k]
        hits = [c for c in crates if name in self.real[(c, mod)]]
        if len(hits) == 1:
            return hits[0]
        return None

    def resolves_in(self, c, vpath):
        """Whether `crate::<vpath>` resolves in crate c (its own modules,
        definitions and re-exports)."""
        mod = ""
        if c not in self.module_crates.get("", set()):
            return False
        for i, s in enumerate(vpath):
            sub = f"{mod}::{s}" if mod else s
            if c in self.module_crates.get(sub, set()):
                mod = sub
                continue
            if s in self.real[(c, mod)] or (c, mod, s) in self.aliases:
                return True
            if self.globs.get((c, mod)):
                return True
            return False
        return True


# `crate::…` or a platform crate's `fc_platform_<x>::…` (not the assembly's
# `fc_platform::`, whose paths are its facades').
PREFIX = r"(crate|fc_platform_[a-z_]+)"
PATH_RE = re.compile(r"(?<![\w$])" + PREFIX + r"::((?:" + IDENT + r"::)*)(" + IDENT + r"|\{)")
USE_RE = re.compile(r"(?m)^([ \t]*)((?:pub(?:\([^)]*\))?\s+)?)use\s+" + PREFIX + "::")


def rewrite_file(ns, c, content, stats):
    out = content

    # 1. `use crate::…;` statements (trees included)
    def fix_use(text):
        res = []
        pos = 0
        for m in USE_RE.finditer(text):
            if m.start() < pos:
                continue
            start = m.start()
            orig = m.group(3)
            if orig != "crate" and orig not in ns.crates:
                continue
            tree_start = m.end() - len(orig + "::")
            leaves, end = parse_tree(text, tree_start)
            semi = text.find(";", end)
            if semi < 0 or text[end:semi].strip():
                continue
            groups = defaultdict(list)
            changed = False
            for segs, alias in leaves:
                assert segs[0] == orig, segs
                v = segs[1:]
                prefix, newv = map_path(ns, c, v, orig)
                if prefix != orig or newv != v:
                    changed = True
                groups[prefix].append((newv, alias))
            if not changed:
                continue
            indent, vis = m.group(1), m.group(2)
            stmts = []
            for prefix in sorted(groups, key=lambda p: (p != "crate", p)):
                stmts.append(f"{indent}{vis}use {prefix}::{render_tree(groups[prefix])};")
            res.append(text[pos:start])
            res.append("\n".join(stmts))
            pos = semi + 1
            stats["use"] += 1
        res.append(text[pos:])
        return "".join(res)

    out = fix_use(out)

    # 2. every other `crate::a::b::C` path (expressions, types, attributes,
    #    strings, doc links)
    def fix_path(m):
        orig = m.group(1)
        if orig != "crate" and orig not in ns.crates:
            return m.group(0)
        if m.group(3) == "{":
            return m.group(0)
        before = out_src[max(0, m.start() - 4) : m.start()]
        if before.endswith("in "):
            return m.group(0)
        segs = [s for s in m.group(2).split("::") if s] + [m.group(3)]
        prefix, newv = map_path(ns, c, segs, orig)
        if prefix == orig and newv == segs:
            return m.group(0)
        stats["path"] += 1
        return f"{prefix}::" + "::".join(newv)

    out_src = out
    out = PATH_RE.sub(fix_path, out)
    return out


def map_path(ns, c, v, orig="crate"):
    """(prefix, path) a V-path is written as from crate c (`orig`: the prefix
    it is written with now)."""
    canon = ns.canon(v)
    owner = ns.owner(canon)
    if owner is None:
        owner = ns.owner(v)
    if orig != "crate" and (owner is None or orig == c):
        # unknown, or the crate naming itself (a doc example): leave it
        return orig, v
    if owner is None or owner == c:
        if ns.resolves_in(c, v):
            return "crate", v
        return "crate", canon
    # another crate: its own text path if that resolves there, else canonical
    if ns.resolves_in(owner, v) and ns.owner(ns.canon(v)) == owner and not v[0][:1].isupper():
        return owner, v
    return owner, canon


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true")
    ap.add_argument("--only", nargs="*", default=None, help="crates to rewrite (fc_platform_x)")
    ap.add_argument("--files", nargs="*", default=None, help="only these files (paths)")
    ap.add_argument("--explain", nargs="*", default=None, help="print how V-paths map")
    args = ap.parse_args()
    ns = Namespace()
    if args.explain:
        for p in args.explain:
            v = p.split("::")
            canon = ns.canon(v)
            print(p, "->", "::".join(canon), "owner", ns.owner(canon))
        return
    total = defaultdict(int)
    files_changed = 0
    for (c, mod, rel), content in sorted(ns.files.items()):
        if args.only is not None and c not in args.only:
            continue
        path = os.path.join(ns.crates[c], rel)
        if args.files is not None and not any(os.path.abspath(path) == os.path.abspath(f) for f in args.files):
            continue
        stats = defaultdict(int)
        new = rewrite_file(ns, c, content, stats)
        if new != content:
            files_changed += 1
            for k, v in stats.items():
                total[k] += v
            if args.apply:
                open(path, "w").write(new)
            else:
                print(f"{c}: {rel}: {dict(stats)}")
    print(f"files changed: {files_changed}, {dict(total)}", file=sys.stderr)


if __name__ == "__main__":
    main()

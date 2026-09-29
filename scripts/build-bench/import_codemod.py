#!/usr/bin/env python3
"""Replace inline absolute paths with `use` imports (CLAUDE.md, "Imports").

  cargo clippy --workspace --all-targets --message-format=json \
      > target/absolute-paths.json        # with clippy::absolute_paths on
  scripts/build-bench/import_codemod.py target/absolute-paths.json [--apply]

Reads clippy's `absolute_paths` warnings and, for each flagged path, adds a
`use` to the innermost module the path is in and shortens the path:

- a type, trait or constant (the first capitalised segment) is imported by
  name: `std::sync::Arc<T>` -> `Arc<T>`, `crate::x::Enum::Variant` ->
  `Enum::Variant`;
- a function or other lowercase item is written through its module:
  `std::mem::take(x)` -> `mem::take(x)` (`use std::mem;`);
- a name that is already bound in that module to something else (another
  import, an item defined there), or one of the usual clashes (`Result`,
  `Error`, `Duration`, …), is written through its parent module instead
  (`fmt::Result`); if that clashes too, it is left for a person.

Skipped: paths inside `routes!(…)` and files that allow the lint (the
route-auth scanner reads their handler paths). A path clippy reports twice
(two targets compile the file) is rewritten once.
"""
import json
import os
import re
import sys
from collections import defaultdict

sys.dont_write_bytecode = True
ROOT = os.path.realpath(os.path.join(os.path.dirname(__file__), "..", ".."))
IDENT = r"[A-Za-z_][A-Za-z0-9_]*"
# Names imported through their parent module rather than bare: the usual
# clashes between std, the crates and our own aliases.
AMBIGUOUS = {"Result", "Error"}
# The std prelude: always bound. A path to the prelude's own item needs no
# import at all.
PRELUDE = {
    "Result": "std::result::Result", "Option": "std::option::Option",
    "Vec": "std::vec::Vec", "String": "std::string::String", "Box": "std::boxed::Box",
    "Ok": "std::result::Result::Ok", "Err": "std::result::Result::Err",
    "Some": "std::option::Option::Some", "None": "std::option::Option::None",
    "ToString": "std::string::ToString", "ToOwned": "std::borrow::ToOwned",
    "Clone": "std::clone::Clone", "Copy": "std::marker::Copy", "Send": "std::marker::Send",
    "Sync": "std::marker::Sync", "Sized": "std::marker::Sized", "Default": "std::default::Default",
    "Drop": "std::ops::Drop", "Fn": "std::ops::Fn", "FnMut": "std::ops::FnMut",
    "FnOnce": "std::ops::FnOnce", "Iterator": "std::iter::Iterator",
    "IntoIterator": "std::iter::IntoIterator", "Extend": "std::iter::Extend",
    "PartialEq": "std::cmp::PartialEq", "Eq": "std::cmp::Eq", "PartialOrd": "std::cmp::PartialOrd",
    "Ord": "std::cmp::Ord", "AsRef": "std::convert::AsRef", "AsMut": "std::convert::AsMut",
    "Into": "std::convert::Into", "From": "std::convert::From", "TryFrom": "std::convert::TryFrom",
    "TryInto": "std::convert::TryInto", "FromIterator": "std::iter::FromIterator",
    "Unpin": "std::marker::Unpin", "Debug": None, "Hash": None,
}


def load_diagnostics(path):
    """(file, byte_start, byte_end, text) for every absolute_paths warning."""
    seen = set()
    out = []
    for line in open(path):
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") != "compiler-message":
            continue
        msg = m["message"]
        code = (msg.get("code") or {}).get("code")
        if code != "clippy::absolute_paths":
            continue
        for sp in msg["spans"]:
            if not sp.get("is_primary"):
                continue
            f = sp["file_name"]
            if not os.path.isabs(f):
                f = os.path.join(ROOT, f)
            # one file reached through two paths (`#[path = "../…"]`)
            f = os.path.realpath(f)
            key = (f, sp["byte_start"], sp["byte_end"])
            if key in seen:
                continue
            seen.add(key)
            out.append((f, sp["byte_start"], sp["byte_end"]))
    return out


def split_path(text):
    """Segments of a path, keeping `::<…>` generics on their segment."""
    segs = []
    i = 0
    cur = ""
    depth = 0
    while i < len(text):
        if depth == 0 and text.startswith("::", i):
            if text.startswith("::<", i):
                cur += "::"
                i += 2
                continue
            segs.append(cur)
            cur = ""
            i += 2
            continue
        ch = text[i]
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
        cur += ch
        i += 1
    segs.append(cur)
    return [s.strip() for s in segs]


def norm(x):
    return re.sub(r"\s+", "", x).replace("core::", "std::")


def lead_keep(rest):
    return "::".join(rest)


def base(seg):
    return re.match(IDENT, seg).group(0) if re.match(IDENT, seg) else seg


def match_brace(code, i):
    depth = 0
    for j in range(i, len(code)):
        if code[j] == "{":
            depth += 1
        elif code[j] == "}":
            depth -= 1
            if depth == 0:
                return j
    return len(code)


def blank_comments_strings(s):
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
        elif s[i] == '"' and not (i > 0 and s[i - 1] == "'"):
            j = i + 1
            while j < n and s[j] != '"':
                if s[j] == "\\":
                    j += 1
                j += 1
            for k in range(i + 1, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j + 1
        else:
            i += 1
    return "".join(out)


class Module:
    """A module's scope in a file: the whole file, or an inline `mod x { }`."""

    def __init__(self, start, end, body_start):
        self.start, self.end, self.body_start = start, end, body_start
        self.children = []


def modules_of(code):
    root = Module(0, len(code), 0)
    stack = [root]
    for m in re.finditer(r"(?m)^[ \t]*(?:#\[[^\n]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+" + IDENT + r"\s*\{", code):
        o = m.end() - 1
        mod = Module(m.start(), match_brace(code, o), o + 1)
        # parent: the innermost module containing it
        parent = root
        stack = [root]
        while True:
            for c in parent.children:
                if c.start <= mod.start and mod.end <= c.end:
                    parent = c
                    break
            else:
                break
        parent.children.append(mod)
    return root


def innermost(root, pos):
    m = root
    while True:
        for c in m.children:
            if c.start <= pos <= c.end:
                m = c
                break
        else:
            return m


def own_region(mod, code):
    """The module's own text, children blanked out."""
    s = list(code[mod.body_start : mod.end])
    for c in mod.children:
        for k in range(c.start - mod.body_start, c.end - mod.body_start + 1):
            if 0 <= k < len(s) and s[k] != "\n":
                s[k] = " "
    return "".join(s)


def _parse_tree():
    import importlib.util

    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location("split_rewrite", os.path.join(here, "split_rewrite.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.parse_tree


parse_tree = _parse_tree()


def bound_names(region, indent):
    """Names a module binds in the type namespace (where a module or type
    import could clash): its `use` leaves (and aliases), and the types,
    traits and modules it defines. `indent` is the module's own item
    indentation, so nested items (impl methods, inner modules) don't count."""
    names = {}
    ind = re.escape(indent)
    for m in re.finditer(r"(?m)^" + ind + r"(?:pub(?:\([^)]*\))?\s+)?use\s+", region):
        leaves, _ = parse_tree(region, m.end())
        for segs, alias in leaves:
            if not segs or segs[-1] == "*":
                continue
            if alias:
                names[alias] = "::".join(segs)
                continue
            if segs[-1] == "self":
                if len(segs) >= 2:
                    names[segs[-2]] = "::".join(segs[:-1])
                continue
            names[segs[-1]] = "::".join(segs)
    for m in re.finditer(
        r"(?m)^" + ind + r"(?:#\[[^\n]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|mod|type|union)\s+(" + IDENT + ")",
        region,
    ):
        names.setdefault(m.group(1), "<item>")
    return names


def bound_values(code, mod, root):
    """Value-namespace names a module defines itself (fns, consts, statics)."""
    region = own_region(mod, blank_comments_strings(code))
    ind = re.escape(module_indent(code, mod, root))
    return set(
        m.group(1)
        for m in re.finditer(
            r"(?m)^" + ind + r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|const\s+|unsafe\s+)*(?:fn|const|static)\s+(" + IDENT + ")",
            region,
        )
    )


def module_indent(code, mod, root):
    if mod is root:
        return ""
    line_start = code.rfind("\n", 0, mod.start) + 1
    return re.match(r"[ \t]*", code[line_start:]).group(0) + "    "


def cfg_at(code, mod, root, pos):
    """The `#[cfg(…)]` of the module-level item `pos` is in, if any."""
    blank = blank_comments_strings(code)
    region = own_region(mod, blank)
    ind = re.escape(module_indent(code, mod, root))
    rel = pos - mod.body_start
    for m in re.finditer(r"(?m)^" + ind + r"#\[cfg\((.*)\)\][ \t]*\n", region):
        j = m.end()
        brace = region.find("{", j)
        semi = region.find(";", j)
        if semi != -1 and (brace == -1 or semi < brace):
            end = semi
        else:
            end = match_brace(region, brace)
        if m.start() <= rel <= end:
            # the attribute's text from the source (strings are blanked above)
            a, b = mod.body_start + m.start(1), mod.body_start + m.end(1)
            return code[a:b].strip()
    return None


def insert_point(code, mod, root):
    """Where a new `use` goes: after the module's last module-level `use`
    (at the module's own indentation, not one inside a function), else
    after its inner attributes and doc comments."""
    region = own_region(mod, code)
    ind = re.escape(module_indent(code, mod, root))
    last = None
    for m in re.finditer(r"(?m)^" + ind + r"(?:pub(?:\([^)]*\))?\s+)?use\s+", region):
        # the statement's end: its `;` at depth 0
        depth = 0
        j = m.end()
        while j < len(region):
            ch = region[j]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            elif ch == ";" and depth == 0:
                break
            j += 1
        nl = region.find("\n", j)
        last = (nl + 1) if nl >= 0 else len(region)
    if last is not None:
        return mod.body_start + last, False
    pos = mod.body_start
    for m in re.finditer(r"(?m)^[ \t]*(//![^\n]*|#!\[[^\n]*\]|)\n", region):
        if m.start() != pos - mod.body_start:
            break
        pos = mod.body_start + m.end()
    return pos, True


def in_routes_macro(code, pos):
    i = code.rfind("routes!(", 0, pos)
    if i < 0:
        return False
    depth = 0
    for j in range(i + len("routes!"), len(code)):
        if code[j] == "(":
            depth += 1
        elif code[j] == ")":
            depth -= 1
            if depth == 0:
                return pos < j
    return False


def plan_file(path, spans):
    raw = open(path, "rb").read()
    code = raw.decode()
    if "allow(clippy::absolute_paths" in code:
        return None, []
    # byte offsets -> str offsets
    bmap = {}
    acc = 0
    for idx, ch in enumerate(code):
        bmap[acc] = idx
        acc += len(ch.encode())
    bmap[acc] = len(code)
    root = modules_of(blank_comments_strings(code))
    edits = []  # (start, end, replacement)
    imports = defaultdict(dict)  # module -> {name: full}
    cfgs = defaultdict(set)  # (module, name) -> the cfgs of the items using it
    skipped = []
    bound_cache = {}
    for bs, be in sorted(spans):
        s, e = bmap[bs], bmap[be]
        text = code[s:e]
        if in_routes_macro(code, s):
            continue
        segs = split_path(text)
        if len(segs) < 3:
            skipped.append((text, "short"))
            continue
        lead = ""
        if segs[0] == "":
            lead = "::"
            segs = segs[1:]
        mod = innermost(root, s)
        if id(mod) not in bound_cache:
            bound_cache[id(mod)] = bound_names(
                own_region(mod, blank_comments_strings(code)), module_indent(code, mod, root)
            )
        bound = bound_cache[id(mod)]
        if ("roots", id(mod)) not in bound_cache:
            own = own_region(mod, blank_comments_strings(code))
            roots = set(re.findall(r"(?<![\w:])([a-z_][a-z0-9_]*)\s*::", own))
            roots -= {"crate", "self", "super", "std", "core", "alloc"}
            bound_cache[("roots", id(mod))] = {r for r in roots if r not in bound}
        roots = bound_cache[("roots", id(mod))]
        pending = imports[id(mod)]
        cap = next((i for i, sg in enumerate(segs) if i > 0 and base(sg)[:1].isupper()), None)

        def fits(name, full):
            if name in roots and name != base(full.split("::")[0]):
                # an unimported path root (an extern crate) of that name
                return False
            have = bound.get(name) or pending.get(name)
            if have is None:
                if name in PRELUDE:
                    # shadowing the prelude: only by the prelude's own item
                    return PRELUDE[name] is not None and norm(PRELUDE[name]) == norm(full)
                return True
            return norm(have) == norm(full)

        choice = None
        if cap is not None:
            name = base(segs[cap])
            full = "::".join(segs[:cap]) + "::" + name
            is_const = name.isupper() and len(name) > 1
            if not is_const and name not in AMBIGUOUS and fits(name, full):
                choice = (name, full, cap)
        if choice is None and "permissions" in segs[1:]:
            # the permission catalogue reads best as `permissions::area::X`
            k = segs.index("permissions", 1)
            full = "::".join(segs[: k + 1])
            if fits("permissions", full):
                choice = ("permissions", full, k)
        if choice is None:
            # through the parent module of the item
            k = cap if cap is not None else len(segs) - 1
            if k >= 2:
                parent = base(segs[k - 1])
                full = "::".join(segs[:k])
                if parent not in ("self", "super", "crate") and fits(parent, full):
                    choice = (parent, full, k - 1)
        if choice is None:
            # the item itself, when its name is free (a clashing module name,
            # `Result` / `Error` whose module name is taken)
            k = cap if cap is not None else len(segs) - 1
            name = base(segs[k])
            full = "::".join(segs[:k]) + "::" + name
            if fits(name, full) and name not in bound_values(code, mod, root):
                choice = (name, full, k)
        if choice is None:
            skipped.append((text, "clash"))
            continue
        name, full, idx = choice
        # replace only the path's prefix, up to the chosen segment's name:
        # generic arguments after it (which may hold paths of their own,
        # rewritten separately) are left as they are
        pat = r"\s*(?:::\s*)?" + r"\s*::\s*".join(re.escape(base(sg)) for sg in (segs[:idx + 1]))
        pm = re.match(pat, text)
        if not pm:
            skipped.append((text, "unparsed"))
            continue
        pending.setdefault(name, full)
        cfgs[(id(mod), name)].add(cfg_at(code, mod, root, s))
        edits.append((s, s + pm.end(), name))
    if not edits:
        return None, skipped
    # imports per module
    ins = []
    for mod_id, names in imports.items():
        mod = None
        stack = [root]
        while stack:
            m = stack.pop()
            if id(m) == mod_id:
                mod = m
                break
            stack.extend(m.children)
        bound = bound_cache[mod_id]
        lines = []
        for name, full in sorted(names.items(), key=lambda kv: kv[1]):
            if name in bound:
                continue  # already imported (same path)
            used_under = cfgs[(mod_id, name)]
            if None in used_under or not used_under:
                lines.append(f"use {full};")
            elif len(used_under) == 1:
                lines.append(f"#[cfg({next(iter(used_under))})]\nuse {full};")
            else:
                any_of = ", ".join(sorted(used_under))
                lines.append(f"#[cfg(any({any_of}))]\nuse {full};")
        if not lines:
            continue
        pos, bare = insert_point(code, mod, root)
        indent = ""
        if mod is not root:
            line_start = code.rfind("\n", 0, mod.start) + 1
            indent = re.match(r"[ \t]*", code[line_start:]).group(0) + "    "
        block = "".join(
            "".join(f"{indent}{part}\n" for part in l.split("\n")) for l in lines
        )
        if bare and mod is root:
            block = block + "\n"
        ins.append((pos, pos, block))
    new_code = code
    for s, e, rep in sorted(edits + ins, key=lambda t: (t[0], t[1]), reverse=True):
        new_code = new_code[:s] + rep + new_code[e:]
    return new_code, skipped


def main():
    diag = sys.argv[1]
    apply = "--apply" in sys.argv
    only = None
    if "--files" in sys.argv:
        only = {os.path.realpath(l.strip()) for l in open(sys.argv[sys.argv.index("--files") + 1]) if l.strip()}
    spans = defaultdict(list)
    for f, s, e in load_diagnostics(diag):
        spans[f].append((s, e))
    total = 0
    left = []
    for f, sp in sorted(spans.items()):
        if "/target/" in f or not f.startswith(ROOT):
            continue
        if only is not None and f not in only:
            continue
        new, skipped = plan_file(f, sp)
        left += [(f, t, why) for t, why in skipped]
        if new is None:
            continue
        total += 1
        if apply:
            open(f, "w").write(new)
    print(f"files rewritten: {total}; paths left: {len(left)}", file=sys.stderr)
    for f, t, why in left[:400]:
        print(f"left {os.path.relpath(f, ROOT)}: {t} ({why})")


if __name__ == "__main__":
    main()

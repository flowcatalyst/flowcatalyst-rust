#!/usr/bin/env python3
"""Move `use` lines the import codemod added below a file's first item up
into the file's top `use` block.

  scripts/build-bench/import_hoist.py <base-rev> [--apply]

The codemod appends a module's new imports after the module's last `use`;
in a file that also has a `use` further down (after a macro, say), they land
mid-file. For each changed `.rs` file, a module-level `use` statement (with
its `#[cfg]` line) that is not in the base revision and sits after the
file's first item moves to the end of the first `use` block.
"""
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = os.path.realpath(os.path.join(os.path.dirname(__file__), "..", ".."))
ITEM = re.compile(
    r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|const\s+|unsafe\s+|extern\s+)*"
    r"(?:fn|struct|enum|trait|impl|mod|const|static|type|union|macro_rules!)\b"
)


def statements(lines):
    """Module-level (column 0) statements: (start, end, text) line ranges."""
    out = []
    i = 0
    depth = 0
    while i < len(lines):
        line = lines[i]
        if depth == 0 and (line.startswith("use ") or line.startswith("pub use ") or line.startswith("pub(crate) use ")):
            j = i
            while not lines[j].rstrip().endswith(";"):
                j += 1
            start = i
            if i > 0 and lines[i - 1].startswith("#[cfg("):
                start = i - 1
            out.append(("use", start, j))
            i = j + 1
            continue
        if depth == 0 and ITEM.match(line):
            out.append(("item", i, i))
        depth += line.count("{") - line.count("}")
        i += 1
    return out


def main():
    base = sys.argv[1]
    apply = "--apply" in sys.argv
    files = subprocess.run(
        ["git", "diff", "--name-only", "--diff-filter=AMR", base, "--", "*.rs"],
        cwd=ROOT, capture_output=True, text=True,
    ).stdout.split()
    moved_files = 0
    for rel in files:
        path = os.path.join(ROOT, rel)
        if not os.path.isfile(path):
            continue
        try:
            before = subprocess.run(["git", "show", f"{base}:{rel}"], cwd=ROOT, capture_output=True, text=True).stdout
        except Exception:
            before = ""
        old_uses = {l.strip() for l in before.split("\n") if "use " in l}
        lines = open(path).read().split("\n")
        st = statements(lines)
        first_item = next((s for s in st if s[0] == "item"), None)
        if first_item is None:
            continue
        top_uses = [s for s in st if s[0] == "use" and s[2] < first_item[1]]
        if not top_uses:
            continue
        anchor = top_uses[-1][2]
        late = [s for s in st if s[0] == "use" and s[1] > first_item[1]]
        move = []
        for _, a, b in late:
            block = lines[a : b + 1]
            if all(l.strip() in old_uses or l.startswith("#[cfg(") for l in block):
                continue  # it was there before
            move.append((a, b))
        if not move:
            continue
        moved_files += 1
        if not apply:
            print(rel, len(move))
            continue
        take = []
        for a, b in sorted(move, reverse=True):
            take = lines[a : b + 1] + take
            del lines[a : b + 1]
            # a blank line left on its own twice: drop one
            if 0 < a < len(lines) and lines[a - 1] == "" and lines[a] == "":
                del lines[a]
        lines[anchor + 1 : anchor + 1] = take
        open(path, "w").write("\n".join(lines))
    print(f"files: {moved_files}", file=sys.stderr)


if __name__ == "__main__":
    main()

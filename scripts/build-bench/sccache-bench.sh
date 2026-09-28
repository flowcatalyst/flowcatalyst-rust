#!/usr/bin/env bash
# sccache across worktrees: does a second worktree reuse the dependencies the
# first one compiled?
#
#   scripts/build-bench/sccache-bench.sh [path/to/sccache]
#
#   1. cold-test in worktree target/sc-wt-a with an empty cache (populates it);
#   2. cold-test in worktree target/sc-wt-b with the cache from 1;
#   3. test-one x3 in sc-wt-b (the incremental loop through sccache);
#   4. sccache stats after each step -> target/build-bench/sccache-*.txt.
#
# Real git worktrees (detached at HEAD, with this tree's .cargo/config.toml
# copied in), each with its own default target/, built by this tree's
# bench.sh (BENCH_ROOT). sccache keys a Rust compile on every CARGO_*
# environment variable rustc sees, so anything that differs per worktree in
# one of those gets no hits at all (measured): CARGO_TARGET_DIR, or a
# CARGO_TARGET_<triple>_LINKER pointing into the worktree. SCCACHE_BASEDIRS
# (0.18) does not help Rust.
#
# The cache lives in target/tools/sccache-cache; the worktrees and the cache
# are deleted at the end (FC_BENCH_KEEP=1 keeps them).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/scripts/build-bench"
SCCACHE="${1:-$(command -v sccache || echo "$ROOT/target/tools/bin/sccache")}"
OUT="$ROOT/target/build-bench"
export BENCH_OUT="$OUT"
export RUSTC_WRAPPER="$SCCACHE"
export SCCACHE_DIR="$ROOT/target/tools/sccache-cache"
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-30G}"
T=scheduled_job_cron_golden_test
LEAF=crates/fc-platform/src/client/repository.rs

unset CARGO_TARGET_DIR
worktree() { # worktree <dir>: a detached worktree at HEAD with this tree's config
  git -C "$ROOT" worktree add -q --detach "$1" HEAD
  cp "$ROOT/.cargo/config.toml" "$1/.cargo/config.toml"
}
WA="$ROOT/target/sc-wt-a"
WB="$ROOT/target/sc-wt-b"
worktree "$WA"
worktree "$WB"

"$SCCACHE" --stop-server >/dev/null 2>&1 || true
rm -rf "$SCCACHE_DIR"
"$SCCACHE" --start-server
"$SCCACHE" --zero-stats >/dev/null

BENCH_ROOT="$WA" BENCH_LABEL=sccache-empty "$HERE/bench.sh" cold-test "$T"
"$SCCACHE" --show-stats >"$OUT/sccache-empty.txt"
du -sh "$SCCACHE_DIR" >>"$OUT/sccache-empty.txt"
"$SCCACHE" --zero-stats >/dev/null

BENCH_ROOT="$WB" BENCH_LABEL=sccache-warm "$HERE/bench.sh" cold-test "$T"
"$SCCACHE" --show-stats >"$OUT/sccache-warm.txt"
"$SCCACHE" --zero-stats >/dev/null

BENCH_ROOT="$WB" BENCH_LABEL=sccache-warm "$HERE/bench.sh" test-one "$LEAF" "$T" 3
"$SCCACHE" --show-stats >"$OUT/sccache-incremental.txt"

"$SCCACHE" --stop-server >/dev/null 2>&1 || true
if [ "${FC_BENCH_KEEP:-}" != 1 ]; then
  git -C "$ROOT" worktree remove --force "$WA"
  git -C "$ROOT" worktree remove --force "$WB"
  rm -rf "$SCCACHE_DIR"
fi
grep -hE "Compile requests|Cache hits|Cache misses|Non-cacheable|Cache size|sccache-cache" \
  "$OUT/sccache-empty.txt" "$OUT/sccache-warm.txt" "$OUT/sccache-incremental.txt"

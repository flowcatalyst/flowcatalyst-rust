#!/usr/bin/env bash
# Build-speed benchmarks for the workspace (docs/plans/build-speed-2026-09-28.md).
#
# Every scenario appends rows to $BENCH_OUT/results.csv and keeps cargo's
# --timings HTML under $BENCH_OUT/timings/, so part B (the crate split, the
# merged test binary) reruns exactly these measurements and compares.
#
#   bench.sh cold-check <pkg>                 # rm -rf target/debug, then check
#   bench.sh cold-build <pkg>                 # rm -rf target/debug, then build
#   bench.sh cold-test <test>                 # rm -rf target/debug, then build one fc-platform test
#   bench.sh incr-check <file> <pkg> [runs]   # edit <file>, check <pkg>
#   bench.sh incr-build <file> <pkg> [runs]   # edit <file>, build <pkg>
#   bench.sh test-one <file> <test> [runs]    # edit <file>, build + run one test binary
#   bench.sh test-all <file> [runs]           # edit <file>, build every fc-platform test binary
#   bench.sh size                             # target/ size, split by kind
#
# The "edit" inserts a comment line at the top of <file> (a real change to the
# file's contents and its spans, as a one-line edit is) and restores the file
# on exit. Incremental scenarios do one unmeasured warm-up build first, so
# every measured run starts from a warm target.
#
# Environment:
#   BENCH_OUT     results directory (default: target/build-bench)
#   BENCH_LABEL   tag for the CSV rows, e.g. "baseline" or "line-tables"
#   BENCH_MIN_FREE_GB  refuse to build below this much free disk (default 35)
#   BENCH_ROOT    the checkout to build (default: the one holding this script)
#   CARGO_TARGET_DIR   honoured as usual
#
# Link time is taken from a linker wrapper (timed-link.sh) set as the host
# target's linker. cargo_s is cargo's own total from --timings (it leaves out
# time spent blocked on a lock another cargo holds, which wall_s includes);
# compile_s = cargo_s - link_s, exact for the sequential scenarios.
# platform_s / platform_frontend_s: fc-platform's units (lib + test), total
# and front end (parse, expand, type-check; the rest is codegen). cpu_s is
# user + sys CPU time of the whole build: other builds on the machine inflate
# wall times far more than CPU times, so compare settings on cpu_s too.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
# BENCH_ROOT: build another checkout (a second worktree) with these scripts,
# so the linker wrapper's path (a CARGO_* variable rustc sees, which sccache
# hashes) is the same for every worktree.
ROOT="${BENCH_ROOT:-$(cd "$HERE/../.." && pwd)}"
cd "$ROOT"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
OUT="${BENCH_OUT:-$TARGET_DIR/build-bench}"
LABEL="${BENCH_LABEL:-unlabelled}"
MIN_FREE_GB="${BENCH_MIN_FREE_GB:-35}"
mkdir -p "$OUT/timings"
CSV="$OUT/results.csv"
HEADER="label,scenario,subject,run,wall_s,compile_s,link_s,links,run_s,load1_before,load1_after,free_gb,notes,cargo_s,platform_s,platform_frontend_s,cpu_s"
if [ ! -f "$CSV" ]; then
  echo "$HEADER" >"$CSV"
elif ! head -1 "$CSV" | grep -q cpu_s; then
  { echo "$HEADER"; tail -n +2 "$CSV"; } >"$CSV.tmp" && mv "$CSV.tmp" "$CSV"
fi

HOST="$(rustc -vV | sed -n 's/^host: //p')"
HOST_ENV="$(echo "$HOST" | tr 'a-z-' 'A-Z_')"
export "CARGO_TARGET_${HOST_ENV}_LINKER=$HERE/timed-link.sh"
export FC_LINK_LOG="$OUT/link.log"

now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }
load1() {
  if [ -r /proc/loadavg ]; then cut -d' ' -f1 /proc/loadavg; else sysctl -n vm.loadavg | awk '{print $2}'; fi
}
free_gb() {
  if df -g "$ROOT" >/dev/null 2>&1; then df -g "$ROOT" | awk 'NR==2{print $4}'; else df -BG "$ROOT" | awk 'NR==2{gsub("G","",$4); print $4}'; fi
}

check_disk() {
  local f
  f="$(free_gb)"
  if [ "$f" -lt "$MIN_FREE_GB" ]; then
    echo "bench: only ${f} GB free (< ${MIN_FREE_GB}); refusing to build" >&2
    exit 3
  fi
}

EDIT_FILE=""
EDIT_BACKUP=""
restore() {
  if [ -n "$EDIT_BACKUP" ] && [ -f "$EDIT_BACKUP" ]; then
    cp "$EDIT_BACKUP" "$EDIT_FILE"
    rm -f "$EDIT_BACKUP"
  fi
}
trap restore EXIT

edit() { # edit <file> <n>: put a fresh comment line on top of the pristine file
  local file="$1" n="$2"
  if [ -z "$EDIT_BACKUP" ]; then
    EDIT_FILE="$file"
    EDIT_BACKUP="$(mktemp)"
    cp "$file" "$EDIT_BACKUP"
  fi
  { echo "// build-bench edit $(date +%s)-$n"; cat "$EDIT_BACKUP"; } >"$file"
}

latest_timing() { ls -t "$TARGET_DIR"/cargo-timings/cargo-timing-2*.html 2>/dev/null | head -1; }

# run_cargo <scenario> <subject> <run> <cargo args...>
LAST_WALL=0 LAST_LINK=0 LAST_LINKS=0 LAST_LOG=""
run_cargo() {
  local scenario="$1" subject="$2" run="$3"
  shift 3
  check_disk
  : >"$FC_LINK_LOG"
  local lb t0 t1
  lb="$(load1)"
  t0="$(now)"
  LAST_LOG="$OUT/last-cargo.log"
  /usr/bin/time -p cargo "$@" --timings 2>&1 | tee "$LAST_LOG" | grep -E '^(error|warning: unused)|Finished|Executable' || true
  if grep -q '^error' "$LAST_LOG"; then echo "bench: cargo failed, see $LAST_LOG" >&2; exit 1; fi
  t1="$(now)"
  LAST_WALL="$(echo "$t1 - $t0" | bc)"
  LAST_LINK="$(awk '{s+=$2} END{printf "%.3f", s+0}' "$FC_LINK_LOG")"
  LAST_LINKS="$(wc -l <"$FC_LINK_LOG" | tr -d ' ')"
  LAST_LOAD_BEFORE="$lb"
  local tf
  tf="$(latest_timing)"
  LAST_TIMINGS=",,"
  if [ -n "$tf" ]; then
    cp "$tf" "$OUT/timings/${LABEL}-${scenario}-$(echo "$subject" | tr '/ ' '__')-r${run}.html"
    LAST_TIMINGS="$(python3 "$HERE/timings.py" --csv "$tf" fc-platform)"
  fi
  LAST_CARGO="${LAST_TIMINGS%%,*}"
  # CPU seconds (user + sys) of cargo and every rustc/linker it ran: much
  # less sensitive to the machine's load than wall time.
  LAST_CPU="$(awk '/^user /{u=$2} /^sys /{s=$2} END{printf "%.1f", u+s}' "$LAST_LOG")"
  [ -n "$LAST_CARGO" ] || LAST_CARGO="$LAST_WALL"
}

# record <scenario> <subject> <run> <run_s> <notes>
# compile_s is cargo's own build time (the --timings total, which leaves out
# time blocked on a cargo lock held by another build) minus the link time.
record() {
  local compile
  compile="$(echo "$LAST_CARGO - $LAST_LINK" | bc)"
  echo "$LABEL,$1,$2,$3,$LAST_WALL,$compile,$LAST_LINK,$LAST_LINKS,$4,$LAST_LOAD_BEFORE,$(load1),$(free_gb),$5,$LAST_TIMINGS,$LAST_CPU" >>"$CSV"
  echo "bench: $LABEL $1 $2 run $3: cargo ${LAST_CARGO}s (wall ${LAST_WALL}s, cpu ${LAST_CPU}s) link ${LAST_LINK}s (${LAST_LINKS} links) run ${4}s load ${LAST_LOAD_BEFORE}"
}

scenario="${1:-}"
shift || true
case "$scenario" in
cold-check)
  pkg="$1"
  rm -rf "$TARGET_DIR/debug"
  run_cargo cold-check "$pkg" 1 check -p "$pkg"
  record cold-check "$pkg" 1 "" ""
  ;;
cold-build)
  pkg="$1"
  rm -rf "$TARGET_DIR/debug"
  run_cargo cold-build "$pkg" 1 build -p "$pkg"
  record cold-build "$pkg" 1 "" ""
  ;;
cold-test)
  test="$1"
  rm -rf "$TARGET_DIR/debug"
  run_cargo cold-test "$test" 1 test -p fc-platform --test "$test" --no-run
  record cold-test "$test" 1 "" ""
  ;;
incr-check | incr-build)
  file="$1" pkg="$2" runs="${3:-3}"
  verb="${scenario#incr-}"
  run_cargo warmup "$pkg" 0 "$verb" -p "$pkg"
  for n in $(seq 1 "$runs"); do
    edit "$file" "$n"
    run_cargo "$scenario" "$(basename "$file")" "$n" "$verb" -p "$pkg"
    record "$scenario" "$(basename "$file")->$pkg" "$n" "" ""
  done
  ;;
test-one)
  file="$1" test="$2" runs="${3:-3}"
  run_cargo warmup "$test" 0 test -p fc-platform --test "$test" --no-run
  for n in $(seq 1 "$runs"); do
    edit "$file" "$n"
    run_cargo test-one "$test" "$n" test -p fc-platform --test "$test" --no-run
    exe="$(sed -n 's/.*Executable .*(\(.*\))$/\1/p' "$LAST_LOG" | tail -1)"
    r0="$(now)"
    case "$exe" in /*) ;; *) exe="$ROOT/$exe" ;; esac
    (cd "$ROOT/crates/fc-platform" && "$exe" -q >/dev/null) || { echo "bench: test failed" >&2; exit 1; }
    r1="$(now)"
    record test-one "$(basename "$file")->$test" "$n" "$(echo "$r1 - $r0" | bc)" ""
  done
  ;;
test-all)
  file="$1" runs="${2:-1}"
  run_cargo warmup all-tests 0 test -p fc-platform --no-run
  for n in $(seq 1 "$runs"); do
    edit "$file" "$n"
    run_cargo test-all all-tests "$n" test -p fc-platform --no-run
    record test-all "$(basename "$file")->fc-platform-tests" "$n" "" "links run in parallel: link_s is the sum; compile_s = cargo_s - link_s is only a bound"
  done
  ;;
size)
  for d in "$TARGET_DIR/debug" "$TARGET_DIR/debug/deps" "$TARGET_DIR/debug/incremental" "$TARGET_DIR/debug/build" "$TARGET_DIR/release"; do
    [ -d "$d" ] && du -sh "$d"
  done
  if [ -d "$TARGET_DIR/debug/deps" ]; then
    echo "test/bin executables in deps (count, total):"
    find "$TARGET_DIR/debug/deps" -maxdepth 1 -type f -perm -u+x ! -name '*.d' ! -name '*.rlib' ! -name '*.rmeta' ! -name '*.dylib' ! -name '*.so' -print0 |
      xargs -0 du -ck 2>/dev/null | tail -1 | awk '{printf "  %.1f GB\n", $1/1048576}'
    find "$TARGET_DIR/debug/deps" -maxdepth 1 -type f -perm -u+x ! -name '*.d' ! -name '*.rlib' ! -name '*.rmeta' ! -name '*.dylib' ! -name '*.so' | wc -l
    echo "fc_platform object files / rlibs:"
    du -ch "$TARGET_DIR"/debug/deps/libfc_platform-* 2>/dev/null | tail -1
    echo "incremental, fc_platform:"
    du -ch "$TARGET_DIR"/debug/incremental/fc_platform-* 2>/dev/null | tail -1
  fi
  ;;
*)
  sed -n '2,30p' "$0"
  exit 2
  ;;
esac

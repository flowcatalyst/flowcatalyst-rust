#!/usr/bin/env bash
# One build-settings experiment: the whole measurement set under one label.
#
#   scripts/build-bench/experiment.sh <label> [--with-check]
#
#   1. cold-test   rm -rf target/debug; build one fc-platform test binary
#                  (every dependency and dev-dependency, codegen included)
#   2. test-one    x5: edit a leaf file, rebuild that test binary, run it
#   3. test-all    x2: edit a leaf file, rebuild all fc-platform test binaries
#   4. size        target/ breakdown -> $BENCH_OUT/size-<label>.txt
#   5. incr-check  x5 (with --with-check): edit a leaf file, check fc-server
#
# Runs under disk-guard.sh (stops below 35 GB free or above 50 GB of target/).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/scripts/build-bench"
label="$1"
with_check="${2:-}"
export BENCH_LABEL="$label"
OUT="${BENCH_OUT:-${CARGO_TARGET_DIR:-$ROOT/target}/build-bench}"
LEAF="crates/fc-platform-iam/src/client/repository.rs"
TEST="it:scheduled_job_cron_golden_test::"

(
  "$HERE/bench.sh" cold-test "$TEST"
  "$HERE/bench.sh" test-one "$LEAF" "$TEST" 5
  "$HERE/bench.sh" test-all "$LEAF" 2
  "$HERE/bench.sh" size >"$OUT/size-$label.txt" 2>&1
  if [ "$with_check" = "--with-check" ]; then
    "$HERE/bench.sh" incr-check "$LEAF" fc-server 5
  fi
) &
pid=$!
"$HERE/disk-guard.sh" "$pid" 35 50 &
wait "$pid"
cat "$OUT/size-$label.txt"

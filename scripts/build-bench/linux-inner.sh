#!/usr/bin/env bash
# Runs inside the container linux-docker.sh starts (see there). Env: LABEL,
# DEPS_DEBUG (debug level for dependencies), SPLIT (dev split-debuginfo).
set -euo pipefail
apt-get update -qq >/dev/null && apt-get install -y -qq lld mold time bc >/dev/null
cd /work/src
cfg=.cargo/config.toml
if [ -n "${DEPS_DEBUG:-}" ]; then
  # A number or boolean stays bare TOML; anything else is a string.
  case "$DEPS_DEBUG" in [0-9] | true | false) V="$DEPS_DEBUG" ;; *) V="\"$DEPS_DEBUG\"" ;; esac
  # Drop the section's debug line, then set it right after opt-level.
  V="$V" perl -i -ne 'if (/^\[/) { $in = /^\[profile\.dev\.package\."\*"\]/ } next if $in && /^debug = /; print; print "debug = $ENV{V}\n" if $in && /^opt-level = /' "$cfg"
fi
if [ -n "${SPLIT:-}" ]; then
  V="$SPLIT" perl -0pi -e 's/(\[profile\.dev\]\n)/$1split-debuginfo = "$ENV{V}"\n/' "$cfg"
fi
sed -n '/^\[profile.dev\]/,/^\[profile.release\]/p' "$cfg"
export CARGO_HOME=/work/cargo CARGO_TARGET_DIR=/work/target BENCH_OUT=/work/bench BENCH_MIN_FREE_GB=5
B=scripts/build-bench/bench.sh
LEAF=crates/fc-platform/src/client/repository.rs
T=scheduled_job_cron_golden_test
BENCH_LABEL="linux-$LABEL" $B cold-test "$T"
for ld in bfd lld mold; do
  FC_BENCH_FUSE_LD=$ld BENCH_LABEL="linux-$LABEL-$ld" $B test-one "$LEAF" "$T" 3
done
cargo test -q -p fc-platform --no-run --test go_routes_test --test auth_security_test \
  --test client_admin_test --test function_api_test 2>&1 | tail -2
exes() { find /work/target/debug/deps -maxdepth 1 -type f -perm -u+x ! -name '*.so' ! -name '*.d' "$@"; }
{
  echo "== linux-$LABEL: test executables (bytes, name)"
  exes -printf '%s %f\n' | sort -n
  echo "== DWARF share of the largest executable"
  f="$(exes -printf '%s %p\n' | sort -n | tail -1 | cut -d' ' -f2)"
  size -A "$f" | awk '/^\.debug/{d+=$2} /^Total/{t=$2} END{printf "debug sections %.0f MB of %.0f MB\n", d/1048576, t/1048576}'
  echo "== target"
  du -sh /work/target/debug /work/target/debug/deps /work/target/debug/incremental
} >/work/bench/linux-sizes.txt
cat /work/bench/linux-sizes.txt

# Shared helpers: run one measurement, echo the exact command, append to a results file.
set -u
cd "$(dirname "$0")/.."
B=./target/release/fnhost-density
V=./target/release/fnhost-v8
export FC_SCRATCH="${FC_SCRATCH:-${TMPDIR:-/tmp}/fnhost-density}"
run() {
  local out="$1"; shift
  echo "\$ ${FC_TICK_US:+FC_TICK_US=$FC_TICK_US }$*" | tee -a "results/$out"
  "$@" 2>&1 | grep -E "^(scenario|egress)=|^egress|Error|error|panicked|^-rw" | tee -a "results/$out"
}

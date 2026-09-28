#!/usr/bin/env bash
# Watches the disk while a benchmark builds, and kills the build when the
# volume's free space drops below MIN_FREE_GB or the target directory grows
# past MAX_TARGET_GB (the machine is shared with other builds).
#
#   scripts/build-bench/disk-guard.sh <pid-to-kill> [MIN_FREE_GB=35] [MAX_TARGET_GB=50]
#
# Exits when <pid> exits. Kills <pid>'s process group and every cargo/rustc
# whose working tree is this checkout.
set -u
pid="$1"
min_free="${2:-35}"
max_target="${3:-50}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
while kill -0 "$pid" 2>/dev/null; do
  free="$(df -g "$ROOT" | awk 'NR==2{print $4}')"
  used="$(du -sg "$TARGET_DIR" 2>/dev/null | awk '{print $1}')"
  if [ "$free" -lt "$min_free" ] || [ "${used:-0}" -gt "$max_target" ]; then
    echo "disk-guard: free ${free} GB, target ${used} GB: stopping $pid" >&2
    pkill -TERM -g "$(ps -o pgid= -p "$pid" | tr -d ' ')" 2>/dev/null
    kill -TERM "$pid" 2>/dev/null
    pgrep -f "$TARGET_DIR" | xargs kill -TERM 2>/dev/null
    exit 1
  fi
  sleep 20
done

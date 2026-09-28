#!/usr/bin/env bash
# The Linux side of the build benchmarks, in a Docker container (aarch64 on
# Apple Silicon, the same architecture as the production ECS tasks).
#
#   scripts/build-bench/linux-docker.sh <label> [--dev-debug-deps <value>] [--split-debuginfo <value>]
#
# Copies the working tree (tracked + untracked, not ignored) into a Docker
# volume, then inside rust:1.98 (Debian bookworm, GNU ld 2.40, plus lld and
# mold from apt):
#
#   1. cold-test: build one fc-platform test binary from nothing;
#   2. test-one x3 per linker (bfd, lld, mold): edit a leaf file, rebuild and
#      relink that test binary (timed-link.sh's FC_BENCH_FUSE_LD picks the
#      linker at link time, so cargo's fingerprints don't change);
#   3. builds four more fc-platform test binaries and records the size of
#      each test executable (on Linux the DWARF is inside the executable).
#
# Results go to target/build-bench/linux-results.csv and
# target/build-bench/linux-sizes-<label>.txt. The volume is removed at the end
# (FC_BENCH_KEEP_VOLUME=1 keeps it).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
label="$1"
shift
deps_debug=""
split=""
while [ $# -gt 0 ]; do
  case "$1" in
  --dev-debug-deps) deps_debug="$2"; shift 2 ;;
  --split-debuginfo) split="$2"; shift 2 ;;
  *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
IMAGE="${FC_BENCH_IMAGE:-rust:1-bookworm}"
VOL="fc-build-bench-linux-$label"
OUT="$ROOT/target/build-bench"
mkdir -p "$OUT"

docker volume create "$VOL" >/dev/null
trap '[ "${FC_BENCH_KEEP_VOLUME:-}" = 1 ] || docker volume rm -f "$VOL" >/dev/null' EXIT

(cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | COPYFILE_DISABLE=1 tar --no-xattrs --null -T - -cf -) |
  docker run -i --rm -v "$VOL:/work" "$IMAGE" sh -c 'mkdir -p /work/src && tar -xf - -C /work/src'

docker run --rm -v "$VOL:/work" \
  -e LABEL="$label" -e DEPS_DEBUG="$deps_debug" -e SPLIT="$split" \
  "$IMAGE" bash /work/src/scripts/build-bench/linux-inner.sh
docker run --rm -v "$VOL:/work" "$IMAGE" cat /work/bench/results.csv |
  { if [ -f "$OUT/linux-results.csv" ]; then tail -n +2; else cat; fi; } >>"$OUT/linux-results.csv"
docker run --rm -v "$VOL:/work" "$IMAGE" cat /work/bench/linux-sizes.txt >"$OUT/linux-sizes-$label.txt"

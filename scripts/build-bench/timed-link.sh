#!/bin/sh
# Linker wrapper for the build benchmarks: runs the real linker driver (`cc`,
# or $FC_BENCH_REAL_LINKER) and appends one line per link to $FC_LINK_LOG:
#
#   <start epoch> <seconds> <output file>
#
# bench.sh sets it as the target linker so a build's link time can be split
# from its compile time. It changes nothing about the link itself unless
# FC_BENCH_FUSE_LD is set (below).
real="${FC_BENCH_REAL_LINKER:-cc}"
# FC_BENCH_FUSE_LD=bfd|lld|mold swaps the linker the driver uses (drops any
# -fuse-ld= from the config's rustflags) without changing cargo's
# fingerprints, so one build can be relinked with each linker.
if [ -n "${FC_BENCH_FUSE_LD:-}" ]; then
  for a in "$@"; do
    shift
    case "$a" in -fuse-ld=*) ;; *) set -- "$@" "$a" ;; esac
  done
  set -- "-fuse-ld=$FC_BENCH_FUSE_LD" "$@"
fi
log="${FC_LINK_LOG:-/dev/null}"
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }
start=$(now)
"$real" "$@"
rc=$?
end=$(now)
out=""
prev=""
for a in "$@"; do
  [ "$prev" = "-o" ] && out="$a"
  prev="$a"
done
echo "$start $(echo "$end - $start" | bc) $(basename "$out")" >>"$log"
exit $rc

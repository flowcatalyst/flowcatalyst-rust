#!/usr/bin/env bash
# Noisy neighbour: A = echo at 500 req/s open loop (latency from dispatch), 10 s alone, then 10 s
# while B runs spin (100 ms deadline, discarded, repeated) or alloc (16 MiB touched) with
# B concurrent workers. Usage: neighbour.sh <engine> [B...]   (default B = 28 and 8)
. "$(dirname "$0")/lib.sh"
e="$1"; shift
bs="${*:-28 8}"
for b in $bs; do for m in spin alloc; do run neighbour.txt $B neighbour "$e" --mode $m --b $b --rate 500 --secs 10; done; done

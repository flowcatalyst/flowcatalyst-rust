#!/usr/bin/env bash
# Steady echo at c=1 and c=64, 10 s each after a 1 s warm-up. Usage: steady.sh <engine> [extra args]
. "$(dirname "$0")/lib.sh"
e="$1"; shift
for c in 1 64; do run steady.txt $B steady "$e" --c $c --secs 10 "$@"; done

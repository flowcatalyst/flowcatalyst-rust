#!/usr/bin/env bash
# Density: N distinct functions per engine; one process per point. Usage: density.sh <engine> <N...>
. "$(dirname "$0")/lib.sh"
e="$1"; shift
for n in "$@"; do run density.txt $B density "$e" --n "$n"; done

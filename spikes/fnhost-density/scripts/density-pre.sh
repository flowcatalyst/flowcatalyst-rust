#!/usr/bin/env bash
# Density from precompiled .cwasm (deserialize_file) — extism: its wasmtime cache. Usage: density-pre.sh <engine> <N...>
. "$(dirname "$0")/lib.sh"
e="$1"; shift
for n in "$@"; do run density.txt $B density "$e" --n "$n" --precompiled; done

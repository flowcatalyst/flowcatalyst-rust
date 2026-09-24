#!/usr/bin/env bash
# Decision 5 probes: HTTP egress policy, kernel/guest memory caps, log + WASI output routing, config/secret.
. "$(dirname "$0")/lib.sh"
for e in extism kernel native component; do run egress.txt $B egress $e; done
run egress.txt $B egress extism --mode reserve   # extism with a per-memory wasmtime reservation cap

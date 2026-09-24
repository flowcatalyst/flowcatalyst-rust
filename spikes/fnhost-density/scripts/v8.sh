#!/usr/bin/env bash
# (e) V8 isolates via deno_core: one isolate per function, a JSON-transform handler.
. "$(dirname "$0")/lib.sh"
run v8.txt $V density --n 100
run v8.txt $V density --n 1000
run v8.txt $V density --n 100 --no-snapshot
run v8.txt $V steady --c 1 --secs 10
run v8.txt $V steady --c 64 --secs 10

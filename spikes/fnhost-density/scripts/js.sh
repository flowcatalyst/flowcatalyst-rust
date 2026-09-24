#!/usr/bin/env bash
# (d) JS guests: QuickJS via extism-js (on (b') native and (a) extism) and StarlingMonkey via
# componentize-js (on (c); instance-per-request only, see the doc).
. "$(dirname "$0")/lib.sh"
QJS=guests/js-extism/js_echo.wasm
SM=guests/js-component/js_echo.component.wasm
ls -la $QJS $SM | tee -a results/js.txt
run js.txt $B density native --n 100 --guest $QJS
run js.txt $B density native --n 100 --guest $QJS --precompiled
run js.txt $B density extism --n 100 --guest $QJS
run js.txt $B steady native --c 1 --secs 10 --guest $QJS
run js.txt $B steady native --c 64 --secs 10 --guest $QJS
run js.txt $B steady extism --c 1 --secs 10 --guest $QJS
run js.txt $B density component --n 10 --fresh --guest $SM
run js.txt $B density component --n 100 --fresh --precompiled --guest $SM
run js.txt $B steady component --c 1 --secs 10 --fresh --guest $SM
run js.txt $B steady component --c 64 --secs 10 --fresh --guest $SM

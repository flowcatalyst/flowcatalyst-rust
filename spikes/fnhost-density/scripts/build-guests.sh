#!/usr/bin/env bash
# Rebuild the spike's own guests (the Extism fixture is Java's, copied byte-identical into fixtures/).
set -eu
cd "$(dirname "$0")/.."
( cd guests/comp-echo && cargo build --release --target wasm32-wasip2 )           # (c) Rust component
( cd guests/js-extism && extism-js index.js -i index.d.ts -o js_echo.wasm )        # (d) QuickJS via extism-js 1.6.1
( cd guests/js-component && npm install --no-audit --no-fund \
  && npx jco componentize app.js --wit wit --world-name proxy-fn --out js_echo.component.wasm )  # (d) StarlingMonkey

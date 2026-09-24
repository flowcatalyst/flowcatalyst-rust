#!/usr/bin/env bash
# Rebuilds the committed test guests (wasm32-wasip2 components) into
# ../fixtures/wasm/ and rewrites their SHA256SUMS. Needs the wasm32-wasip2
# target: `rustup target add wasm32-wasip2`. The tests never run this; they
# load the committed files and verify them against SHA256SUMS.
set -euo pipefail
cd "$(dirname "$0")"
guests=(echo spin alloc fail config secret http emit log pure)
cargo build --release --target wasm32-wasip2
out=../fixtures/wasm
mkdir -p "$out"
for guest in "${guests[@]}"; do
  cp "target/wasm32-wasip2/release/${guest}.wasm" "$out/${guest}.wasm"
done
( cd "$out" && shasum -a 256 "${guests[@]/%/.wasm}" > SHA256SUMS && cat SHA256SUMS )

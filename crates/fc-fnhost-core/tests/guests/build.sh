#!/usr/bin/env bash
# Rebuilds the committed test guests (wasm32-wasip2 components) into
# ../fixtures/wasm/ and rewrites their SHA256SUMS. Needs the wasm32-wasip2
# target: `rustup target add wasm32-wasip2`. The tests never run this; they
# load the committed files and verify them against SHA256SUMS.
#
#   ./build.sh              # every guest
#   ./build.sh pdk hello    # only these; the others stay as committed
#
# Rebuilds are not byte-reproducible across toolchains and checkouts, so
# rebuild only the guests you changed.
#
# `pdk` is this workspace's guest written with crates/fc-function-pdk (G1);
# `pdk-pure` is the PDK's own `pure` example, built without the
# `flowcatalyst` feature (so in the PDK's workspace, where no other guest
# unifies it back on); `hello` is examples/function-hello-rust (G2).
set -euo pipefail
cd "$(dirname "$0")"
all=(echo spin alloc fail config secret http emit emit_event log pure pdk pdk-pure hello)
if [[ $# -gt 0 ]]; then guests=("$@"); else guests=("${all[@]}"); fi
example=../../../../examples/function-hello-rust
pdk=../../../fc-function-pdk
out=../fixtures/wasm
mkdir -p "$out"
cargo build --release --target wasm32-wasip2
for guest in "${guests[@]}"; do
  case "$guest" in
    pdk-pure)
      cargo build --release --target wasm32-wasip2 --manifest-path "$pdk/Cargo.toml" \
        --example pure --no-default-features --features json
      cp "$pdk/target/wasm32-wasip2/release/examples/pure.wasm" "$out/pdk-pure.wasm"
      ;;
    hello)
      cargo build --release --target wasm32-wasip2 --manifest-path "$example/Cargo.toml"
      cp "$example/target/wasm32-wasip2/release/function_hello_rust.wasm" "$out/hello.wasm"
      ;;
    *)
      cp "target/wasm32-wasip2/release/${guest}.wasm" "$out/${guest}.wasm"
      ;;
  esac
done
( cd "$out" && shasum -a 256 "${all[@]/%/.wasm}" > SHA256SUMS && cat SHA256SUMS )

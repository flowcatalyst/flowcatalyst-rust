# fc-function-pdk

Write FlowCatalyst functions in Rust. A FlowCatalyst function is a WASI 0.2
component that exports `wasi:http/incoming-handler`; this crate gives it a
`#[handler]` entry point, a `Request`, a `Response` and a `Context` with the
host's `flowcatalyst:function` interfaces (config, secrets, events, logging
and the invocation context).

```rust,ignore
use fc_function_pdk::prelude::*;

#[handler]
async fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
    let order: serde_json::Value = req.json()?;
    let event = OutboundEvent::new("shop:orders:order:seen", req.header("x-id").unwrap_or("x"))?
        .with_json(&order)?;
    ctx.events().emit(&event)?;
    Ok(Response::ack())
}
```

The crate documentation (`cargo doc --open`, or `src/lib.rs`) has the whole
API, the features and the host's rules.

## Depending on it

Two ways, both supported.

**From crates.io**, once published:

```toml
[dependencies]
fc-function-pdk = "0.1"
```

**From the repository**, as a git dependency. This always works, including
for changes not yet released:

```toml
[dependencies]
fc-function-pdk = { git = "https://github.com/flowcatalyst/flowcatalyst-rust" }
# or pinned: { git = "…", tag = "…" } / { git = "…", rev = "…" }
```

Either way, build the component with:

```sh
rustup target add wasm32-wasip2                  # once
cargo build --release --target wasm32-wasip2
```

The `cdylib` in `target/wasm32-wasip2/release/` is the component to publish.
`templates/function-rust` (a `cargo generate` template) and
`examples/function-hello-rust` in the repository are complete functions.

Without the default `flowcatalyst` feature
(`default-features = false, features = ["json", "log"]`) the component
imports nothing but WASI 0.2 and also runs on `wasmtime serve`, Spin or
wasmCloud.

## The vendored WIT

`wit/flowcatalyst-function/` (the `flowcatalyst:function` package and its
WASI 0.2 dependencies) is a copy of the repository root's
`wit/flowcatalyst-function/`, which the function host is built from and
which stays the source of truth. The copy is what lets this crate build on its
own, from crates.io. After changing the root WIT, refresh it:

```sh
rm -rf crates/fc-function-pdk/wit/flowcatalyst-function
cp -R wit/flowcatalyst-function crates/fc-function-pdk/wit/
```

`crates/fc-fnhost-core/tests/pdk_wit_sync.rs` (part of the root workspace's
`cargo test`) fails while the two differ.

## Developing the crate

It is its own Cargo workspace (with `macros/`, the `#[handler]` attribute),
not a member of the repository's root workspace, so its `wasm32-wasip2`
dependencies stay out of every host build.

```sh
cargo test                                    # native unit tests
cargo test --target wasm32-wasip2             # the same tests, in wasmtime
```

To publish, in dependency order: `crates/fc-function-abi`, then
`crates/fc-function-pdk/macros`, then this crate, each at the same version.

## Licence

[MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/); see [LICENSE](LICENSE).

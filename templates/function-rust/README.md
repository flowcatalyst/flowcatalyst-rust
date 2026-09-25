# {{project-name}}

A FlowCatalyst function in Rust: a WASI 0.2 component that exports
`wasi:http/incoming-handler`, written with `fc-function-pdk`.

```sh
rustup target add wasm32-wasip2                  # once
cargo test                                       # unit tests, natively
cargo build --release --target wasm32-wasip2     # the component:
                                                 # target/wasm32-wasip2/release/{{crate_name}}.wasm
```

## The PDK dependency

`Cargo.toml` takes `fc-function-pdk` from the FlowCatalyst repository, as a
git dependency, which always works (pin it with `tag = "…"` or `rev = "…"`):

```toml
fc-function-pdk = { git = "https://github.com/flowcatalyst/flowcatalyst-rust" }
```

Once the PDK is published to crates.io, a version works as well:

```toml
fc-function-pdk = "0.1"
```

## Publishing the function

`manifest.json` declares the function's endpoints, config and secrets. Keep
`runtime: component` (a WASI 0.2 component exporting
`wasi:http/incoming-handler`, which is also the default `entrypoint`), and
publish to a pool served by Rust function hosts.

Publish and promote it with `fc-dev fn deploy <the .wasm> <app.service.name>`
(against a local `fc-dev`, which runs a function host), or over
`/api/functions/{address}` (upload the artifact, publish a version, promote
it to `live`); the steps are in
`examples/function-hello-rust/README.md` in the FlowCatalyst repository,
with a fuller example: an adapter that maps an event, calls an HTTPS API and
emits an event.

A platform that predates `runtime: component` (Java's included) takes the
same component as `runtime: wasm` with `entrypoint:
wasi_http_incoming_handler`, the manifest-safe name of the export; publish
there with Java's `fcdev fn deploy <the .wasm> <app.service.name> --manifest
manifest.json`.

Without the PDK's default `flowcatalyst` feature
(`fc-function-pdk = { …, default-features = false, features = ["json", "log"] }`)
the component imports nothing but WASI 0.2 and also runs on
`wasmtime serve`, Spin or wasmCloud (no config, secrets, events or
invocation context then).

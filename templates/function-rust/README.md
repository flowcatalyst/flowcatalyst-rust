# {{project-name}}

A FlowCatalyst function in Rust: a WASI 0.2 component that exports
`wasi:http/incoming-handler`, written with `fc-function-pdk`.

```sh
rustup target add wasm32-wasip2                  # once
cargo test                                       # unit tests, natively
cargo build --release --target wasm32-wasip2     # the component:
                                                 # target/wasm32-wasip2/release/{{crate_name}}.wasm
```

`manifest.json` declares the function's endpoints, config and secrets. Keep
`runtime: wasm` and `entrypoint: wasi_http_incoming_handler` (the
manifest-safe name of the `wasi:http/incoming-handler` export), and publish
to a pool served by Rust function hosts.

Publish and promote it with `fc-dev fn deploy <the .wasm> <app.service.name>`
(against a local `fc-dev`, which runs a function host) or Java's
`fcdev fn deploy <the .wasm> <app.service.name> --manifest manifest.json`,
or over `/api/functions/{address}` (upload the
artifact, publish a version, promote it to `live`); the steps are in
`examples/function-hello-rust/README.md` in the FlowCatalyst repository,
with a fuller example: an adapter that maps an event, calls an HTTPS API and
emits an event.

Without the PDK's default `flowcatalyst` feature
(`fc-function-pdk = { …, default-features = false, features = ["json", "log"] }`)
the component imports nothing but WASI 0.2 and also runs on
`wasmtime serve`, Spin or wasmCloud (no config, secrets, events or
invocation context then).

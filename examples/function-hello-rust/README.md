# function-hello-rust

A FlowCatalyst function in Rust: a WASI 0.2 component written with the
guest SDK, [`crates/fc-function-pdk`](../../crates/fc-function-pdk/src/lib.rs).

It is a small adapter. When an order is placed it:

1. receives the `shop:orders:order:placed` subscription delivery on
   `POST /events/order-placed` (a signed webhook, verified by the host);
2. maps the order onto a carrier's shipment request (reshapes the address,
   normalises the country and postcode, sums the lines into a declared value);
3. books the shipment over HTTPS with the `CARRIER_API_KEY` secret, idempotent
   on the order id;
4. emits `shop:fulfilment:shipment:requested`, deduplicated on the order id.

A carrier or platform outage answers `Response::retry` (the platform defers
the delivery without spending a retry); a refusal that a retry cannot fix
fails the attempt. `GET /healthz` answers `{"ok":true}`.

## Build

```sh
rustup target add wasm32-wasip2        # once
cargo test                             # the unit tests, natively
cargo build --release --target wasm32-wasip2
```

The component is `target/wasm32-wasip2/release/function_hello_rust.wasm`
(about 320 KB).

The host test `crates/fc-fnhost-core/tests/wasm_pdk.rs` runs this exact
component, with this `manifest.json`, on the real host. Its committed copy
is `crates/fc-fnhost-core/tests/fixtures/wasm/hello.wasm`; rebuild it with
`crates/fc-fnhost-core/tests/guests/build.sh hello`.

## The manifest

[`manifest.json`](manifest.json) is Java's manifest, unchanged:

- `runtime: wasm`, and `entrypoint: wasi_http_incoming_handler`: the
  manifest-safe name for `wasi:http/incoming-handler` (the entrypoint rule
  refuses `:` and `/`). The Rust host loads WASI 0.2 components only, so the
  version must go to a pool served by Rust hosts (`pool`).
- `config` and `secrets` declare the keys the function reads; promote refuses
  (`SETTINGS_MISSING`) until each has a value.
- `httpAllow` lists the carrier's host; any other outbound call is refused
  (`HttpError::Denied`).
- The subscription is created at promote and delivers to the webhook
  endpoint.

## Publish

The address used below is `shop.fulfilment.book-shipment`.

### Locally, with fc-dev

`fc-dev` runs a function host beside the platform (pool `default`, private
listener on `:8090`) and writes the `fc-dev fn` CLI's credentials, so the
loop needs no flags. The function's application needs a service account
with a signing secret, because the manifest declares a subscription;
`fc-dev init` makes both.

```sh
fc-dev                                                 # platform + function host
fc-dev init --code shop --name Shop                    # once: the application
fc-dev fn build                                        # cargo, wasm32-wasip2
fc-dev fn config set shop.fulfilment.book-shipment \
    CARRIER_API_URL=https://api.carrier.example CARRIER_ACCOUNT=acct-7
fc-dev fn secret set shop.fulfilment.book-shipment CARRIER_API_KEY < carrier-key.txt
fc-dev fn deploy target/wasm32-wasip2/release/function_hello_rust.wasm \
    shop.fulfilment.book-shipment                      # publish, READY, promote live
fc-dev fn invoke shop.fulfilment.book-shipment --path /healthz
```

The first `config set` creates the function from `manifest.json` (its
event type, `shop:orders:order:placed`, must exist for the subscription).
`fn deploy` uploads the component, publishes a version with the manifest,
waits for the host to report it `READY` and promotes it to `live`; deploying
the same bytes again is a no-op. Signatures are off in fc-dev.

### Against a deployed platform

With Java's `fcdev` CLI (it uploads any artifact, a component included),
or `fc-dev fn` with `--platform-url`, `--client-id` and `--client-secret`:

```sh
fcdev fn config set shop.fulfilment.book-shipment \
    CARRIER_API_URL=https://api.carrier.example CARRIER_ACCOUNT=acct-7 --manifest manifest.json
fcdev fn secret set shop.fulfilment.book-shipment CARRIER_API_KEY --manifest manifest.json
    # (the value on stdin: a secret-manager reference, or `encrypt:<the key>`)
fcdev fn deploy target/wasm32-wasip2/release/function_hello_rust.wasm \
    shop.fulfilment.book-shipment --manifest manifest.json --wait 120s
```

Or over the HTTP API directly (bearer token with the
`platform:function:version:publish` permission):

```sh
WASM=target/wasm32-wasip2/release/function_hello_rust.wasm
DIGEST="sha256:$(shasum -a 256 "$WASM" | cut -d' ' -f1)"
FN="$PLATFORM/api/functions/shop.fulfilment.book-shipment"

# 1. upload the artifact; the answer carries its platform:// reference
REF=$(curl -sf -X PUT -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/octet-stream' --data-binary @"$WASM" \
    "$FN/artifacts/$DIGEST" | jq -r .artifactRef)

# 2. publish a version (add "signatureBundle" when the platform requires
#    signatures: `cosign sign-blob --bundle fn.sigstore.json "$WASM"`)
jq -n --arg ref "$REF" --arg digest "$DIGEST" --slurpfile m manifest.json \
    '{artifactRef: $ref, digest: $digest, manifest: ($m[0] | del(."$schema"))}' |
  curl -sf -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    --data-binary @- "$FN/versions"

# 3. once the version is READY, promote it to live
curl -sf -X PUT -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d '{"version": 1}' "$FN/aliases/live"
```

The Rust platform serves the same `/api/functions*` interface.

## Start a new function

Scaffold one from [`templates/function-rust`](../../templates/function-rust/README.md)
with fc-dev (the template is built in; no `cargo generate` needed):

```sh
fc-dev fn init --runtime wasm --lang rust my-function
cd my-function
fc-dev fn build
fc-dev fn config set shop.default.my-function GREETING=Hello
fc-dev fn deploy target/wasm32-wasip2/release/my_function.wasm shop.default.my-function
fc-dev fn invoke shop.default.my-function --path /hello/world
```

`--pdk-path <checkout>/crates/fc-function-pdk` depends on a local PDK
instead of the git one. `cargo generate --git
https://github.com/flowcatalyst/flowcatalyst-rust templates/function-rust`
renders the same template.

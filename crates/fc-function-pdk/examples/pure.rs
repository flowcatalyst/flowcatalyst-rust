//! A pure `wasi:http/proxy` component written with the PDK: built without
//! the `flowcatalyst` feature it imports nothing but WASI 0.2, so it runs on
//! `wasmtime serve`, Spin or wasmCloud as well as on the FlowCatalyst host.
//!
//! ```sh
//! cargo build --release --target wasm32-wasip2 --example pure \
//!     --no-default-features --features json
//! wasmtime serve -Scli target/wasm32-wasip2/release/examples/pure.wasm
//! curl -d '{"n":1}' 'http://127.0.0.1:8080/any/path?x=1'
//! ```

use fc_function_pdk::prelude::*;
use serde_json::json as j;

#[handler]
async fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
    ctx.logger()
        .info(format_args!("{} {}", req.method(), req.path()));
    let body: serde_json::Value = if req.body().is_empty() {
        serde_json::Value::Null
    } else {
        req.json()?
    };
    json(
        200,
        &j!({
            "pure": true,
            "method": req.method(),
            "path": req.path(),
            "query": req.query(),
            "body": body,
        }),
    )
}

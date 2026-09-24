//! The Extism JSON envelope between Java's WASM host and a guest (Java
//! `function-host/src/main/java/io/flowcatalyst/fnhost/wasm/WasmAbi.java`,
//! `HostFunctions.java`; `docs/spec/function-wasm-runtime.md` §3-4 in the
//! Java repo). Behind the default-on `extism-abi` feature: if the Rust host
//! adopts WASI components instead, this module goes and the crate root stays.
//!
//! | Direction | Shape | Here |
//! |---|---|---|
//! | host → guest (the export's input) | [`Request`] as UTF-8 JSON, byte-identical to Java's | [`Request::encode`] / [`Request::decode`] |
//! | guest → host (the export's output) | `{"status","headers"?,"body"\|"bodyBase64"}` | [`decode_reply`] / [`encode_reply`] |
//! | guest → host (`fc_emit_event`) | the [`crate::OutboundEvent`] shape | [`decode_emit_input`] / [`encode_emit_input`] |
//! | host → guest (`fc_emit_event`'s result) | `{"ok":true}` / `{"ok":false,"error"}` | [`encode_emit_answer`] / [`decode_emit_answer`] |
//!
//! A malformed reply is never an error the host propagates: it answers
//! [`crate::Response::function_failed`] (`500 {"error":"the function failed"}`)
//! and discards the instance ([`MalformedReply::response`]).

mod base64;
mod emit;
mod jackson;
mod reply;
mod request;

pub use emit::{
    decode_emit_answer, decode_emit_input, encode_emit_answer, encode_emit_input, EmitAnswer,
};
pub use reply::{decode_reply, encode_reply, MalformedReply};
pub use request::Request;

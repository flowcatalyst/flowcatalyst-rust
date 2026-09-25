//! Write FlowCatalyst functions in Rust.
//!
//! A FlowCatalyst function is a WASI 0.2 component that exports
//! `wasi:http/incoming-handler`: each invocation arrives as an HTTP request
//! and the function answers with an HTTP response. On top of `wasi:http` the
//! host offers the `flowcatalyst:function` interfaces (config, secrets,
//! events, logging and the invocation context; `wit/flowcatalyst-function/`
//! at the repository root, vendored in this crate). This crate is the Rust
//! side of both: the surface Java's `function-api` gives JVM authors, mapped
//! onto `wasi:http` and the WIT.
//!
//! ```ignore
//! use fc_function_pdk::prelude::*;
//!
//! #[handler]
//! async fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
//!     let order: serde_json::Value = req.json()?;
//!     let event = OutboundEvent::new("shop:orders:order:seen", req.header("x-id").unwrap_or("x"))?
//!         .with_json(&order)?;
//!     ctx.events().emit(&event)?;
//!     Ok(Response::ack())
//! }
//! ```
//!
//! Build with `cargo build --release --target wasm32-wasip2`; the
//! `cdylib` is the component to publish, with `runtime: wasm` and
//! `entrypoint: wasi_http_incoming_handler` in its manifest.
//!
//! # What maps to what
//!
//! | Java `function-api` | This crate |
//! |---|---|
//! | `Function.handle(Request, FunctionContext)` | a [`#[handler]`](handler) function |
//! | `Request` | [`Request`] (HTTP) + [`Context::invocation`] (ids, caller, …) |
//! | `Result.ack/retry/fail/json/http` | [`Response`] (re-exported), [`json()`](fn@json) |
//! | a thrown exception | an `Err` from the handler: logged; `500 {"error":"the function failed"}` |
//! | `ctx.config()`, `ctx.secrets()` | [`Context::config`], [`Context::secrets`] |
//! | `ctx.events().emit(OutboundEvent)` / `EventEmitException` | [`Events::emit`] / [`EmitError`] |
//! | `ctx.http().send(HttpCall)` / `HttpCallRefusedException` | [`Http::send`] / [`HttpError::Denied`] |
//! | `ctx.logger()` | [`Context::logger`], and the `log` crate (feature `log`) |
//! | `ctx.clock()` | [`Context::now`] |
//! | `Webhook.event/schedule(body)` | [`Webhook::event`]`(&req)`, [`Webhook::schedule`]`(&req)` |
//!
//! # Features
//!
//! | Feature | Default | What it adds |
//! |---|---|---|
//! | `flowcatalyst` | yes | The `flowcatalyst:function` imports: [`Context::config`], [`Context::secrets`], [`Context::events`], [`Context::invocation`], [`Request::path_param`], host logging. |
//! | `json` | yes | serde helpers: [`Request::json`], [`HttpCall::with_json`], [`HttpReply::json`], [`json()`](fn@json), [`OutboundEventExt::with_json`]. |
//! | `log` | yes | `log::info!` and friends reach the function's logger. |
//!
//! Without `flowcatalyst` the component imports nothing but WASI 0.2 (the
//! `wasi:http/proxy` world), so it runs unchanged on `wasmtime serve`, Spin
//! or wasmCloud; the API that needs the FlowCatalyst host does not exist at
//! compile time rather than failing at run time. Logging then goes to
//! standard output and error, which the FlowCatalyst host also logs.
//!
//! # The host's rules
//!
//! One instance serves one request (globals never survive between calls);
//! bodies are buffered in full; memory is capped by `limits.wasmMemoryMb`;
//! the call stops at the endpoint's `timeoutMs`; outbound HTTP reaches only
//! `httpAllow` hosts, over `https`, without following redirects. See
//! `wit/flowcatalyst-function/function.wit` for the whole list.
//!
//! # Testing a function
//!
//! Everything here also compiles for the host target, and [`testing::TestHost`]
//! stands in for the FlowCatalyst host, so a handler's unit tests run with a
//! plain `cargo test`.

// The crate docs describe every feature; without one, its links dangle.
#![cfg_attr(
    not(all(feature = "flowcatalyst", feature = "json")),
    allow(rustdoc::broken_intra_doc_links)
)]

mod backend;
mod context;
mod error;
mod http;
#[cfg(feature = "json")]
mod json;
#[cfg(feature = "log")]
mod log_bridge;
mod query;
mod request;
mod runtime;
pub mod testing;
mod wasi;

#[cfg(feature = "flowcatalyst")]
pub use context::{Config, EmitError, Events, Invocation, MissingKey, Secrets};
pub use context::{Context, Level, Logger};
pub use error::{Error, HandlerOutput, Result};
pub use http::{Http, HttpCall, HttpDenied, HttpError, HttpReply};
#[cfg(feature = "json")]
pub use json::{json, OutboundEventExt};
pub use request::Request;
pub use runtime::block_on;

/// Makes a function the component's `wasi:http/incoming-handler` export.
///
/// The function takes a [`Request`] and, optionally, a [`Context`]; it may be
/// `async` or not; it returns a [`Response`] or a `Result<Response, E>` with
/// `E: Display` (see [`HandlerOutput`]). An `Err` is logged (with its causes)
/// and answers `500 {"error":"the function failed"}`; return
/// `Response::fail(message)` to send a message of your choosing. A panic
/// traps, which the host answers with `500 {"error":"the function failed"}`.
///
/// One per component: a component has exactly one incoming handler.
pub use fc_function_pdk_macros::handler;

pub use fc_function_abi::{
    emit_error, permission_matches, Caller, Event, EventEmitError, FunctionAddress, InvalidAddress,
    InvalidArgument, MultiMap, OutboundEvent, Principal, Response, Schedule, Timestamp, Webhook,
    WebhookFormatError, FAILURE_REASON,
};

/// The names nearly every function uses.
pub mod prelude {
    pub use crate::{
        handler, Caller, Context, Error, Http, HttpCall, HttpError, HttpReply, OutboundEvent,
        Request, Response, Webhook,
    };
    #[cfg(feature = "json")]
    pub use crate::{json, OutboundEventExt};
    #[cfg(feature = "flowcatalyst")]
    pub use crate::{EmitError, Invocation};
}

/// What `#[handler]` expands to. Not a public API.
#[doc(hidden)]
pub mod __private {
    pub use crate::runtime::serve;
    pub use wasip2;
    pub use wasip2::exports::http::incoming_handler::Guest as IncomingHandler;
    pub use wasip2::http::types::{IncomingRequest, ResponseOutparam};
}

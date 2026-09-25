//! The FlowCatalyst function ABI: what a function sees of its host, mirrored
//! from Java's `function-api` module (`../flowcatalyst-javalin` at `0118cdca`)
//! so a guest built against it behaves exactly as one built against Java.
//!
//! Shared by the Rust function host (`fc-fnhost`) and the Rust guest PDK. Pure
//! types plus logic, no async runtime, and it builds for
//! `wasm32-unknown-unknown`.
//!
//! # Layout
//!
//! The crate root holds what does not depend on how the host and guest talk:
//!
//! | Item | Java source |
//! |---|---|
//! | [`FunctionAddress`] | `function-api/.../FunctionAddress.java` |
//! | [`Caller`], [`Principal`], [`permission_matches`] | `function-api/.../Caller.java` |
//! | [`Response`] (`ack` / `retry` / `fail` / `json` / `http`) | `function-api/.../Result.java` |
//! | [`OutboundEvent`], [`EventEmitError`], [`emit_error`] codes | `OutboundEvent.java`, `EventEmitException.java`, `fnhost/wasm/HostFunctions.java` |
//! | [`Webhook::event`], [`Webhook::schedule`], [`Event`], [`Schedule`] | `Webhook.java`, `Event.java`, `Schedule.java` |
//!
//! # Fidelity
//!
//! What a guest can observe means the same as on Java: the same envelopes
//! accepted, the same fields, `String.isBlank`'s notion of whitespace. Byte
//! spelling that JSON does not give meaning to (escaping, key order, number
//! formatting) is serde's. `tests/data/java-golden/` holds answers generated
//! by running Java's own code (see `tests/java/`), and the tests compare
//! against them.
//!
//! Enums follow ruling X-06 (strict: an unknown value is an error) except
//! where Java carries a raw string; those stay strings, with a comment saying
//! so ([`Principal::tier`], [`Principal::principal_type`],
//! [`Schedule::trigger_kind`]).

mod address;
mod caller;
mod error;
mod event;
#[doc(hidden)]
pub mod java;
mod response;
mod timestamp;
mod webhook;

pub use address::{FunctionAddress, InvalidAddress};
pub use caller::{permission_matches, Caller, Principal};
pub use error::InvalidArgument;
pub use event::{emit_error, EventEmitError, OutboundEvent};
pub use response::{Response, FAILURE_REASON};
pub use timestamp::Timestamp;
pub use webhook::{Event, Schedule, Webhook, WebhookFormatError};

/// An ordered multi-valued map: query parameters, request headers and
/// response headers. Keys keep their original spelling and insertion order,
/// as Java's `LinkedHashMap<String, List<String>>` does; equality ignores
/// order, as `Map.equals` does.
pub type MultiMap = indexmap::IndexMap<String, Vec<String>>;

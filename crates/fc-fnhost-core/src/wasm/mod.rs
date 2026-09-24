//! The WASM runtime (plan §5 H4): `runtime: wasm` functions as WASI 0.2
//! components exporting `wasi:http/incoming-handler`, on plain wasmtime
//! (the design `docs/function-runner-density.md` §5-§8 chose), plus the
//! optional `flowcatalyst:function` interfaces (`wit/flowcatalyst-function`).
//!
//! | Piece | Where |
//! |---|---|
//! | one `Engine`, pooling allocator, epoch ticker, `.cwasm` fingerprint | [`engine`] |
//! | load-time checks and their refusal codes | [`inspect`] |
//! | the `.cwasm` cache and its invariant | [`cwasm`] |
//!
//! **Load refusals** (the heartbeat's `LOAD:<code>`): `WASM_INVALID`,
//! `WASM_CORE_MODULE_UNSUPPORTED`, `WASM_IMPORT_NOT_ALLOWED`,
//! `WASM_ENTRYPOINT_NOT_EXPORTED`, `WASM_MEMORY_OVER_CAP`.

pub mod cwasm;
pub mod engine;
pub mod inspect;

pub use cwasm::Source as CompileSource;
pub use engine::EngineSettings;
pub use inspect::{
    WASM_CORE_MODULE_UNSUPPORTED, WASM_ENTRYPOINT_NOT_EXPORTED, WASM_IMPORT_NOT_ALLOWED,
    WASM_INVALID, WASM_MEMORY_OVER_CAP,
};

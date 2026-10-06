//! Every integration test of this crate, in one test binary: one module
//! per former `tests/<name>.rs` (docs/plans/build-speed-2026-09-28.md,
//! section 7). One binary links once instead of once per file.
//!
//! `cargo test -p <crate> --test it <module>::` runs one former file;
//! `-- --ignored` still selects the Docker tests.
#![expect(
    clippy::let_underscore_must_use,
    reason = "test code: a discarded Result is a deliberate no-op in a test (setup, teardown or a send whose receiver is gone)"
)]
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code: a failed unwrap, expect or panic is a failed test (clippy's test exemption covers #[test] fns and #[cfg(test)] modules, not the helpers of an integration-test crate)"
)]

#[path = "../support/mod.rs"]
mod support;

mod control_plane;
mod db_postgres;
mod host_integration;
mod listener;
mod listener_public;
mod observability;
mod pdk_wit_sync;
mod reconcile_loop;
mod reconciler;
mod wasm_db;
mod wasm_fuel;
mod wasm_listener;
mod wasm_loading;
mod wasm_neighbour;

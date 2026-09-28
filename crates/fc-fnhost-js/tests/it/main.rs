//! Every integration test of this crate, in one test binary: one module
//! per former `tests/<name>.rs` (docs/plans/build-speed-2026-09-28.md,
//! section 7). One binary links once instead of once per file.
//!
//! `cargo test -p <crate> --test it <module>::` runs one former file;
//! `-- --ignored` still selects the Docker tests.

#[path = "../support/mod.rs"]
mod support;

mod js_density;
mod js_listener;
mod js_loading;
mod shared_budget;

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

mod function_host_role;
mod jvm_function_host_e2e;
mod mcp_role;
mod outbox_role_backends;
mod prod_env_boot_test;
mod router_role_prod_env;

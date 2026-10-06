//! Every integration test of this crate, in one test binary: one module
//! per former `tests/<name>.rs` (docs/plans/build-speed-2026-09-28.md,
//! section 7). One binary links once instead of once per file.
//!
//! `cargo test -p <crate> --test it <module>::` runs one former file;
//! `-- --ignored` still selects the Docker tests.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed unwrap, expect or panic is a failed test (clippy's test exemption covers #[test] fns and #[cfg(test)] modules, not the helpers of an integration-test crate)"
)]

mod audit_redaction;
mod axum_smoke;
mod scheduled_jobs_runner;

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

mod api_operator_surface_test;
mod api_platform_auth_test;
mod cascade_dispatch_mode_test;
mod fifo_tests;
mod health_startup_gate_test;
mod integration_tests;
mod manager_synth_pool_test;
mod manager_tests;
mod mediation_conformance_test;
mod mediation_http2_test;
mod mediator_tests;
mod observability_test;
mod pool_retry_in_place_test;
mod pool_tests;
mod rate_limit_tests;
mod router_http_prefix_test;
mod settled_reporter_test;

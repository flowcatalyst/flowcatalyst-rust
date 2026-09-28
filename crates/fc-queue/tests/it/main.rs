//! Every integration test of this crate, in one test binary: one module
//! per former `tests/<name>.rs` (docs/plans/build-speed-2026-09-28.md,
//! section 7). One binary links once instead of once per file.
//!
//! `cargo test -p <crate> --test it <module>::` runs one former file;
//! `-- --ignored` still selects the Docker tests.

mod activemq_integration_tests;
mod nats_integration_tests;
mod postgres_integration_tests;
mod sqs_integration_tests;
mod sqs_publisher_localstack_test;

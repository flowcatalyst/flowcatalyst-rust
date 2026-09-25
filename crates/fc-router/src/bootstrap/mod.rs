//! Router bootstrap: Go's router environment ([`RouterEnv`]) and one wiring
//! of it ([`RouterRuntime`]) for every binary that runs a router — the
//! standalone `fc-router` and `fc-server` in its router role
//! (`MESSAGE_ROUTER_ENABLED=true`), as Go's `newRouterServer` is shared by
//! its callers. With the `backends` feature it also carries the broker
//! consumer factory and SQS publisher both binaries use.

mod env;
mod runtime;

#[cfg(feature = "backends")]
mod backends;

pub use env::{
    dev_router_config, RouterCredentials, RouterEnv, RouterEnvError, DEFAULT_CONFIG_INTERVAL,
    DEFAULT_DRAIN_TIMEOUT, DEFAULT_NOTIFY_BATCH_INTERVAL_SECS,
};
pub use runtime::{RouterRuntime, RouterRuntimeOptions};

#[cfg(feature = "backends")]
pub use backends::{sqs_client, SchemeConsumerFactory, SqsPublisher};

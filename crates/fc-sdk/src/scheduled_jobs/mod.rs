//! Consumer-side scheduled-job runner. Re-export `client::scheduled_jobs`
//! DTOs for convenience so users only need one import.

mod runner;

pub use runner::{
    BoxedHandler, HandlerContext, HandlerError, HandlerFuture, LogOptions, OnErrorHook, RunResult,
    RunnerError, RunnerOptions, ScheduledJobEnvelope, ScheduledJobRunner,
    ScheduledJobRunnerBuilder, TriggerKind,
};

// Re-export the DTOs that handlers will need.
pub use crate::client::scheduled_jobs::{
    CompletionStatus, InstanceCompleteRequest, InstanceLogRequest, LogLevel,
};

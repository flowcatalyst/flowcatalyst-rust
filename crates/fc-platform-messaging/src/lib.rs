//! FlowCatalyst Platform: messaging.
//!
//! Event types, events, subscriptions, connections, dispatch pools and
//! dispatch jobs, the dispatch scheduler, processes, the platform's own
//! event-type catalogue (`seed`), and the ingest and dispatch endpoints in
//! `shared`.
//!
//! Every module keeps its path from the single `fc-platform` crate this was
//! split from (docs/plans/build-speed-2026-09-28.md, section 6); `fc-platform`
//! re-exports them at `fc_platform::<module>`.

pub mod connection;
pub mod dispatch_job;
pub mod dispatch_job_actions;
pub mod dispatch_pool;
pub mod event;
pub mod event_type;
pub mod process;
pub mod scheduler;
pub mod seed;
pub mod service_account;
pub mod shared;
pub mod subscription;

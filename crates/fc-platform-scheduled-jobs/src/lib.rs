//! FlowCatalyst Platform: scheduled jobs (definitions, the cron scheduler
//! and its firings).
//!
//! Every module keeps its path from the single `fc-platform` crate this was
//! split from (docs/plans/build-speed-2026-09-28.md, section 6); `fc-platform`
//! re-exports them at `fc_platform::<module>`.

pub mod scheduled_job;

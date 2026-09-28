//! ScheduledJob use cases.
//!
//! Every write to a ScheduledJob aggregate goes through one of these.
//! Instance + log writes (cron tick, delivery lifecycle, log/complete
//! callbacks) are infrastructure paths and live in `instance_repository.rs`,
//! NOT here.

pub mod events;

pub mod archive;
pub mod create;
pub mod delete;
pub mod fire_now;
pub mod pause;
pub mod resume;
pub mod sync;
pub mod update;

pub use archive::{ArchiveScheduledJobCommand, ArchiveScheduledJobUseCase};
pub use create::{CreateScheduledJobCommand, CreateScheduledJobUseCase};
pub use delete::{DeleteScheduledJobCommand, DeleteScheduledJobUseCase};
pub use fire_now::{FireScheduledJobCommand, FireScheduledJobUseCase};
pub use pause::{PauseScheduledJobCommand, PauseScheduledJobUseCase};
pub use resume::{ResumeScheduledJobCommand, ResumeScheduledJobUseCase};
pub use sync::{ScheduledJobSyncEntry, SyncScheduledJobsCommand, SyncScheduledJobsUseCase};
pub use update::{UpdateScheduledJobCommand, UpdateScheduledJobUseCase};

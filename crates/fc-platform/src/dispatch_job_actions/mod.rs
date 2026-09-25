//! Operator actions on dispatch jobs, as Go serves them
//! (`dispatchjob/api/api.go:75-78,109-112`, `dispatchjob/operations/*`):
//! requeue, cancel, complete and the signing dry run. Kept apart from
//! `dispatch_job/` (the scheduler's), which it only reads through.

pub mod api;
pub mod operations;
pub mod repository;

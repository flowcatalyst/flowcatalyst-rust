//! Shared services and endpoints of messaging: event and dispatch-job
//! ingest, the dispatch-processing callback, the dispatch queue, the read
//! projections.

pub mod batch_api;
pub mod dispatch_process_api;
pub mod dispatch_queue;
pub mod projections_service;
pub mod sdk_dispatch_jobs_api;

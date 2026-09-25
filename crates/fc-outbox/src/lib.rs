//! FlowCatalyst outbox processor: forwards the rows an application's SDK
//! writes to its outbox table to the platform's batch APIs, as Go's
//! `internal/outbox` does. See [`enhanced_processor`] for the behaviour.

pub mod backend;
pub mod enhanced_processor;
pub mod group_distributor;
pub mod group_state;
pub mod http_dispatcher;
pub mod recovery;
pub mod repository;

#[cfg(feature = "mongo")]
pub mod mongo;
#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(any(feature = "postgres", test))]
pub mod postgres;
#[cfg(any(feature = "sqlite", test))]
pub mod sqlite;

#[cfg(test)]
mod processor_tests;

// Re-export key types
pub use backend::{OutboxBackend, UnknownOutboxBackend};
pub use enhanced_processor::{EnhancedOutboxProcessor, EnhancedProcessorConfig, ProcessorMetrics};
pub use group_distributor::{DistributorStats, GroupDistributor, GroupHandler};
pub use group_state::{GroupInfo, GroupStateManager, GroupStatus};
pub use http_dispatcher::{
    DispatchOutcome, HttpDispatcher, HttpDispatcherConfig, OutboxDispatcher, MAX_PLATFORM_BATCH,
};
pub use recovery::{RecoveryConfig, RecoveryTask};
pub use repository::{ClaimedBatch, InvalidRow, OutboxRepository, OutboxTableConfig};

/// Leader election configuration. Re-exported from `fc_common` — a single
/// unified type replacing the previous per-crate duplicates in fc-outbox and fc-standby.
pub use fc_common::LeaderElectionConfig;

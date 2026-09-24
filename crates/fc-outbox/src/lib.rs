pub mod buffer;
pub mod enhanced_processor;
pub mod group_distributor;
pub mod http_dispatcher;
pub mod message_group_processor;
pub mod recovery;
pub mod repository;

#[cfg(feature = "mongo")]
pub mod mongo;
#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "sqlite")]
pub mod sqlite;

// Re-export key types
pub use buffer::{BufferFullError, GlobalBuffer, GlobalBufferConfig};
pub use enhanced_processor::{EnhancedOutboxProcessor, EnhancedProcessorConfig, ProcessorMetrics};
pub use group_distributor::{DistributorStats, GroupDistributor, GroupDistributorConfig};
pub use http_dispatcher::{
    BatchRequest, BatchResponse, HttpDispatcher, HttpDispatcherConfig, ItemStatus,
    OutboxDispatchResult,
};
pub use message_group_processor::{
    BatchDispatchResult, BatchItemResult, BatchMessageDispatcher, DispatchResult,
    MessageGroupProcessor, MessageGroupProcessorConfig, ProcessorState, TrackedMessage,
};
pub use recovery::{RecoveryConfig, RecoveryTask};
pub use repository::{OutboxRepository, OutboxTableConfig};

/// Leader election configuration. Re-exported from `fc_common` — a single
/// unified type replacing the previous per-crate duplicates in fc-outbox and fc-standby.
pub use fc_common::LeaderElectionConfig;

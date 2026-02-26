pub mod repository;
pub mod buffer;
pub mod message_group_processor;
pub mod group_distributor;
pub mod recovery;
pub mod http_dispatcher;
pub mod enhanced_processor;

#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "mongo")]
pub mod mongo;

// Re-export key types
pub use buffer::{GlobalBuffer, GlobalBufferConfig, BufferFullError};
pub use message_group_processor::{
    MessageGroupProcessor, MessageGroupProcessorConfig, MessageDispatcher,
    BatchMessageDispatcher, BatchDispatchResult, BatchItemResult,
    DispatchResult, ProcessorState, TrackedMessage,
};
pub use group_distributor::{GroupDistributor, GroupDistributorConfig, DistributorStats};
pub use recovery::{RecoveryTask, RecoveryConfig};
pub use http_dispatcher::{
    HttpDispatcher, HttpDispatcherConfig, BatchRequest, BatchResponse,
    ItemStatus, OutboxDispatchResult,
};
pub use enhanced_processor::{EnhancedOutboxProcessor, EnhancedProcessorConfig, ProcessorMetrics};
pub use repository::{OutboxRepository, OutboxTableConfig, OutboxRepositoryExt};

/// Configuration for leader election in outbox processor
#[derive(Debug, Clone)]
pub struct LeaderElectionConfig {
    /// Whether leader election is enabled
    pub enabled: bool,
    /// Redis URL for leader election
    pub redis_url: String,
    /// Lock key for this processor
    pub lock_key: String,
    /// Lock TTL in seconds
    pub lock_ttl_seconds: u64,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_seconds: u64,
}

impl Default for LeaderElectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            redis_url: "redis://127.0.0.1:6379".to_string(),
            lock_key: "fc:outbox-processor-leader".to_string(),
            lock_ttl_seconds: 30,
            heartbeat_interval_seconds: 10,
        }
    }
}

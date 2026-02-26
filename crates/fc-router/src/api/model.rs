use serde::{Deserialize, Serialize};
use fc_common::PoolConfig;
use utoipa::ToSchema;

/// Request to publish a message
#[derive(Debug, Deserialize, ToSchema)]
pub struct PublishMessageRequest {
    /// Message payload (JSON)
    pub payload: serde_json::Value,
    /// Pool code for processing (default: DEFAULT)
    pub pool_code: Option<String>,
    /// Message group ID for FIFO ordering
    pub message_group_id: Option<String>,
    /// HTTP endpoint to call
    pub mediation_target: Option<String>,
}

/// Response after publishing a message
#[derive(Debug, Serialize, ToSchema)]
pub struct PublishMessageResponse {
    /// Generated message ID
    pub message_id: String,
    /// Status: ACCEPTED
    pub status: String,
}

/// Pool status response
#[derive(Debug, Serialize, ToSchema)]
pub struct PoolStatusResponse {
    /// Pool configuration
    pub pool: PoolConfig,
    /// Number of active workers
    pub active_workers: u32,
    /// Current queue size
    pub queue_size: u32,
}

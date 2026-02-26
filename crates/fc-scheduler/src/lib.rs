//! FlowCatalyst Dispatch Scheduler
//!
//! This crate provides the dispatch scheduler functionality:
//! - PendingJobPoller: Polls for PENDING dispatch jobs
//! - BlockOnErrorChecker: Checks for blocked message groups
//! - StaleQueuedJobPoller: Recovers jobs stuck in QUEUED status
//! - JobDispatcher: Dispatches jobs to the message queue

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bson::{doc, Document};
use chrono::{DateTime, Utc};
use bson::serde_helpers::chrono_datetime_as_bson_datetime;
use mongodb::{Collection, Database};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{error, info, warn};

pub mod auth;
pub mod dispatcher;
pub mod poller;
pub mod stale_recovery;

pub use auth::{AuthError, DispatchAuthService};
pub use dispatcher::JobDispatcher;
pub use poller::PendingJobPoller;
pub use stale_recovery::StaleQueuedJobPoller;

#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] mongodb::error::Error),
    #[error("Queue error: {0}")]
    QueueError(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchMode {
    Immediate,
    NextOnError,
    BlockOnError,
}

impl Default for DispatchMode {
    fn default() -> Self { Self::Immediate }
}

impl From<&str> for DispatchMode {
    fn from(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "NEXT_ON_ERROR" => Self::NextOnError,
            "BLOCK_ON_ERROR" => Self::BlockOnError,
            _ => Self::Immediate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchStatus {
    Pending, Queued, Processing, Completed, Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchJob {
    #[serde(rename = "_id")]
    pub id: String,
    pub message_group: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub status: DispatchStatus,
    pub mode: DispatchMode,
    pub target_url: Option<String>,
    pub payload: Option<Document>,
    #[serde(with = "chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default, with = "bson::serde_helpers::chrono_datetime_as_bson_datetime_optional")]
    pub queued_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub enabled: bool,
    pub poll_interval: Duration,
    pub batch_size: usize,
    pub stale_threshold: Duration,
    pub default_dispatch_mode: DispatchMode,
    pub default_pool_code: String,
    pub processing_endpoint: String,
    /// App key for HMAC auth token generation
    pub app_key: Option<String>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval: Duration::from_millis(100),
            batch_size: 100,
            stale_threshold: Duration::from_secs(15 * 60),
            default_dispatch_mode: DispatchMode::Immediate,
            default_pool_code: "default".to_string(),
            processing_endpoint: "http://localhost:8080/api/router/process".to_string(),
            app_key: None,
        }
    }
}

pub struct BlockOnErrorChecker {
    db: Database,
}

impl BlockOnErrorChecker {
    pub fn new(db: Database) -> Self { Self { db } }

    pub async fn get_blocked_groups(&self, groups: &HashSet<String>) -> Result<HashSet<String>, SchedulerError> {
        if groups.is_empty() { return Ok(HashSet::new()); }

        let collection: Collection<Document> = self.db.collection("dispatch_jobs");
        let filter = doc! {
            "messageGroup": { "$in": groups.iter().collect::<Vec<_>>() },
            "status": "ERROR"
        };

        let mut cursor = collection.find(filter).await?;
        let mut blocked = HashSet::new();

        while cursor.advance().await? {
            let doc = cursor.deserialize_current()?;
            if let Some(group) = doc.get_str("messageGroup").ok() {
                blocked.insert(group.to_string());
            }
        }
        Ok(blocked)
    }
}

#[async_trait]
pub trait QueuePublisher: Send + Sync {
    async fn publish(&self, message: QueueMessage) -> Result<(), SchedulerError>;
    fn is_healthy(&self) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMessage {
    pub id: String,
    pub message_group: Option<String>,
    pub deduplication_id: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePointer {
    pub job_id: String,
    pub dispatch_pool_id: String,
    pub auth_token: String,
    pub mediation_type: String,
    pub processing_endpoint: String,
    pub message_group: Option<String>,
    pub batch_id: Option<String>,
}

pub struct DispatchScheduler {
    config: SchedulerConfig,
    poller: PendingJobPoller,
    stale_poller: StaleQueuedJobPoller,
    running: Arc<RwLock<bool>>,
}

impl DispatchScheduler {
    pub fn new(config: SchedulerConfig, db: Database, queue_publisher: Arc<dyn QueuePublisher>) -> Self {
        let auth_service = DispatchAuthService::new(config.app_key.clone());
        let poller = PendingJobPoller::new(config.clone(), db.clone(), queue_publisher.clone(), auth_service);
        let stale_poller = StaleQueuedJobPoller::new(config.clone(), db);
        Self { config, poller, stale_poller, running: Arc::new(RwLock::new(false)) }
    }

    pub async fn start(&self) {
        if !self.config.enabled {
            info!("Dispatch scheduler is disabled");
            return;
        }

        let mut running = self.running.write().await;
        if *running {
            warn!("Scheduler already running");
            return;
        }
        *running = true;
        drop(running);

        info!(poll_interval_ms = self.config.poll_interval.as_millis(), batch_size = self.config.batch_size, "Starting dispatch scheduler");

        let poller = self.poller.clone();
        let poll_interval = self.config.poll_interval;
        let running_clone = self.running.clone();

        tokio::spawn(async move {
            let mut interval = interval(poll_interval);
            loop {
                interval.tick().await;
                if !*running_clone.read().await { break; }
                if let Err(e) = poller.poll().await {
                    error!(error = %e, "Error in pending job poller");
                }
            }
        });

        let stale_poller = self.stale_poller.clone();
        let running_clone2 = self.running.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                if !*running_clone2.read().await { break; }
                if let Err(e) = stale_poller.recover_stale_jobs().await {
                    error!(error = %e, "Error in stale job recovery");
                }
            }
        });
    }

    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
        info!("Dispatch scheduler stopped");
    }

    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }
}

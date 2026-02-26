//! Job dispatcher for sending jobs to the message queue

use std::sync::Arc;
use bson::doc;
use mongodb::{Collection, Database};
use tracing::{debug, error, warn};

use crate::{MessagePointer, QueueMessage, QueuePublisher, SchedulerConfig, SchedulerError};
use crate::auth::DispatchAuthService;

/// Job dispatcher that sends dispatch jobs to the message queue
pub struct JobDispatcher {
    config: SchedulerConfig,
    db: Database,
    queue_publisher: Arc<dyn QueuePublisher>,
    auth_service: DispatchAuthService,
}

impl JobDispatcher {
    /// Create a new job dispatcher
    pub fn new(
        config: SchedulerConfig,
        db: Database,
        queue_publisher: Arc<dyn QueuePublisher>,
        auth_service: DispatchAuthService,
    ) -> Self {
        Self {
            config,
            db,
            queue_publisher,
            auth_service,
        }
    }

    /// Create dispatcher with default auth service (for backward compatibility)
    pub fn new_with_app_key(
        config: SchedulerConfig,
        db: Database,
        queue_publisher: Arc<dyn QueuePublisher>,
        app_key: Option<String>,
    ) -> Self {
        Self::new(config, db, queue_publisher, DispatchAuthService::new(app_key))
    }

    /// Dispatch a job to the message queue
    pub async fn dispatch(&self, job_id: &str) -> Result<bool, SchedulerError> {
        let collection: Collection<bson::Document> = self.db.collection("dispatch_jobs");

        let filter = doc! { "_id": job_id };
        let Some(doc) = collection.find_one(filter.clone()).await? else {
            warn!(job_id = %job_id, "Job not found");
            return Ok(false);
        };

        let message_group = doc.get_str("messageGroup").ok().map(|s| s.to_string());
        let dispatch_pool_id = doc.get_str("dispatchPoolId").ok().map(|s| s.to_string());

        // Generate HMAC auth token
        let auth_token = match self.auth_service.generate_auth_token(job_id) {
            Ok(token) => token,
            Err(e) => {
                warn!(job_id = %job_id, error = %e, "Failed to generate auth token, using fallback");
                // Fallback for development/unconfigured environments
                format!("dev_{}", job_id)
            }
        };

        let pointer = MessagePointer {
            job_id: job_id.to_string(),
            dispatch_pool_id: dispatch_pool_id.unwrap_or_else(|| self.config.default_pool_code.clone()),
            auth_token,
            mediation_type: "HTTP".to_string(),
            processing_endpoint: self.config.processing_endpoint.clone(),
            message_group: message_group.clone(),
            batch_id: None,
        };

        let message = QueueMessage {
            id: job_id.to_string(),
            message_group,
            deduplication_id: job_id.to_string(),
            body: serde_json::to_string(&pointer)?,
        };

        // Record metrics
        metrics::counter!("scheduler.jobs.dispatched_total").increment(1);

        match self.queue_publisher.publish(message).await {
            Ok(_) => {
                let update = doc! {
                    "$set": {
                        "status": "QUEUED",
                        "queuedAt": bson::DateTime::now(),
                        "updatedAt": bson::DateTime::now()
                    }
                };
                collection.update_one(filter, update).await?;
                debug!(job_id = %job_id, "Job dispatched successfully");
                metrics::counter!("scheduler.jobs.queued_total").increment(1);
                Ok(true)
            }
            Err(e) => {
                error!(job_id = %job_id, error = %e, "Failed to dispatch job");
                metrics::counter!("scheduler.jobs.dispatch_errors_total").increment(1);
                Ok(false)
            }
        }
    }

    /// Check if the queue publisher is healthy
    pub fn is_healthy(&self) -> bool {
        self.queue_publisher.is_healthy()
    }

    /// Check if auth service is configured
    pub fn is_auth_configured(&self) -> bool {
        self.auth_service.is_configured()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatcher_auth_configured() {
        // This would need a full integration test setup
        // Just verifying the structure compiles correctly
        let auth = DispatchAuthService::new(Some("test-key".to_string()));
        assert!(auth.is_configured());
    }
}

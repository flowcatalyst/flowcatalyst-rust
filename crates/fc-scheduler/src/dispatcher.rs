//! Job dispatcher for sending individual jobs to the message queue

use std::sync::Arc;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
};
use tracing::{debug, error, warn};

use crate::{DispatchJob, MessagePointer, QueueMessage, QueuePublisher, SchedulerConfig, SchedulerError};
use crate::auth::DispatchAuthService;

/// Job dispatcher that sends dispatch jobs to the message queue
pub struct JobDispatcher {
    config: SchedulerConfig,
    db: DatabaseConnection,
    queue_publisher: Arc<dyn QueuePublisher>,
    auth_service: DispatchAuthService,
}

impl JobDispatcher {
    /// Create a new job dispatcher
    pub fn new(
        config: SchedulerConfig,
        db: DatabaseConnection,
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
        db: DatabaseConnection,
        queue_publisher: Arc<dyn QueuePublisher>,
        app_key: Option<String>,
    ) -> Self {
        Self::new(config, db, queue_publisher, DispatchAuthService::new(app_key))
    }

    /// Dispatch a job to the message queue
    pub async fn dispatch(&self, job_id: &str) -> Result<bool, SchedulerError> {
        let sql = "SELECT id, message_group, dispatch_pool_id, status, mode, target_url, \
                    payload, sequence, created_at, updated_at, queued_at, last_error \
                    FROM msg_dispatch_jobs WHERE id = $1";

        let jobs = DispatchJob::find_by_statement(
            Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                vec![sea_orm::Value::from(job_id.to_string())],
            ),
        )
        .all(&self.db)
        .await?;

        let Some(job) = jobs.into_iter().next() else {
            warn!(job_id = %job_id, "Job not found");
            return Ok(false);
        };

        // Generate HMAC auth token
        let auth_token = match self.auth_service.generate_auth_token(job_id) {
            Ok(token) => token,
            Err(e) => {
                warn!(job_id = %job_id, error = %e, "Failed to generate auth token, using fallback");
                format!("dev_{}", job_id)
            }
        };

        let pointer = MessagePointer {
            job_id: job_id.to_string(),
            dispatch_pool_id: job.dispatch_pool_id.unwrap_or_else(|| self.config.default_pool_code.clone()),
            auth_token,
            mediation_type: "HTTP".to_string(),
            processing_endpoint: self.config.processing_endpoint.clone(),
            message_group: job.message_group.clone(),
            batch_id: None,
        };

        let message = QueueMessage {
            id: job_id.to_string(),
            message_group: job.message_group,
            deduplication_id: job_id.to_string(),
            body: serde_json::to_string(&pointer)?,
        };

        metrics::counter!("scheduler.jobs.dispatched_total").increment(1);

        match self.queue_publisher.publish(message).await {
            Ok(_) => {
                let update_sql = "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() WHERE id = $1";
                self.db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    update_sql,
                    vec![sea_orm::Value::from(job_id.to_string())],
                )).await?;
                debug!(job_id = %job_id, "Job dispatched successfully");
                metrics::counter!("scheduler.jobs.queued_total").increment(1);
                Ok(true)
            }
            Err(e) => {
                let error_msg = format!("{}", e);

                // Handle deduplication — still mark as QUEUED
                if error_msg.contains("Deduplicated") || error_msg.contains("deduplicated") {
                    let update_sql = "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() WHERE id = $1";
                    self.db.execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        update_sql,
                        vec![sea_orm::Value::from(job_id.to_string())],
                    )).await?;
                    debug!(job_id = %job_id, "Job was deduplicated (already dispatched)");
                    return Ok(true);
                }

                error!(job_id = %job_id, error = %error_msg, "Failed to dispatch job");
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
        let auth = DispatchAuthService::new(Some("test-key".to_string()));
        assert!(auth.is_configured());
    }
}

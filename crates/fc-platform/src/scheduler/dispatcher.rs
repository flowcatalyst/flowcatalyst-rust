//! Job dispatcher for sending individual jobs to the message queue

use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, error, warn};

use crate::scheduler::{SchedulerConfig, SchedulerError, SchedulerJobRow};

/// Job dispatcher that sends dispatch jobs to the message queue
pub struct JobDispatcher {
    config: SchedulerConfig,
    pool: PgPool,
    queue_publisher: Arc<dyn fc_queue::QueuePublisher>,
}

impl JobDispatcher {
    pub fn new(
        config: SchedulerConfig,
        pool: PgPool,
        queue_publisher: Arc<dyn fc_queue::QueuePublisher>,
    ) -> Self {
        Self {
            config,
            pool,
            queue_publisher,
        }
    }

    /// Dispatch a job to the message queue
    pub async fn dispatch(&self, job_id: &str) -> Result<bool, SchedulerError> {
        let sql = "SELECT id, message_group, dispatch_pool_id, status, mode, target_url, \
                    payload, sequence, created_at, updated_at, queued_at, last_error, subscription_id \
                    FROM msg_dispatch_jobs WHERE id = $1";

        let job = sqlx::query_as::<_, SchedulerJobRow>(sql)
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await?;

        let Some(job) = job else {
            warn!(job_id = %job_id, "Job not found");
            return Ok(false);
        };

        // Build fc_common::Message directly — no MessagePointer intermediary
        let message = fc_common::Message {
            id: job_id.to_string(),
            pool_code: job
                .dispatch_pool_id
                .clone()
                .unwrap_or_else(|| self.config.default_pool_code.clone()),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: self.config.processing_endpoint.clone(),
            message_group_id: job.message_group.clone(),
            high_priority: false,
            dispatch_mode: job.dispatch_mode(),
            dispatch_mode_specified: true,
        };

        metrics::counter!("scheduler.jobs.dispatched_total").increment(1);

        let update_sql = "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() WHERE id = $1 AND created_at = $2";
        match self.queue_publisher.publish(message).await {
            Ok(_) => {
                sqlx::query(update_sql)
                    .bind(job_id)
                    .bind(job.created_at)
                    .execute(&self.pool)
                    .await?;
                debug!(job_id = %job_id, "Job dispatched successfully");
                metrics::counter!("scheduler.jobs.queued_total").increment(1);
                Ok(true)
            }
            Err(e) => {
                let error_msg = format!("{}", e);

                if error_msg.contains("Deduplicated") || error_msg.contains("deduplicated") {
                    sqlx::query(update_sql)
                        .bind(job_id)
                        .bind(job.created_at)
                        .execute(&self.pool)
                        .await?;
                    debug!(job_id = %job_id, "Job was deduplicated (already dispatched)");
                    return Ok(true);
                }

                error!(job_id = %job_id, error = %error_msg, "Failed to dispatch job");
                metrics::counter!("scheduler.jobs.dispatch_errors_total").increment(1);
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fc_common::DispatchMode;

    fn make_job(id: &str, pool: Option<&str>, group: Option<&str>, mode: &str) -> SchedulerJobRow {
        let now = Utc::now();
        SchedulerJobRow {
            id: id.to_string(),
            message_group: group.map(|s| s.to_string()),
            dispatch_pool_id: pool.map(|s| s.to_string()),
            status: "PENDING".to_string(),
            mode: mode.to_string(),
            target_url: "http://target.example.com/webhook".to_string(),
            payload: Some(r#"{"event":"order.created"}"#.to_string()),
            sequence: 1,
            created_at: now,
            updated_at: now,
            queued_at: None,
            last_error: None,
            subscription_id: Some("sub_123".to_string()),
        }
    }

    // Message construction tests (replaces old MessagePointer tests)

    #[test]
    fn message_uses_job_pool_when_present() {
        let config = SchedulerConfig::default();
        let job = make_job("job_1", Some("MY-POOL"), Some("grp_1"), "IMMEDIATE");

        let message = fc_common::Message {
            id: job.id.clone(),
            pool_code: job
                .dispatch_pool_id
                .clone()
                .unwrap_or_else(|| config.default_pool_code.clone()),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: config.processing_endpoint.clone(),
            message_group_id: job.message_group.clone(),
            high_priority: false,
            dispatch_mode: job.dispatch_mode(),
            dispatch_mode_specified: true,
        };

        assert_eq!(message.pool_code, "MY-POOL");
        assert_eq!(message.message_group_id, Some("grp_1".to_string()));
        assert_eq!(message.dispatch_mode, DispatchMode::Immediate);
    }

    #[test]
    fn message_falls_back_to_default_pool() {
        let config = SchedulerConfig::default();
        let job = make_job("job_2", None, None, "BLOCK_ON_ERROR");

        let message = fc_common::Message {
            id: job.id.clone(),
            pool_code: job
                .dispatch_pool_id
                .clone()
                .unwrap_or_else(|| config.default_pool_code.clone()),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: config.processing_endpoint.clone(),
            message_group_id: job.message_group.clone(),
            high_priority: false,
            dispatch_mode: job.dispatch_mode(),
            dispatch_mode_specified: true,
        };

        assert_eq!(message.pool_code, "DISPATCH-POOL");
        assert!(message.message_group_id.is_none());
        assert_eq!(message.dispatch_mode, DispatchMode::BlockOnError);
    }

    #[test]
    fn message_mediation_target_comes_from_config() {
        let config = SchedulerConfig {
            processing_endpoint: "https://custom.host/dispatch".to_string(),
            ..SchedulerConfig::default()
        };
        let job = make_job("job_3", None, None, "IMMEDIATE");

        let message = fc_common::Message {
            id: job.id.clone(),
            pool_code: job
                .dispatch_pool_id
                .clone()
                .unwrap_or_else(|| config.default_pool_code.clone()),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: config.processing_endpoint.clone(),
            message_group_id: job.message_group.clone(),
            high_priority: false,
            dispatch_mode: job.dispatch_mode(),
            dispatch_mode_specified: true,
        };

        assert_eq!(message.mediation_target, "https://custom.host/dispatch");
    }

    #[test]
    fn message_serializes_to_camel_case() {
        let message = fc_common::Message {
            id: "job_x".to_string(),
            pool_code: "POOL-A".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: "http://localhost:8080/api/dispatch/process".to_string(),
            message_group_id: Some("grp_1".to_string()),
            high_priority: false,
            dispatch_mode: DispatchMode::NextOnError,
            dispatch_mode_specified: true,
        };

        let json = serde_json::to_string(&message).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(v["id"], "job_x");
        assert_eq!(v["poolCode"], "POOL-A");
        assert_eq!(v["messageGroupId"], "grp_1");
        assert_eq!(
            v["mediationTarget"],
            "http://localhost:8080/api/dispatch/process"
        );
        assert_eq!(v["dispatchMode"], "NEXT_ON_ERROR");
    }

    // Deduplication detection tests

    #[test]
    fn deduplication_detection_matches_capitalized() {
        let error_msg = "Message was Deduplicated by SQS";
        assert!(error_msg.contains("Deduplicated") || error_msg.contains("deduplicated"));
    }

    #[test]
    fn deduplication_detection_matches_lowercase() {
        let error_msg = "message deduplicated";
        assert!(error_msg.contains("Deduplicated") || error_msg.contains("deduplicated"));
    }

    #[test]
    fn deduplication_detection_does_not_match_unrelated_errors() {
        let error_msg = "connection refused";
        assert!(!(error_msg.contains("Deduplicated") || error_msg.contains("deduplicated")));
    }

    // DispatchJob → dispatch mode mapping tests

    #[test]
    fn dispatch_job_mode_variants() {
        let immediate = make_job("j1", None, None, "IMMEDIATE");
        assert_eq!(immediate.dispatch_mode(), DispatchMode::Immediate);

        let next = make_job("j2", None, None, "NEXT_ON_ERROR");
        assert_eq!(next.dispatch_mode(), DispatchMode::NextOnError);

        let block = make_job("j3", None, None, "BLOCK_ON_ERROR");
        assert_eq!(block.dispatch_mode(), DispatchMode::BlockOnError);
    }

    #[test]
    fn dispatch_job_unknown_mode_defaults_to_next_on_error() {
        // Ledger A-09/X-01: unspecified/unrecognised dispatch mode ⇒ NEXT_ON_ERROR.
        let job = make_job("j_unk", None, None, "SOME_RANDOM_VALUE");
        assert_eq!(job.dispatch_mode(), DispatchMode::NextOnError);
    }

    #[test]
    fn dispatch_job_mode_case_insensitive() {
        let job = make_job("j_ci", None, None, "block_on_error");
        assert_eq!(job.dispatch_mode(), DispatchMode::BlockOnError);
    }
}

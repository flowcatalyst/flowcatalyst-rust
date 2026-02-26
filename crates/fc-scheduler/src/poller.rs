//! Pending job poller

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bson::{doc, Document};
use mongodb::{Collection, Database, options::FindOptions};
use tracing::{debug, trace, warn};

use crate::{
    BlockOnErrorChecker, DispatchJob, DispatchMode, DispatchStatus,
    MessagePointer, QueueMessage, QueuePublisher, SchedulerConfig, SchedulerError,
};
use crate::auth::DispatchAuthService;

const DEFAULT_MESSAGE_GROUP: &str = "default";

#[derive(Clone)]
pub struct PendingJobPoller {
    config: SchedulerConfig,
    db: Database,
    block_checker: Arc<BlockOnErrorChecker>,
    queue_publisher: Arc<dyn QueuePublisher>,
    auth_service: DispatchAuthService,
}

impl PendingJobPoller {
    pub fn new(
        config: SchedulerConfig,
        db: Database,
        queue_publisher: Arc<dyn QueuePublisher>,
        auth_service: DispatchAuthService,
    ) -> Self {
        let block_checker = Arc::new(BlockOnErrorChecker::new(db.clone()));
        Self {
            config,
            db,
            block_checker,
            queue_publisher,
            auth_service,
        }
    }

    /// Create poller with app key for backward compatibility
    pub fn new_with_app_key(
        config: SchedulerConfig,
        db: Database,
        queue_publisher: Arc<dyn QueuePublisher>,
        app_key: Option<String>,
    ) -> Self {
        Self::new(config, db, queue_publisher, DispatchAuthService::new(app_key))
    }

    pub async fn poll(&self) -> Result<(), SchedulerError> {
        let pending_jobs = self.find_pending_jobs().await?;
        if pending_jobs.is_empty() {
            trace!("No pending jobs found");
            return Ok(());
        }

        debug!(count = pending_jobs.len(), "Found pending jobs to process");
        metrics::gauge!("scheduler.pending_jobs").set(pending_jobs.len() as f64);

        let jobs_by_group = self.group_by_message_group(pending_jobs);
        let groups: HashSet<String> = jobs_by_group.keys().cloned().collect();
        let blocked_groups = self.block_checker.get_blocked_groups(&groups).await?;

        metrics::gauge!("scheduler.blocked_groups").set(blocked_groups.len() as f64);

        for (group, jobs) in jobs_by_group {
            if blocked_groups.contains(&group) {
                debug!(group = %group, count = jobs.len(), "Message group blocked, skipping");
                metrics::counter!("scheduler.jobs.blocked_total").increment(jobs.len() as u64);
                continue;
            }

            let dispatchable = self.filter_by_dispatch_mode(&group, jobs, &blocked_groups);
            if !dispatchable.is_empty() {
                debug!(group = %group, count = dispatchable.len(), "Dispatching jobs");
                self.dispatch_jobs(dispatchable).await?;
            }
        }
        Ok(())
    }

    async fn find_pending_jobs(&self) -> Result<Vec<DispatchJob>, SchedulerError> {
        let collection: Collection<Document> = self.db.collection("dispatch_jobs");
        let filter = doc! { "status": "PENDING" };
        let options = FindOptions::builder().limit(self.config.batch_size as i64).build();

        let mut cursor = collection.find(filter).with_options(options).await?;
        let mut jobs = Vec::new();
        while cursor.advance().await? {
            let doc = cursor.deserialize_current()?;
            if let Ok(job) = self.document_to_job(doc) {
                jobs.push(job);
            }
        }
        Ok(jobs)
    }

    fn document_to_job(&self, doc: Document) -> Result<DispatchJob, SchedulerError> {
        let id = doc.get_str("_id").unwrap_or_default().to_string();
        let message_group = doc.get_str("messageGroup").ok().map(|s| s.to_string());
        let dispatch_pool_id = doc.get_str("dispatchPoolId").ok().map(|s| s.to_string());

        let status = match doc.get_str("status").unwrap_or("PENDING") {
            "QUEUED" => DispatchStatus::Queued,
            "PROCESSING" => DispatchStatus::Processing,
            "COMPLETED" => DispatchStatus::Completed,
            "ERROR" => DispatchStatus::Error,
            _ => DispatchStatus::Pending,
        };

        let mode = DispatchMode::from(doc.get_str("mode").unwrap_or("IMMEDIATE"));
        let target_url = doc.get_str("targetUrl").ok().map(|s| s.to_string());
        let payload = doc.get_document("payload").ok().cloned();
        let error_message = doc.get_str("errorMessage").ok().map(|s| s.to_string());

        let created_at = doc.get_datetime("createdAt")
            .map(|d| d.to_chrono())
            .unwrap_or_else(|_| chrono::Utc::now());
        let updated_at = doc.get_datetime("updatedAt").ok().map(|d| d.to_chrono());
        let queued_at = doc.get_datetime("queuedAt").ok().map(|d| d.to_chrono());

        Ok(DispatchJob {
            id, message_group, dispatch_pool_id, status, mode,
            target_url, payload, created_at, updated_at, queued_at, error_message,
        })
    }

    fn group_by_message_group(&self, jobs: Vec<DispatchJob>) -> HashMap<String, Vec<DispatchJob>> {
        let mut grouped: HashMap<String, Vec<DispatchJob>> = HashMap::new();
        for job in jobs {
            let group = job.message_group.clone().unwrap_or_else(|| DEFAULT_MESSAGE_GROUP.to_string());
            grouped.entry(group).or_default().push(job);
        }
        grouped
    }

    fn filter_by_dispatch_mode(&self, group: &str, jobs: Vec<DispatchJob>, blocked_groups: &HashSet<String>) -> Vec<DispatchJob> {
        jobs.into_iter()
            .filter(|job| match job.mode {
                DispatchMode::Immediate => true,
                DispatchMode::NextOnError | DispatchMode::BlockOnError => !blocked_groups.contains(group),
            })
            .collect()
    }

    async fn dispatch_jobs(&self, jobs: Vec<DispatchJob>) -> Result<(), SchedulerError> {
        let collection: Collection<Document> = self.db.collection("dispatch_jobs");

        for job in jobs {
            // Generate HMAC auth token
            let auth_token = match self.auth_service.generate_auth_token(&job.id) {
                Ok(token) => token,
                Err(e) => {
                    warn!(job_id = %job.id, error = %e, "Failed to generate auth token, using fallback");
                    format!("dev_{}", job.id)
                }
            };

            let pointer = MessagePointer {
                job_id: job.id.clone(),
                dispatch_pool_id: job.dispatch_pool_id.clone().unwrap_or_else(|| self.config.default_pool_code.clone()),
                auth_token,
                mediation_type: "HTTP".to_string(),
                processing_endpoint: self.config.processing_endpoint.clone(),
                message_group: job.message_group.clone(),
                batch_id: None,
            };

            let message = QueueMessage {
                id: job.id.clone(),
                message_group: job.message_group.clone(),
                deduplication_id: job.id.clone(),
                body: serde_json::to_string(&pointer)?,
            };

            metrics::counter!("scheduler.jobs.dispatched_total").increment(1);

            match self.queue_publisher.publish(message).await {
                Ok(_) => {
                    let filter = doc! { "_id": &job.id };
                    let update = doc! {
                        "$set": {
                            "status": "QUEUED",
                            "queuedAt": bson::DateTime::now(),
                            "updatedAt": bson::DateTime::now()
                        }
                    };
                    collection.update_one(filter, update).await?;
                    debug!(job_id = %job.id, "Job dispatched");
                    metrics::counter!("scheduler.jobs.queued_total").increment(1);
                }
                Err(e) => {
                    warn!(job_id = %job.id, error = %e, "Failed to dispatch job");
                    metrics::counter!("scheduler.jobs.dispatch_errors_total").increment(1);
                }
            }
        }
        Ok(())
    }
}

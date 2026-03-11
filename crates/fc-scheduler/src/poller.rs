//! Pending job poller
//!
//! Polls for PENDING dispatch jobs, groups by message_group, filters by
//! dispatch mode and connection status, then submits to the MessageGroupDispatcher
//! for ordered, semaphore-limited dispatch.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use sea_orm::{
    DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
};
use tracing::{debug, trace};

use crate::{
    BlockOnErrorChecker, DispatchJob, DispatchMode,
    MessageGroupDispatcher, SchedulerConfig, SchedulerError,
};

const DEFAULT_MESSAGE_GROUP: &str = "default";

#[derive(Clone)]
pub struct PendingJobPoller {
    config: SchedulerConfig,
    db: DatabaseConnection,
    block_checker: Arc<BlockOnErrorChecker>,
    group_dispatcher: Arc<MessageGroupDispatcher>,
}

impl PendingJobPoller {
    pub fn new(
        config: SchedulerConfig,
        db: DatabaseConnection,
        group_dispatcher: Arc<MessageGroupDispatcher>,
    ) -> Self {
        let block_checker = Arc::new(BlockOnErrorChecker::new(db.clone()));
        Self {
            config,
            db,
            block_checker,
            group_dispatcher,
        }
    }

    pub async fn poll(&self) -> Result<(), SchedulerError> {
        let pending_jobs = self.find_pending_jobs().await?;
        if pending_jobs.is_empty() {
            trace!("No pending jobs found");
            return Ok(());
        }

        debug!(count = pending_jobs.len(), "Found pending jobs to process");
        metrics::gauge!("scheduler.pending_jobs").set(pending_jobs.len() as f64);

        // Group by message_group
        let jobs_by_group = Self::group_by_message_group(pending_jobs);
        let groups: HashSet<String> = jobs_by_group.keys().cloned().collect();

        // Batch check for blocked groups (single query)
        let blocked_groups = self.block_checker.get_blocked_groups(&groups).await?;
        metrics::gauge!("scheduler.blocked_groups").set(blocked_groups.len() as f64);

        // Process each group
        for (group, jobs) in jobs_by_group {
            if blocked_groups.contains(&group) {
                debug!(group = %group, count = jobs.len(), "Message group blocked, skipping");
                metrics::counter!("scheduler.jobs.blocked_total").increment(jobs.len() as u64);
                continue;
            }

            // Filter by dispatch mode
            let dispatchable = Self::filter_by_dispatch_mode(jobs, &blocked_groups);
            if !dispatchable.is_empty() {
                debug!(group = %group, count = dispatchable.len(), "Submitting jobs for message group");
                // Submit to the group dispatcher (1-in-flight per group, semaphore-limited)
                self.group_dispatcher.submit_jobs(&group, dispatchable);
            }
        }
        Ok(())
    }

    /// Query PENDING jobs with proper ordering: message_group, sequence, created_at.
    /// Optionally filters out jobs for paused connections.
    async fn find_pending_jobs(&self) -> Result<Vec<DispatchJob>, SchedulerError> {
        let sql = if self.config.connection_filter_enabled {
            // LEFT JOIN through subscription → connection, exclude jobs where connection is PAUSED
            "SELECT j.id, j.message_group, j.dispatch_pool_id, j.status, j.mode, j.target_url, \
                    j.payload, j.sequence, j.created_at, j.updated_at, j.queued_at, j.last_error \
             FROM msg_dispatch_jobs j \
             LEFT JOIN msg_subscriptions s ON j.subscription_id = s.id \
             LEFT JOIN msg_connections c ON s.connection_id = c.id \
             WHERE j.status = 'PENDING' \
               AND (c.id IS NULL OR c.status != 'PAUSED') \
             ORDER BY j.message_group ASC NULLS LAST, j.sequence ASC, j.created_at ASC \
             LIMIT $1"
        } else {
            "SELECT id, message_group, dispatch_pool_id, status, mode, target_url, \
                    payload, sequence, created_at, updated_at, queued_at, last_error \
             FROM msg_dispatch_jobs \
             WHERE status = 'PENDING' \
             ORDER BY message_group ASC NULLS LAST, sequence ASC, created_at ASC \
             LIMIT $1"
        };

        let jobs = DispatchJob::find_by_statement(
            Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                vec![sea_orm::Value::from(self.config.batch_size as i64)],
            ),
        )
        .all(&self.db)
        .await?;

        Ok(jobs)
    }

    fn group_by_message_group(jobs: Vec<DispatchJob>) -> HashMap<String, Vec<DispatchJob>> {
        let mut grouped: HashMap<String, Vec<DispatchJob>> = HashMap::new();
        for job in jobs {
            let group = job.message_group.clone().unwrap_or_else(|| DEFAULT_MESSAGE_GROUP.to_string());
            grouped.entry(group).or_default().push(job);
        }
        grouped
    }

    fn filter_by_dispatch_mode(jobs: Vec<DispatchJob>, blocked_groups: &HashSet<String>) -> Vec<DispatchJob> {
        jobs.into_iter()
            .filter(|job| {
                let group = job.message_group.as_deref().unwrap_or(DEFAULT_MESSAGE_GROUP);
                match job.dispatch_mode() {
                    DispatchMode::Immediate => true,
                    DispatchMode::NextOnError | DispatchMode::BlockOnError => {
                        !blocked_groups.contains(group)
                    }
                }
            })
            .collect()
    }
}

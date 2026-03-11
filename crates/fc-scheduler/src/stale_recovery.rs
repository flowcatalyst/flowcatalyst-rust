//! Stale job recovery - finds jobs stuck in QUEUED status and resets them

use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
};
use tracing::{debug, info, warn};

use crate::{SchedulerConfig, SchedulerError};

/// Count result from SQL
#[derive(Debug, FromQueryResult)]
struct CountResult {
    pub count: i64,
}

#[derive(Clone)]
pub struct StaleQueuedJobPoller {
    config: SchedulerConfig,
    db: DatabaseConnection,
}

impl StaleQueuedJobPoller {
    pub fn new(config: SchedulerConfig, db: DatabaseConnection) -> Self {
        Self { config, db }
    }

    /// Recover jobs stuck in QUEUED status beyond the stale threshold
    pub async fn recover_stale_jobs(&self) -> Result<usize, SchedulerError> {
        let threshold = Utc::now() - chrono::Duration::from_std(self.config.stale_threshold)
            .unwrap_or_else(|_| chrono::Duration::minutes(15));

        let sql = "UPDATE msg_dispatch_jobs SET status = 'PENDING', queued_at = NULL, updated_at = NOW() \
                    WHERE status = 'QUEUED' AND queued_at < $1";

        let result = self.db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![sea_orm::Value::from(threshold)],
        )).await?;

        let count = result.rows_affected() as usize;

        // Record metrics
        metrics::counter!("scheduler.stale_jobs.recovered_total").increment(count as u64);
        metrics::gauge!("scheduler.stale_jobs.last_recovery_count").set(count as f64);

        if count > 0 {
            info!(count = count, threshold_mins = self.config.stale_threshold.as_secs() / 60, "Recovered stale QUEUED jobs");
        } else {
            debug!("No stale jobs to recover");
        }

        Ok(count)
    }

    /// Count jobs currently in QUEUED status (for monitoring)
    pub async fn count_queued_jobs(&self) -> Result<u64, SchedulerError> {
        let sql = "SELECT COUNT(*) as count FROM msg_dispatch_jobs WHERE status = 'QUEUED'";

        let result = CountResult::find_by_statement(
            Statement::from_string(DatabaseBackend::Postgres, sql),
        )
        .one(&self.db)
        .await?;

        let count = result.map(|r| r.count as u64).unwrap_or(0);
        metrics::gauge!("scheduler.queued_jobs_total").set(count as f64);
        Ok(count)
    }

    /// Count jobs that are approaching stale threshold (early warning)
    pub async fn count_near_stale_jobs(&self) -> Result<u64, SchedulerError> {
        let warning_threshold = Utc::now() - chrono::Duration::from_std(self.config.stale_threshold / 2)
            .unwrap_or_else(|_| chrono::Duration::minutes(7));

        let sql = "SELECT COUNT(*) as count FROM msg_dispatch_jobs WHERE status = 'QUEUED' AND queued_at < $1";

        let result = CountResult::find_by_statement(
            Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                vec![sea_orm::Value::from(warning_threshold)],
            ),
        )
        .one(&self.db)
        .await?;

        let count = result.map(|r| r.count as u64).unwrap_or(0);

        if count > 0 {
            warn!(count = count, "Jobs approaching stale threshold");
            metrics::gauge!("scheduler.near_stale_jobs").set(count as f64);
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_stale_threshold_conversion() {
        let config = SchedulerConfig {
            stale_threshold: Duration::from_secs(15 * 60),
            ..Default::default()
        };

        // Verify threshold is 15 minutes
        assert_eq!(config.stale_threshold.as_secs(), 15 * 60);
    }
}

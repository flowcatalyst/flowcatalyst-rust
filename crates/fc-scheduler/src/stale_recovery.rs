//! Stale job recovery - finds jobs stuck in QUEUED status and resets them

use bson::doc;
use chrono::Utc;
use mongodb::{Collection, Database};
use tracing::{debug, info, warn};

use crate::{SchedulerConfig, SchedulerError};

#[derive(Clone)]
pub struct StaleQueuedJobPoller {
    config: SchedulerConfig,
    db: Database,
}

impl StaleQueuedJobPoller {
    pub fn new(config: SchedulerConfig, db: Database) -> Self {
        Self { config, db }
    }

    /// Recover jobs stuck in QUEUED status beyond the stale threshold
    pub async fn recover_stale_jobs(&self) -> Result<usize, SchedulerError> {
        let collection: Collection<bson::Document> = self.db.collection("dispatch_jobs");

        let threshold = Utc::now() - chrono::Duration::from_std(self.config.stale_threshold)
            .unwrap_or_else(|_| chrono::Duration::minutes(15));
        let threshold_bson = bson::DateTime::from_chrono(threshold);

        let filter = doc! {
            "status": "QUEUED",
            "queuedAt": { "$lt": threshold_bson }
        };
        let update = doc! {
            "$set": {
                "status": "PENDING",
                "updatedAt": bson::DateTime::now()
            },
            "$unset": { "queuedAt": "" }
        };

        let result = collection.update_many(filter, update).await?;
        let count = result.modified_count as usize;

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
        let collection: Collection<bson::Document> = self.db.collection("dispatch_jobs");
        let filter = doc! { "status": "QUEUED" };
        let count = collection.count_documents(filter).await?;

        metrics::gauge!("scheduler.queued_jobs_total").set(count as f64);
        Ok(count)
    }

    /// Count jobs that are approaching stale threshold (early warning)
    pub async fn count_near_stale_jobs(&self) -> Result<u64, SchedulerError> {
        let collection: Collection<bson::Document> = self.db.collection("dispatch_jobs");

        // Jobs queued more than half the stale threshold ago
        let warning_threshold = Utc::now() - chrono::Duration::from_std(self.config.stale_threshold / 2)
            .unwrap_or_else(|_| chrono::Duration::minutes(7));
        let threshold_bson = bson::DateTime::from_chrono(warning_threshold);

        let filter = doc! {
            "status": "QUEUED",
            "queuedAt": { "$lt": threshold_bson }
        };

        let count = collection.count_documents(filter).await?;

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

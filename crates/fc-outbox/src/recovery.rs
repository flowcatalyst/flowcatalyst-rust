//! Crash recovery for stuck outbox items.
//!
//! A background task that periodically returns rows stuck IN_PROGRESS for too
//! long (their processor died mid-dispatch) to PENDING, as Go's recovery
//! ticker does. [`crate::EnhancedOutboxProcessor`] runs the same pass on its
//! own; this task is for callers driving the repository themselves.

use crate::repository::OutboxRepository;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{debug, error, info};

/// Configuration for the crash recovery task.
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// How often to check for stuck items. Default: 60 seconds.
    pub check_interval: Duration,
    /// How long an item can be in PROCESSING before it's considered stuck.
    /// Default: 5 minutes (300 seconds).
    pub stuck_timeout: Duration,
    /// Whether recovery is enabled. Default: true.
    pub enabled: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(60),
            stuck_timeout: Duration::from_secs(300), // 5 minutes
            enabled: true,
        }
    }
}

/// Background task that recovers stuck outbox items.
pub struct RecoveryTask {
    repository: Arc<dyn OutboxRepository>,
    config: RecoveryConfig,
}

impl RecoveryTask {
    pub fn new(repository: Arc<dyn OutboxRepository>, config: RecoveryConfig) -> Self {
        Self { repository, config }
    }

    /// Start the recovery task. This runs indefinitely until cancelled.
    pub async fn run(&self) {
        if !self.config.enabled {
            info!("Outbox recovery task is disabled");
            return;
        }

        info!(
            "Starting outbox recovery task (interval: {:?}, timeout: {:?})",
            self.config.check_interval, self.config.stuck_timeout
        );

        let mut ticker = interval(self.config.check_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;
            self.recover_once().await;
        }
    }

    /// Perform a single recovery check.
    pub async fn recover_once(&self) {
        debug!("Checking for stuck outbox items");
        match self
            .repository
            .recover_stuck(self.config.stuck_timeout)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    info!("Recovered {} stuck outbox items", count);
                }
            }
            Err(e) => {
                error!("Failed to recover stuck outbox items: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RecoveryConfig::default();
        assert_eq!(config.check_interval, Duration::from_secs(60));
        assert_eq!(config.stuck_timeout, Duration::from_secs(300));
        assert!(config.enabled);
    }
}

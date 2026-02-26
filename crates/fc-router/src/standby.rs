//! Standby-Aware Router Integration
//!
//! Provides active/standby high availability support for the message router.
//! When standby mode is enabled, only the leader instance processes messages.
//! Other instances remain in standby, ready to take over if the leader fails.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tracing::{info, warn, debug};

pub use fc_standby::{
    LeaderElection, LeaderElectionConfig, LeadershipStatus, StandbyGuard,
    StandbyError, Result as StandbyResult,
};

/// Configuration for standby-aware router operation
#[derive(Debug, Clone)]
pub struct StandbyRouterConfig {
    /// Enable standby mode
    pub enabled: bool,
    /// Redis URL for leader election
    pub redis_url: String,
    /// Lock key for leader election
    pub lock_key: String,
    /// Lock TTL in seconds
    pub lock_ttl_seconds: u64,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_seconds: u64,
    /// Instance ID (auto-generated if empty)
    pub instance_id: String,
}

impl Default for StandbyRouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            redis_url: "redis://127.0.0.1:6379".to_string(),
            lock_key: "fc:router:leader".to_string(),
            lock_ttl_seconds: 30,
            heartbeat_interval_seconds: 10,
            instance_id: String::new(),
        }
    }
}

impl StandbyRouterConfig {
    /// Create a new config with the given Redis URL
    pub fn new(redis_url: String) -> Self {
        Self {
            enabled: true,
            redis_url,
            ..Default::default()
        }
    }

    /// Convert to fc-standby's LeaderElectionConfig
    pub fn to_leader_config(&self) -> LeaderElectionConfig {
        LeaderElectionConfig {
            redis_url: self.redis_url.clone(),
            lock_key: self.lock_key.clone(),
            lock_ttl_seconds: self.lock_ttl_seconds,
            heartbeat_interval_seconds: self.heartbeat_interval_seconds,
            instance_id: if self.instance_id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                self.instance_id.clone()
            },
        }
    }
}

/// Wrapper that provides standby-aware processing capabilities
pub struct StandbyAwareProcessor {
    election: Arc<LeaderElection>,
    guard: StandbyGuard,
    /// Track if we were previously the leader (for logging transitions)
    was_leader: AtomicBool,
}

impl StandbyAwareProcessor {
    /// Create a new standby-aware processor
    pub async fn new(config: StandbyRouterConfig) -> StandbyResult<Self> {
        let leader_config = config.to_leader_config();
        let election = Arc::new(LeaderElection::new(leader_config).await?);
        let guard = StandbyGuard::new(election.clone());

        Ok(Self {
            election,
            guard,
            was_leader: AtomicBool::new(false),
        })
    }

    /// Start the leader election process
    pub async fn start(&self) -> StandbyResult<()> {
        info!("Starting standby-aware processor with leader election");
        self.election.clone().start().await
    }

    /// Check if this instance is currently the leader
    pub fn is_leader(&self) -> bool {
        self.election.is_leader()
    }

    /// Check if this instance should process messages
    pub fn should_process(&self) -> bool {
        self.guard.should_process()
    }

    /// Get current leadership status
    pub fn status(&self) -> LeadershipStatus {
        self.election.status()
    }

    /// Subscribe to leadership status changes
    pub fn subscribe(&self) -> watch::Receiver<LeadershipStatus> {
        self.election.subscribe()
    }

    /// Get the instance ID
    pub fn instance_id(&self) -> &str {
        self.election.instance_id()
    }

    /// Get the underlying StandbyGuard for use with async operations
    pub fn guard(&self) -> &StandbyGuard {
        &self.guard
    }

    /// Wait until this instance becomes the leader
    pub async fn wait_for_leadership(&self) {
        self.guard.wait_for_leadership().await
    }

    /// Log leadership transitions
    pub fn check_and_log_transition(&self) {
        let is_now_leader = self.is_leader();
        let was_previously_leader = self.was_leader.swap(is_now_leader, Ordering::SeqCst);

        if is_now_leader && !was_previously_leader {
            info!(
                instance_id = %self.instance_id(),
                "This instance became the LEADER - starting message processing"
            );
        } else if !is_now_leader && was_previously_leader {
            warn!(
                instance_id = %self.instance_id(),
                "This instance lost leadership - pausing message processing"
            );
        }
    }

    /// Shutdown the leader election
    pub async fn shutdown(&self) {
        info!(instance_id = %self.instance_id(), "Shutting down standby processor");
        self.election.shutdown().await;
    }
}

/// No-op standby processor for when standby mode is disabled
pub struct DisabledStandbyProcessor;

impl DisabledStandbyProcessor {
    pub fn new() -> Self {
        Self
    }

    pub fn is_leader(&self) -> bool {
        true // Always leader when standby is disabled
    }

    pub fn should_process(&self) -> bool {
        true // Always process when standby is disabled
    }

    pub fn status(&self) -> LeadershipStatus {
        LeadershipStatus::Leader
    }

    pub fn instance_id(&self) -> &str {
        "standalone"
    }

    pub fn check_and_log_transition(&self) {
        // No-op
    }

    pub async fn shutdown(&self) {
        // No-op
    }
}

/// Enum to handle both enabled and disabled standby modes
pub enum StandbyProcessor {
    Enabled(StandbyAwareProcessor),
    Disabled(DisabledStandbyProcessor),
}

impl StandbyProcessor {
    /// Create a new standby processor based on configuration
    pub async fn new(config: StandbyRouterConfig) -> Result<Self, StandbyError> {
        if config.enabled {
            let processor = StandbyAwareProcessor::new(config).await?;
            Ok(Self::Enabled(processor))
        } else {
            info!("Standby mode disabled - this instance will always be active");
            Ok(Self::Disabled(DisabledStandbyProcessor::new()))
        }
    }

    /// Start the processor (only does something for enabled mode)
    pub async fn start(&self) -> Result<(), StandbyError> {
        match self {
            Self::Enabled(processor) => processor.start().await,
            Self::Disabled(_) => Ok(()),
        }
    }

    /// Check if this instance is currently the leader
    pub fn is_leader(&self) -> bool {
        match self {
            Self::Enabled(processor) => processor.is_leader(),
            Self::Disabled(processor) => processor.is_leader(),
        }
    }

    /// Check if this instance should process messages
    pub fn should_process(&self) -> bool {
        match self {
            Self::Enabled(processor) => processor.should_process(),
            Self::Disabled(processor) => processor.should_process(),
        }
    }

    /// Get current leadership status
    pub fn status(&self) -> LeadershipStatus {
        match self {
            Self::Enabled(processor) => processor.status(),
            Self::Disabled(processor) => processor.status(),
        }
    }

    /// Get the instance ID
    pub fn instance_id(&self) -> &str {
        match self {
            Self::Enabled(processor) => processor.instance_id(),
            Self::Disabled(processor) => processor.instance_id(),
        }
    }

    /// Log leadership transitions
    pub fn check_and_log_transition(&self) {
        match self {
            Self::Enabled(processor) => processor.check_and_log_transition(),
            Self::Disabled(processor) => processor.check_and_log_transition(),
        }
    }

    /// Wait for leadership (returns immediately if disabled)
    pub async fn wait_for_leadership(&self) {
        match self {
            Self::Enabled(processor) => processor.wait_for_leadership().await,
            Self::Disabled(_) => {} // Immediate return
        }
    }

    /// Shutdown the processor
    pub async fn shutdown(&self) {
        match self {
            Self::Enabled(processor) => processor.shutdown().await,
            Self::Disabled(processor) => processor.shutdown().await,
        }
    }

    /// Check if standby mode is enabled
    pub fn is_standby_enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }
}

/// Spawn a task that monitors leadership status and logs transitions
pub fn spawn_leadership_monitor(
    processor: Arc<StandbyProcessor>,
    shutdown_tx: broadcast::Sender<()>,
) -> tokio::task::JoinHandle<()> {
    let mut shutdown_rx = shutdown_tx.subscribe();

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    processor.check_and_log_transition();

                    if let StandbyProcessor::Enabled(ref p) = *processor {
                        debug!(
                            instance_id = %p.instance_id(),
                            is_leader = p.is_leader(),
                            status = ?p.status(),
                            "Leadership status check"
                        );
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Leadership monitor shutting down");
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standby_config_defaults() {
        let config = StandbyRouterConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.lock_key, "fc:router:leader");
        assert_eq!(config.lock_ttl_seconds, 30);
    }

    #[test]
    fn test_disabled_processor_is_always_leader() {
        let processor = DisabledStandbyProcessor::new();
        assert!(processor.is_leader());
        assert!(processor.should_process());
        assert_eq!(processor.status(), LeadershipStatus::Leader);
    }

    #[tokio::test]
    async fn test_standby_processor_disabled_mode() {
        let config = StandbyRouterConfig::default(); // enabled = false
        let processor = StandbyProcessor::new(config).await.unwrap();

        assert!(!processor.is_standby_enabled());
        assert!(processor.is_leader());
        assert!(processor.should_process());
    }
}

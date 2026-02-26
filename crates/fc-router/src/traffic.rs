//! ALB Traffic Management
//!
//! Provides traffic routing strategies for managing ALB target group registration.
//! When standby mode is enabled, the leader registers with the ALB target group
//! to receive traffic, and deregisters when it loses leadership.
//!
//! # Strategies
//!
//! - `NoopTrafficStrategy`: No-op, always considers itself registered (default)
//! - `AwsAlbTrafficStrategy`: Manages AWS ALB target group registration (requires `alb` feature)

use std::sync::Arc;
#[cfg(feature = "alb")]
use std::sync::atomic::{AtomicBool, Ordering};
use async_trait::async_trait;
#[cfg(feature = "alb")]
use tracing::{info, warn, error, debug};
#[cfg(not(feature = "alb"))]
use tracing::{info, error, debug};

/// Errors that can occur during traffic management operations.
#[derive(Debug, thiserror::Error)]
pub enum TrafficError {
    /// An error occurred while communicating with AWS.
    #[error("AWS error: {0}")]
    Aws(String),
    /// Timed out waiting for target deregistration to complete.
    #[error("Timeout waiting for deregistration")]
    DeregistrationTimeout,
    /// A configuration error prevented the operation.
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Strategy for managing traffic routing (e.g., ALB target group registration).
///
/// Implementations control how an instance registers and deregisters itself
/// as a target for incoming traffic. This is used in conjunction with the
/// standby/leader election system to ensure only the active leader receives traffic.
#[async_trait]
pub trait TrafficStrategy: Send + Sync {
    /// Register this instance as a target to receive traffic.
    async fn register(&self) -> Result<(), TrafficError>;

    /// Deregister this instance, allowing connections to drain.
    async fn deregister(&self) -> Result<(), TrafficError>;

    /// Check if this instance is currently registered to receive traffic.
    fn is_registered(&self) -> bool;

    /// Get the name of this strategy type (e.g., "NONE", "AWS_ALB").
    fn strategy_type(&self) -> &str;
}

/// No-op traffic strategy for when traffic management is disabled.
///
/// Always considers itself registered and all operations succeed immediately.
/// This is the default strategy when no ALB integration is configured.
pub struct NoopTrafficStrategy;

#[async_trait]
impl TrafficStrategy for NoopTrafficStrategy {
    async fn register(&self) -> Result<(), TrafficError> {
        debug!("NoopTrafficStrategy: register (no-op)");
        Ok(())
    }

    async fn deregister(&self) -> Result<(), TrafficError> {
        debug!("NoopTrafficStrategy: deregister (no-op)");
        Ok(())
    }

    fn is_registered(&self) -> bool {
        true
    }

    fn strategy_type(&self) -> &str {
        "NONE"
    }
}

// ============================================================================
// AWS ALB Traffic Strategy (feature-gated)
// ============================================================================

/// Configuration for the AWS ALB traffic strategy.
#[cfg(feature = "alb")]
#[derive(Debug, Clone)]
pub struct AlbTrafficConfig {
    /// The ARN of the target group to register with.
    pub target_group_arn: String,
    /// The ID of this target (e.g., instance ID or IP address).
    pub target_id: String,
    /// The port on which this target receives traffic.
    pub target_port: i32,
    /// Maximum time in seconds to wait for deregistration to complete.
    /// After this timeout, the deregister call returns a timeout error.
    pub deregistration_delay_seconds: u64,
}

/// AWS ALB traffic strategy that manages target group registration.
///
/// Registers and deregisters this instance with an ALB target group.
/// During deregistration, polls target health until the target is no longer
/// in the "draining" state or a timeout is reached.
#[cfg(feature = "alb")]
pub struct AwsAlbTrafficStrategy {
    client: aws_sdk_elasticloadbalancingv2::Client,
    config: AlbTrafficConfig,
    registered: AtomicBool,
}

#[cfg(feature = "alb")]
impl AwsAlbTrafficStrategy {
    /// Create a new ALB traffic strategy.
    ///
    /// # Arguments
    ///
    /// * `config` - ALB target group configuration
    /// * `aws_config` - AWS SDK configuration (region, credentials, etc.)
    pub fn new(config: AlbTrafficConfig, aws_config: &aws_config::SdkConfig) -> Self {
        let client = aws_sdk_elasticloadbalancingv2::Client::new(aws_config);
        info!(
            target_group_arn = %config.target_group_arn,
            target_id = %config.target_id,
            target_port = config.target_port,
            "Created AwsAlbTrafficStrategy"
        );
        Self {
            client,
            config,
            registered: AtomicBool::new(false),
        }
    }

    /// Build a target description for AWS API calls.
    fn target_description(&self) -> aws_sdk_elasticloadbalancingv2::types::TargetDescription {
        aws_sdk_elasticloadbalancingv2::types::TargetDescription::builder()
            .id(&self.config.target_id)
            .port(self.config.target_port)
            .build()
    }

    /// Poll target health until the target is no longer draining or timeout is reached.
    async fn wait_for_deregistration(&self) -> Result<(), TrafficError> {
        use std::time::{Duration, Instant};

        let timeout = Duration::from_secs(self.config.deregistration_delay_seconds);
        let poll_interval = Duration::from_secs(5);
        let deadline = Instant::now() + timeout;

        info!(
            target_id = %self.config.target_id,
            timeout_secs = self.config.deregistration_delay_seconds,
            "Waiting for target deregistration to complete"
        );

        loop {
            if Instant::now() >= deadline {
                warn!(
                    target_id = %self.config.target_id,
                    "Deregistration wait timed out after {} seconds",
                    self.config.deregistration_delay_seconds
                );
                return Err(TrafficError::DeregistrationTimeout);
            }

            let health_result = self.client
                .describe_target_health()
                .target_group_arn(&self.config.target_group_arn)
                .targets(self.target_description())
                .send()
                .await
                .map_err(|e| TrafficError::Aws(format!("Failed to describe target health: {}", e)))?;

            let is_draining = health_result
                .target_health_descriptions()
                .iter()
                .any(|desc| {
                    desc.target_health()
                        .and_then(|h| h.state())
                        .map(|s| s.as_str() == "draining")
                        .unwrap_or(false)
                });

            if !is_draining {
                debug!(
                    target_id = %self.config.target_id,
                    "Target is no longer draining"
                );
                return Ok(());
            }

            debug!(
                target_id = %self.config.target_id,
                "Target still draining, waiting {} seconds before next check",
                poll_interval.as_secs()
            );

            tokio::time::sleep(poll_interval).await;
        }
    }
}

#[cfg(feature = "alb")]
#[async_trait]
impl TrafficStrategy for AwsAlbTrafficStrategy {
    async fn register(&self) -> Result<(), TrafficError> {
        info!(
            target_group_arn = %self.config.target_group_arn,
            target_id = %self.config.target_id,
            target_port = self.config.target_port,
            "Registering target with ALB target group"
        );

        self.client
            .register_targets()
            .target_group_arn(&self.config.target_group_arn)
            .targets(self.target_description())
            .send()
            .await
            .map_err(|e| TrafficError::Aws(format!("Failed to register target: {}", e)))?;

        self.registered.store(true, Ordering::SeqCst);
        info!(
            target_id = %self.config.target_id,
            "Target successfully registered with ALB"
        );
        Ok(())
    }

    async fn deregister(&self) -> Result<(), TrafficError> {
        info!(
            target_group_arn = %self.config.target_group_arn,
            target_id = %self.config.target_id,
            "Deregistering target from ALB target group"
        );

        self.client
            .deregister_targets()
            .target_group_arn(&self.config.target_group_arn)
            .targets(self.target_description())
            .send()
            .await
            .map_err(|e| TrafficError::Aws(format!("Failed to deregister target: {}", e)))?;

        info!(
            target_id = %self.config.target_id,
            "Target deregistered, waiting for drain to complete"
        );

        let drain_result = self.wait_for_deregistration().await;

        self.registered.store(false, Ordering::SeqCst);

        match drain_result {
            Ok(()) => {
                info!(
                    target_id = %self.config.target_id,
                    "Target fully deregistered from ALB"
                );
                Ok(())
            }
            Err(TrafficError::DeregistrationTimeout) => {
                // Mark as deregistered even on timeout -- the AWS-side deregistration
                // was already initiated; we just couldn't confirm drain completion.
                warn!(
                    target_id = %self.config.target_id,
                    "Deregistration timed out waiting for drain, proceeding anyway"
                );
                Err(TrafficError::DeregistrationTimeout)
            }
            Err(e) => Err(e),
        }
    }

    fn is_registered(&self) -> bool {
        self.registered.load(Ordering::SeqCst)
    }

    fn strategy_type(&self) -> &str {
        "AWS_ALB"
    }
}

// ============================================================================
// Traffic Watcher (watches leadership status and manages registration)
// ============================================================================

/// Spawn a background task that watches leadership status changes and
/// registers/deregisters the traffic strategy accordingly.
///
/// When the instance becomes the leader, it registers with the traffic target.
/// When it loses leadership, it deregisters. All errors are logged but never
/// cause the watcher to crash -- traffic management failures must not take
/// down the standby system.
///
/// # Arguments
///
/// * `strategy` - The traffic strategy to manage
/// * `status_rx` - A watch receiver for leadership status changes
///
/// # Returns
///
/// A `JoinHandle` for the spawned background task.
pub fn spawn_traffic_watcher(
    strategy: Arc<dyn TrafficStrategy>,
    mut status_rx: tokio::sync::watch::Receiver<fc_standby::LeadershipStatus>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            strategy_type = strategy.strategy_type(),
            "Traffic watcher started"
        );

        loop {
            // Wait for a status change
            if status_rx.changed().await.is_err() {
                // Sender dropped, shutting down
                info!("Traffic watcher: leadership status channel closed, shutting down");
                break;
            }

            let status = *status_rx.borrow_and_update();
            debug!(?status, "Traffic watcher: leadership status changed");

            match status {
                fc_standby::LeadershipStatus::Leader => {
                    info!("Traffic watcher: became leader, registering target");
                    if let Err(e) = strategy.register().await {
                        error!(
                            error = %e,
                            strategy_type = strategy.strategy_type(),
                            "Traffic watcher: failed to register target"
                        );
                    }
                }
                fc_standby::LeadershipStatus::Follower
                | fc_standby::LeadershipStatus::Unknown => {
                    info!(?status, "Traffic watcher: no longer leader, deregistering target");
                    if let Err(e) = strategy.deregister().await {
                        error!(
                            error = %e,
                            strategy_type = strategy.strategy_type(),
                            "Traffic watcher: failed to deregister target"
                        );
                    }
                }
            }
        }

        info!("Traffic watcher stopped");
    })
}

//! Standby-Aware Router Integration
//!
//! Provides active/standby high availability support for the message router.
//! When standby mode is enabled, only the leader instance processes messages.
//! Other instances remain in standby, ready to take over if the leader fails.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::manager::QueueManager;
use fc_common::diagnostics;
use fc_common::diagnostics::OnPanic;
pub use fc_standby::{
    LeaderElection, LeaderElectionConfig, LeadershipStatus, Result as StandbyResult, StandbyError,
    StandbyGuard,
};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::MissedTickBehavior;

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
            enabled: self.enabled,
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

/// Spawn a task that monitors leadership status, logs transitions, and
/// drives the [`QueueManager`](crate::manager::QueueManager)'s leadership
/// flag (R-26/R-34).
///
/// It pushes the election's current `is_leader()` value into
/// `manager.set_leader(..)` as soon as the election publishes a status change
/// (so a lost lock pauses intake at once, not at the next tick), and every 5s
/// regardless. The tick is a backstop for the one change nothing announces:
/// `is_leader()` also turns false when the lease ages out with the election
/// loop unable to say so (a hung Redis call), and the manager converges to
/// the true status even if a change were ever missed.
/// `QueueManager::set_leader` does the edge-detected logging and is the
/// single source of truth the consumer poll loop (pause/resume new intake)
/// and the config-reload handler (R-33, refuse on a non-leader instance)
/// both read. Losing leadership never cancels in-flight or buffered work —
/// see `QueueManager::set_leader`'s doc comment.
///
/// Only spawned when standby mode is enabled; with it disabled there is no
/// processor and the manager stays leader.
///
/// **Owns:** the `Arc<StandbyAwareProcessor>`, an `Arc<QueueManager>`, and a
/// [`CancellationToken`] (typically a child of the lifecycle manager's
/// shutdown token).
/// **Exits:** when `shutdown.cancelled()` resolves. `CancellationToken` is
/// level-triggered, so a token that was already cancelled *before* this
/// task was spawned still causes an immediate exit — unlike a broadcast
/// channel, where a late subscriber would never observe a signal sent
/// before it subscribed.
/// **Joined by:** the caller via the returned `JoinHandle`. Lifecycle
/// manager awaits all such handles during graceful shutdown.
pub fn spawn_leadership_monitor(
    processor: Arc<StandbyAwareProcessor>,
    manager: Arc<QueueManager>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    // Supervised: a panic is logged and the loop restarted (a dead monitor
    // would freeze the leader flag for the rest of the process's life).
    diagnostics::spawn_supervised("router.leadership_monitor", OnPanic::Restart, move || {
        let processor = processor.clone();
        let manager = manager.clone();
        let changes = processor.subscribe();
        ticker_loop(
            changes,
            move || {
                processor.check_and_log_transition();
                manager.set_leader(processor.is_leader());
                debug!(
                    instance_id = %processor.instance_id(),
                    is_leader = processor.is_leader(),
                    status = ?processor.status(),
                    "Leadership status check"
                );
            },
            shutdown.clone(),
        )
    })
}

/// The leadership monitor's loop: run `on_check` when `changes` reports a new
/// status, and every 5s, until `shutdown` is cancelled.
async fn ticker_loop(
    mut changes: watch::Receiver<LeadershipStatus>,
    mut on_check: impl FnMut() + Send + 'static,
    shutdown: CancellationToken,
) {
    let mut ticker = time::interval(Duration::from_secs(5));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // Once the election is gone its channel closes; `changed()` would then
    // return at once forever, so stop listening and keep the tick.
    let mut listening = true;

    loop {
        tokio::select! {
            _ = ticker.tick() => on_check(),
            changed = changes.changed(), if listening => match changed {
                Ok(()) => on_check(),
                Err(_) => listening = false,
            },
            _ = shutdown.cancelled() => {
                info!("Leadership monitor shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
fn spawn_ticker(
    changes: watch::Receiver<LeadershipStatus>,
    on_check: impl FnMut() + Send + 'static,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(ticker_loop(changes, on_check, shutdown))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use tokio::time;

    #[test]
    fn test_standby_config_defaults() {
        let config = StandbyRouterConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.lock_key, "fc:router:leader");
        assert_eq!(config.lock_ttl_seconds, 30);
    }

    /// A `CancellationToken` is level-triggered: a token cancelled *before*
    /// the task subscribes to it must still cause an immediate exit. This
    /// is exactly the case a `broadcast` channel could not handle (a
    /// receiver that subscribes after `send()` never observes the signal).
    #[tokio::test]
    async fn leadership_ticker_exits_immediately_on_already_cancelled_token() {
        let token = CancellationToken::new();
        token.cancel();

        let (_tx, rx) = watch::channel(LeadershipStatus::Unknown);
        let handle = spawn_ticker(rx, || {}, token);

        time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("leadership monitor should exit within 1s of an already-cancelled token")
            .expect("task should not panic");
    }

    /// A status change reaches the manager at once, not at the next 5s tick.
    #[tokio::test]
    async fn a_published_status_change_is_acted_on_without_waiting_for_the_tick() {
        let (tx, rx) = watch::channel(LeadershipStatus::Follower);
        let checks = Arc::new(AtomicU32::new(0));
        let token = CancellationToken::new();
        let counted = checks.clone();
        let handle = spawn_ticker(
            rx,
            move || {
                counted.fetch_add(1, Ordering::SeqCst);
            },
            token.clone(),
        );

        // The interval's first tick fires at once; let it pass.
        time::sleep(Duration::from_millis(100)).await;
        let before = checks.load(Ordering::SeqCst);

        tx.send(LeadershipStatus::Leader).unwrap();
        time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            checks.load(Ordering::SeqCst),
            before + 1,
            "the change ran one check, far inside the 5s tick"
        );

        token.cancel();
        handle.await.unwrap();
    }

    /// With the election gone (channel closed) the monitor must not spin on
    /// `changed()`, which would return an error at once for ever.
    #[tokio::test]
    async fn a_closed_status_channel_does_not_spin_the_monitor() {
        let (tx, rx) = watch::channel(LeadershipStatus::Follower);
        let checks = Arc::new(AtomicU32::new(0));
        let token = CancellationToken::new();
        let counted = checks.clone();
        let handle = spawn_ticker(
            rx,
            move || {
                counted.fetch_add(1, Ordering::SeqCst);
            },
            token.clone(),
        );
        drop(tx);
        time::sleep(Duration::from_millis(300)).await;
        assert!(
            checks.load(Ordering::SeqCst) <= 2,
            "only the initial tick, not a busy loop: {}",
            checks.load(Ordering::SeqCst)
        );
        token.cancel();
        handle.await.unwrap();
    }
}

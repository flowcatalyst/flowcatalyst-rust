//! Redis-based Leader Election
//!
//! Implements distributed leader election using Redis with:
//! - SET NX with expiry for atomic lock acquisition
//! - Periodic heartbeat to extend lease
//! - Automatic leader change on lease expiry
//! - Callback notifications for leadership changes
//! - Local demotion when Redis stops answering (see [`LeaderElection::is_leader`])

use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};
use tracing::{debug, error, info, warn};

use crate::error::{Result, StandbyError};

/// Leader election configuration. Re-exported from `fc_common` — a single
/// unified type replacing the previous per-crate duplicates in fc-outbox and fc-standby.
pub use fc_common::LeaderElectionConfig;

/// Leadership status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadershipStatus {
    /// This instance is the leader
    Leader,
    /// Another instance is the leader
    Follower,
    /// Leadership is unknown (election in progress)
    Unknown,
}

/// How long any one Redis command may take before it counts as a failure.
/// Go's go-redis client defaults to a 3s read/write timeout, and its
/// election demotes on any error; without a bound a hung Redis left
/// `is_leader` true while the lock expired and another instance took it.
pub const REDIS_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

/// Bound on establishing a Redis connection (go-redis `DialTimeout`).
pub const REDIS_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long after its last confirmed acquire/extend this instance may still
/// call itself leader: the lock TTL minus a safety margin (a fifth of the
/// TTL, at least one second). Past it the lock may already have expired in
/// Redis and been taken by another instance, so leadership is dropped
/// locally whether or not the election loop has managed to hear back from
/// Redis.
pub fn lease_validity(lock_ttl: Duration) -> Duration {
    let margin = (lock_ttl / 5).max(Duration::from_secs(1));
    lock_ttl.saturating_sub(margin)
}

/// Leader election manager
pub struct LeaderElection {
    config: LeaderElectionConfig,
    conn: ConnectionManager,
    is_leader: AtomicBool,
    /// When Redis last confirmed this instance holds the lock (a successful
    /// SET NX or extend). `is_leader()` answers false once this is older
    /// than [`lease_validity`], even if the election loop is stuck.
    last_confirmed: Mutex<Option<Instant>>,
    running: AtomicBool,
    shutdown_tx: broadcast::Sender<()>,
    status_tx: watch::Sender<LeadershipStatus>,
    status_rx: watch::Receiver<LeadershipStatus>,
}

impl LeaderElection {
    /// Create a new leader election manager
    pub async fn new(config: LeaderElectionConfig) -> Result<Self> {
        let client = redis::Client::open(config.redis_url.as_str())
            .map_err(|e| StandbyError::Connection(e.to_string()))?;

        let manager_config = ConnectionManagerConfig::new()
            .set_response_timeout(REDIS_RESPONSE_TIMEOUT)
            .set_connection_timeout(REDIS_CONNECT_TIMEOUT);
        let conn = ConnectionManager::new_with_config(client, manager_config).await?;
        let (shutdown_tx, _) = broadcast::channel(1);
        let (status_tx, status_rx) = watch::channel(LeadershipStatus::Unknown);

        Ok(Self {
            config,
            conn,
            is_leader: AtomicBool::new(false),
            last_confirmed: Mutex::new(None),
            running: AtomicBool::new(false),
            shutdown_tx,
            status_tx,
            status_rx,
        })
    }

    /// Check if this instance is currently the leader.
    ///
    /// True only while the last confirmation from Redis is younger than
    /// [`lease_validity`] of the lock TTL. The election loop demotes on any
    /// Redis error (as Go's does), but a loop that cannot reach Redis at all
    /// — or is slow to find out — must not keep answering true while the
    /// lock expires under it and another instance acquires it.
    pub fn is_leader(&self) -> bool {
        self.is_leader.load(Ordering::SeqCst) && self.lease_fresh()
    }

    fn lease_fresh(&self) -> bool {
        let validity = lease_validity(Duration::from_secs(self.config.lock_ttl_seconds));
        self.last_confirmed
            .lock()
            .map(|g| g.is_some_and(|t| t.elapsed() < validity))
            .unwrap_or(false)
    }

    fn confirm(&self) {
        if let Ok(mut g) = self.last_confirmed.lock() {
            *g = Some(Instant::now());
        }
    }

    /// Get current leadership status
    pub fn status(&self) -> LeadershipStatus {
        *self.status_rx.borrow()
    }

    /// Subscribe to leadership status changes
    pub fn subscribe(&self) -> watch::Receiver<LeadershipStatus> {
        self.status_rx.clone()
    }

    /// Start the leader election process.
    ///
    /// **Why `self: Arc<Self>`** (owned): the body spawns a long-running
    /// election task that clones `self` into its closure
    /// (`let election = self.clone(); tokio::spawn(async move { … })`).
    /// Taking an owned Arc means the call site relinquishes its
    /// reference; the spawned task becomes the new owner and stays alive
    /// until the shutdown signal fires.
    pub async fn start(self: Arc<Self>) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(StandbyError::AlreadyRunning);
        }

        info!(
            instance_id = %self.config.instance_id,
            lock_key = %self.config.lock_key,
            "Starting leader election"
        );

        let election = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(
                election.config.heartbeat_interval_seconds.max(1),
            ));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // A tick is two Redis round trips at most; bound it
                        // as a whole so nothing can wedge the loop.
                        if tokio::time::timeout(
                            REDIS_RESPONSE_TIMEOUT * 3,
                            election.election_tick(),
                        )
                        .await
                        .is_err()
                        {
                            error!(
                                instance_id = %election.config.instance_id,
                                "Leader election tick timed out; demoting"
                            );
                            election.set_status(LeadershipStatus::Unknown);
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!(instance_id = %election.config.instance_id, "Leader election shutting down");
                        election.release_leadership().await;
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Single election tick — Go's `tryAcquire`: SET NX, and if the lock
    /// is already held, extend it if it is ours. Any Redis error demotes.
    ///
    /// Trying to extend when SET NX fails (rather than only when we think we
    /// are leader) is what lets an instance that was demoted by a transient
    /// error take its own, still-held lock straight back on the next tick,
    /// instead of leaving the deployment leaderless until the lock expires.
    async fn election_tick(&self) {
        let mut conn = self.conn.clone();

        if self.is_leader.load(Ordering::SeqCst) && !self.lease_fresh() {
            warn!(
                instance_id = %self.config.instance_id,
                "Leadership lease not confirmed within its validity window; demoting"
            );
            self.set_status(LeadershipStatus::Unknown);
        }

        let acquired = match self.try_acquire_leadership(&mut conn).await {
            Ok(true) => true,
            Ok(false) => match self.extend_lease(&mut conn).await {
                Ok(extended) => extended,
                Err(e) => {
                    error!(error = %e, "Failed to extend lease");
                    self.set_status(LeadershipStatus::Unknown);
                    return;
                }
            },
            Err(e) => {
                error!(error = %e, "Failed to acquire leadership");
                self.set_status(LeadershipStatus::Unknown);
                return;
            }
        };

        if acquired {
            self.confirm();
            debug!(instance_id = %self.config.instance_id, "Holding leadership lock");
            self.set_status(LeadershipStatus::Leader);
        } else {
            debug!(instance_id = %self.config.instance_id, "Leadership held by another instance");
            self.set_status(LeadershipStatus::Follower);
        }
    }

    /// Try to acquire leadership using SET NX
    async fn try_acquire_leadership(&self, conn: &mut ConnectionManager) -> Result<bool> {
        // SET key value NX EX seconds
        let result: Option<String> = redis::cmd("SET")
            .arg(&self.config.lock_key)
            .arg(&self.config.instance_id)
            .arg("NX")
            .arg("EX")
            .arg(self.config.lock_ttl_seconds)
            .query_async(conn)
            .await?;

        Ok(result.is_some())
    }

    /// Extend the leadership lease
    async fn extend_lease(&self, conn: &mut ConnectionManager) -> Result<bool> {
        // Use a Lua script for atomic check-and-extend
        let script = r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                redis.call("EXPIRE", KEYS[1], ARGV[2])
                return 1
            else
                return 0
            end
        "#;

        let result: i32 = redis::Script::new(script)
            .key(&self.config.lock_key)
            .arg(&self.config.instance_id)
            .arg(self.config.lock_ttl_seconds)
            .invoke_async(conn)
            .await?;

        Ok(result == 1)
    }

    /// Release leadership
    async fn release_leadership(&self) {
        if !self.is_leader.load(Ordering::SeqCst) {
            return;
        }

        let mut conn = self.conn.clone();

        // Use Lua script for atomic check-and-delete
        let script = r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                redis.call("DEL", KEYS[1])
                return 1
            else
                return 0
            end
        "#;

        match tokio::time::timeout(
            REDIS_RESPONSE_TIMEOUT,
            redis::Script::new(script)
                .key(&self.config.lock_key)
                .arg(&self.config.instance_id)
                .invoke_async::<i32>(&mut conn),
        )
        .await
        .unwrap_or_else(|_| {
            Err(redis::RedisError::from((
                redis::ErrorKind::IoError,
                "release timed out",
            )))
        }) {
            Ok(1) => {
                info!(instance_id = %self.config.instance_id, "Released leadership");
            }
            Ok(_) => {
                debug!(instance_id = %self.config.instance_id, "Leadership was already released");
            }
            Err(e) => {
                error!(error = %e, "Failed to release leadership");
            }
        }

        if let Ok(mut g) = self.last_confirmed.lock() {
            *g = None;
        }
        self.set_status(LeadershipStatus::Follower);
    }

    /// Update leadership status
    fn set_status(&self, status: LeadershipStatus) {
        let was_leader = self.is_leader.load(Ordering::SeqCst);
        let is_now_leader = status == LeadershipStatus::Leader;

        self.is_leader.store(is_now_leader, Ordering::SeqCst);
        let _ = self.status_tx.send(status);

        if was_leader != is_now_leader {
            if is_now_leader {
                info!(instance_id = %self.config.instance_id, "Became leader");
            } else {
                info!(instance_id = %self.config.instance_id, "Lost leadership");
            }
        }
    }

    /// Stop the leader election
    pub async fn shutdown(&self) {
        info!(instance_id = %self.config.instance_id, "Stopping leader election");
        self.running.store(false, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(());
    }

    /// Get instance ID
    pub fn instance_id(&self) -> &str {
        &self.config.instance_id
    }
}

/// Standby-aware wrapper that gates operations on leadership
pub struct StandbyGuard {
    election: Arc<LeaderElection>,
}

impl StandbyGuard {
    pub fn new(election: Arc<LeaderElection>) -> Self {
        Self { election }
    }

    /// Run a function only if we're the leader
    pub async fn run_if_leader<F, Fut, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        if self.election.is_leader() {
            Some(f().await)
        } else {
            None
        }
    }

    /// Check if we should process (are leader)
    pub fn should_process(&self) -> bool {
        self.election.is_leader()
    }

    /// Wait until we become leader
    pub async fn wait_for_leadership(&self) {
        let mut rx = self.election.subscribe();

        while *rx.borrow() != LeadershipStatus::Leader || !self.election.is_leader() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = LeaderElectionConfig::default();
        assert_eq!(config.lock_ttl_seconds, 30);
        assert_eq!(config.heartbeat_interval_seconds, 10);
        assert_eq!(config.lock_key, "fc:leader");
    }

    /// The local lease is the lock TTL minus a fifth of it (at least 1s),
    /// so an instance stops calling itself leader before its lock can have
    /// expired in Redis.
    #[test]
    fn lease_validity_is_ttl_minus_margin() {
        assert_eq!(
            lease_validity(Duration::from_secs(30)),
            Duration::from_secs(24)
        );
        assert_eq!(
            lease_validity(Duration::from_secs(3)),
            Duration::from_secs(2)
        );
        assert_eq!(lease_validity(Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    fn test_config_builder() {
        let config = LeaderElectionConfig::new("redis://localhost:6380".to_string())
            .with_lock_key("custom:lock".to_string())
            .with_instance_id("test-instance".to_string());

        assert_eq!(config.redis_url, "redis://localhost:6380");
        assert_eq!(config.lock_key, "custom:lock");
        assert_eq!(config.instance_id, "test-instance");
    }
}

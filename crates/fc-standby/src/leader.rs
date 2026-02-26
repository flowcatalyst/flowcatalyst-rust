//! Redis-based Leader Election
//!
//! Implements distributed leader election using Redis with:
//! - SET NX with expiry for atomic lock acquisition
//! - Periodic heartbeat to extend lease
//! - Automatic leader change on lease expiry
//! - Callback notifications for leadership changes

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use redis::aio::ConnectionManager;
use tokio::sync::{broadcast, watch};
use tracing::{info, warn, error, debug};
use uuid::Uuid;

use crate::error::{StandbyError, Result};

/// Configuration for leader election
#[derive(Debug, Clone)]
pub struct LeaderElectionConfig {
    /// Redis connection URL
    pub redis_url: String,

    /// Key prefix for the lock
    pub lock_key: String,

    /// Lock TTL in seconds
    pub lock_ttl_seconds: u64,

    /// Heartbeat interval (should be less than TTL)
    pub heartbeat_interval_seconds: u64,

    /// Unique identifier for this instance
    pub instance_id: String,
}

impl Default for LeaderElectionConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://127.0.0.1:6379".to_string(),
            lock_key: "fc:leader".to_string(),
            lock_ttl_seconds: 30,
            heartbeat_interval_seconds: 10,
            instance_id: Uuid::new_v4().to_string(),
        }
    }
}

impl LeaderElectionConfig {
    pub fn new(redis_url: String) -> Self {
        Self {
            redis_url,
            ..Default::default()
        }
    }

    pub fn with_lock_key(mut self, key: String) -> Self {
        self.lock_key = key;
        self
    }

    pub fn with_instance_id(mut self, id: String) -> Self {
        self.instance_id = id;
        self
    }
}

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

/// Leader election manager
pub struct LeaderElection {
    config: LeaderElectionConfig,
    conn: ConnectionManager,
    is_leader: AtomicBool,
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

        let conn = ConnectionManager::new(client).await?;
        let (shutdown_tx, _) = broadcast::channel(1);
        let (status_tx, status_rx) = watch::channel(LeadershipStatus::Unknown);

        Ok(Self {
            config,
            conn,
            is_leader: AtomicBool::new(false),
            running: AtomicBool::new(false),
            shutdown_tx,
            status_tx,
            status_rx,
        })
    }

    /// Check if this instance is currently the leader
    pub fn is_leader(&self) -> bool {
        self.is_leader.load(Ordering::SeqCst)
    }

    /// Get current leadership status
    pub fn status(&self) -> LeadershipStatus {
        *self.status_rx.borrow()
    }

    /// Subscribe to leadership status changes
    pub fn subscribe(&self) -> watch::Receiver<LeadershipStatus> {
        self.status_rx.clone()
    }

    /// Start the leader election process
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
            let mut interval = tokio::time::interval(
                Duration::from_secs(election.config.heartbeat_interval_seconds)
            );

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        election.election_tick().await;
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

    /// Single election tick - try to acquire or extend leadership
    async fn election_tick(&self) {
        let mut conn = self.conn.clone();

        if self.is_leader() {
            // Already leader - extend the lease
            match self.extend_lease(&mut conn).await {
                Ok(true) => {
                    debug!(instance_id = %self.config.instance_id, "Extended leadership lease");
                }
                Ok(false) => {
                    // Lost leadership
                    warn!(instance_id = %self.config.instance_id, "Lost leadership");
                    self.set_status(LeadershipStatus::Follower);
                }
                Err(e) => {
                    error!(error = %e, "Failed to extend lease");
                    // Assume we lost leadership on error
                    self.set_status(LeadershipStatus::Follower);
                }
            }
        } else {
            // Not leader - try to acquire
            match self.try_acquire_leadership(&mut conn).await {
                Ok(true) => {
                    info!(instance_id = %self.config.instance_id, "Acquired leadership");
                    self.set_status(LeadershipStatus::Leader);
                }
                Ok(false) => {
                    debug!(instance_id = %self.config.instance_id, "Leadership held by another instance");
                    self.set_status(LeadershipStatus::Follower);
                }
                Err(e) => {
                    error!(error = %e, "Failed to acquire leadership");
                    self.set_status(LeadershipStatus::Unknown);
                }
            }
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
        if !self.is_leader() {
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

        match redis::Script::new(script)
            .key(&self.config.lock_key)
            .arg(&self.config.instance_id)
            .invoke_async::<i32>(&mut conn)
            .await
        {
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

        while *rx.borrow() != LeadershipStatus::Leader {
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

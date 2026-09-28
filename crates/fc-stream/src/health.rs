use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time;
use std::time::SystemTime;

/// Health tracker for a single projection service.
///
/// Thread-safe via atomics — no locks required.
#[derive(Debug)]
pub struct StreamHealth {
    name: String,
    running: AtomicBool,
    /// Total rows successfully projected
    processed_count: AtomicU64,
    /// Total poll errors
    error_count: AtomicU64,
    /// Timestamp of last successful poll (Unix millis)
    last_poll_time: AtomicU64,
}

impl StreamHealth {
    pub fn new(name: String) -> Self {
        Self {
            name,
            running: AtomicBool::new(false),
            processed_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            last_poll_time: AtomicU64::new(0),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn add_processed(&self, count: u64) {
        self.processed_count.fetch_add(count, Ordering::SeqCst);
        self.last_poll_time.store(
            SystemTime::now()
                .duration_since(time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::SeqCst,
        );
    }

    pub fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn is_healthy(&self) -> bool {
        self.is_running()
    }

    /// Snapshot for API responses. Keeps the same shape fc-router expects.
    pub fn status(&self) -> StreamHealthSnapshot {
        let poll_ms = self.last_poll_time.load(Ordering::SeqCst);
        let last_checkpoint_at = if poll_ms > 0 {
            chrono::DateTime::from_timestamp_millis(poll_ms as i64)
        } else {
            None
        };

        let status = if self.is_running() {
            StreamStatus::Running
        } else {
            StreamStatus::Stopped
        };

        StreamHealthSnapshot {
            status,
            batch_sequence: self.processed_count.load(Ordering::SeqCst),
            in_flight_count: 0,
            pending_count: 0,
            error_count: self.error_count.load(Ordering::SeqCst),
            last_checkpoint_at,
        }
    }
}

/// Status enum for stream health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamStatus {
    Running,
    Stopped,
    Error,
}

/// Snapshot of stream health for API responses.
#[derive(Debug, Clone)]
pub struct StreamHealthSnapshot {
    pub status: StreamStatus,
    pub batch_sequence: u64,
    pub in_flight_count: u32,
    pub pending_count: u32,
    pub error_count: u64,
    pub last_checkpoint_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Aggregated health with liveness/readiness.
#[derive(Debug, Clone)]
pub struct AggregatedHealth {
    live: bool,
    ready: bool,
    pub errors: Vec<String>,
}

impl AggregatedHealth {
    pub fn is_live(&self) -> bool {
        self.live
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }
}

/// Detailed health status for a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamHealthStatus {
    pub name: String,
    pub running: bool,
    pub healthy: bool,
    pub batch_sequence: u64,
    pub checkpointed_sequence: u64,
    pub pending_batches: u64,
    pub in_flight_count: i64,
    pub has_fatal_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fatal_error: Option<String>,
    pub reconnect_attempts: u64,
    pub last_checkpoint_time_ms: u64,
}

/// Aggregated health status for all streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamProcessorHealth {
    pub healthy: bool,
    pub total_streams: usize,
    pub healthy_streams: usize,
    pub unhealthy_streams: usize,
    pub streams: Vec<StreamHealthStatus>,
}

/// Health service that aggregates health from all projection services.
pub struct StreamHealthService {
    stream_healths: Vec<Arc<StreamHealth>>,
}

impl StreamHealthService {
    pub fn new() -> Self {
        Self {
            stream_healths: Vec::new(),
        }
    }

    pub fn register(&mut self, health: Arc<StreamHealth>) {
        self.stream_healths.push(health);
    }

    /// Live as long as at least one projection is running.
    pub fn is_live(&self) -> bool {
        !self.stream_healths.is_empty() && self.stream_healths.iter().any(|h| h.is_running())
    }

    /// Ready when all registered projections are healthy.
    pub fn is_ready(&self) -> bool {
        !self.stream_healths.is_empty() && self.stream_healths.iter().all(|h| h.is_healthy())
    }

    pub fn get_health(&self) -> StreamProcessorHealth {
        let statuses: Vec<StreamHealthStatus> = self
            .stream_healths
            .iter()
            .map(|h| {
                let snap = h.status();
                StreamHealthStatus {
                    name: h.name().to_string(),
                    running: h.is_running(),
                    healthy: h.is_healthy(),
                    batch_sequence: snap.batch_sequence,
                    checkpointed_sequence: snap.batch_sequence,
                    pending_batches: 0,
                    in_flight_count: 0,
                    has_fatal_error: false,
                    fatal_error: None,
                    reconnect_attempts: 0,
                    last_checkpoint_time_ms: h
                        .status()
                        .last_checkpoint_at
                        .map(|dt| dt.timestamp_millis() as u64)
                        .unwrap_or(0),
                }
            })
            .collect();

        let healthy_count = statuses.iter().filter(|s| s.healthy).count();
        let total = statuses.len();

        StreamProcessorHealth {
            healthy: healthy_count == total && total > 0,
            total_streams: total,
            healthy_streams: healthy_count,
            unhealthy_streams: total - healthy_count,
            streams: statuses,
        }
    }

    pub fn get_aggregated_health(&self) -> AggregatedHealth {
        let live = self.is_live();
        let ready = self.is_ready();
        AggregatedHealth {
            live,
            ready,
            errors: vec![],
        }
    }

    pub fn get_all_stream_health(&self) -> &[Arc<StreamHealth>] {
        &self.stream_healths
    }
}

impl Default for StreamHealthService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_health_basic() {
        let health = StreamHealth::new("test".to_string());

        assert!(!health.is_running());
        assert!(!health.is_healthy());

        health.set_running(true);
        assert!(health.is_running());
        assert!(health.is_healthy());

        health.record_error();
        let snap = health.status();
        assert_eq!(snap.error_count, 1);
    }

    #[test]
    fn test_health_service() {
        let mut service = StreamHealthService::new();

        let h1 = Arc::new(StreamHealth::new("events".to_string()));
        let h2 = Arc::new(StreamHealth::new("dispatch".to_string()));

        service.register(h1.clone());
        service.register(h2.clone());

        assert!(!service.is_live());
        assert!(!service.is_ready());

        h1.set_running(true);
        assert!(service.is_live());
        assert!(!service.is_ready());

        h2.set_running(true);
        assert!(service.is_ready());

        let status = service.get_health();
        assert!(status.healthy);
        assert_eq!(status.total_streams, 2);
    }

    // --- Extended health tests ---

    #[test]
    fn processed_count_accumulates() {
        let health = StreamHealth::new("test".to_string());
        health.set_running(true);

        health.add_processed(10);
        health.add_processed(25);
        health.add_processed(5);

        let snap = health.status();
        assert_eq!(snap.batch_sequence, 40);
    }

    #[test]
    fn last_poll_time_set_after_processing() {
        let health = StreamHealth::new("test".to_string());
        health.set_running(true);

        // Before any processing, no checkpoint
        let snap = health.status();
        assert!(snap.last_checkpoint_at.is_none());

        health.add_processed(1);

        // After processing, checkpoint should be set
        let snap = health.status();
        assert!(snap.last_checkpoint_at.is_some());
    }

    #[test]
    fn error_count_accumulates() {
        let health = StreamHealth::new("test".to_string());
        health.record_error();
        health.record_error();
        health.record_error();

        let snap = health.status();
        assert_eq!(snap.error_count, 3);
    }

    #[test]
    fn status_reflects_running_state() {
        let health = StreamHealth::new("test".to_string());

        let snap = health.status();
        assert_eq!(snap.status, StreamStatus::Stopped);

        health.set_running(true);
        let snap = health.status();
        assert_eq!(snap.status, StreamStatus::Running);

        health.set_running(false);
        let snap = health.status();
        assert_eq!(snap.status, StreamStatus::Stopped);
    }

    #[test]
    fn health_service_empty_is_not_live_or_ready() {
        let service = StreamHealthService::new();
        assert!(!service.is_live());
        assert!(!service.is_ready());

        let health = service.get_health();
        assert!(!health.healthy);
        assert_eq!(health.total_streams, 0);
        assert_eq!(health.healthy_streams, 0);
        assert_eq!(health.unhealthy_streams, 0);
    }

    #[test]
    fn health_service_single_projection() {
        let mut service = StreamHealthService::new();
        let h = Arc::new(StreamHealth::new("single".to_string()));
        service.register(h.clone());

        // Not running yet
        assert!(!service.is_live());
        assert!(!service.is_ready());

        h.set_running(true);
        assert!(service.is_live());
        assert!(service.is_ready());

        let health = service.get_health();
        assert!(health.healthy);
        assert_eq!(health.total_streams, 1);
        assert_eq!(health.healthy_streams, 1);
        assert_eq!(health.unhealthy_streams, 0);
    }

    #[test]
    fn health_service_mixed_running_status() {
        let mut service = StreamHealthService::new();
        let h1 = Arc::new(StreamHealth::new("a".to_string()));
        let h2 = Arc::new(StreamHealth::new("b".to_string()));
        let h3 = Arc::new(StreamHealth::new("c".to_string()));

        service.register(h1.clone());
        service.register(h2.clone());
        service.register(h3.clone());

        // Only one running: live but not ready
        h1.set_running(true);
        assert!(service.is_live());
        assert!(!service.is_ready());

        let health = service.get_health();
        assert!(!health.healthy);
        assert_eq!(health.healthy_streams, 1);
        assert_eq!(health.unhealthy_streams, 2);

        // Two running: still not ready (one unhealthy)
        h2.set_running(true);
        assert!(service.is_live());
        assert!(!service.is_ready());

        // All running: ready
        h3.set_running(true);
        assert!(service.is_live());
        assert!(service.is_ready());
        assert!(service.get_health().healthy);
    }

    #[test]
    fn aggregated_health_reflects_state() {
        let mut service = StreamHealthService::new();
        let h1 = Arc::new(StreamHealth::new("events".to_string()));
        let h2 = Arc::new(StreamHealth::new("dispatch".to_string()));
        service.register(h1.clone());
        service.register(h2.clone());

        let agg = service.get_aggregated_health();
        assert!(!agg.is_live());
        assert!(!agg.is_ready());

        h1.set_running(true);
        let agg = service.get_aggregated_health();
        assert!(agg.is_live());
        assert!(!agg.is_ready());

        h2.set_running(true);
        let agg = service.get_aggregated_health();
        assert!(agg.is_live());
        assert!(agg.is_ready());
    }

    #[test]
    fn get_all_stream_health_returns_registered() {
        let mut service = StreamHealthService::new();
        let h1 = Arc::new(StreamHealth::new("a".to_string()));
        let h2 = Arc::new(StreamHealth::new("b".to_string()));
        service.register(h1);
        service.register(h2);

        let all = service.get_all_stream_health();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name(), "a");
        assert_eq!(all[1].name(), "b");
    }

    #[test]
    fn health_status_stream_details() {
        let mut service = StreamHealthService::new();
        let h = Arc::new(StreamHealth::new("test-stream".to_string()));
        h.set_running(true);
        h.add_processed(42);
        h.record_error();
        h.record_error();
        service.register(h);

        let health = service.get_health();
        assert_eq!(health.streams.len(), 1);

        let stream = &health.streams[0];
        assert_eq!(stream.name, "test-stream");
        assert!(stream.running);
        assert!(stream.healthy);
        assert_eq!(stream.batch_sequence, 42);
        assert!(!stream.has_fatal_error);
        assert!(stream.fatal_error.is_none());
        assert!(stream.last_checkpoint_time_ms > 0);
    }

    #[test]
    fn default_service_is_new() {
        let service = StreamHealthService::default();
        assert_eq!(service.get_all_stream_health().len(), 0);
        assert!(!service.is_live());
        assert!(!service.is_ready());
    }
}

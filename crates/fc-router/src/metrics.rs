//! Enhanced Metrics Collection
//!
//! Provides sliding window metrics for processing pools with:
//! - Success/failure counters
//! - Processing time tracking with percentiles
//! - 5-minute and 30-minute time windows

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use chrono::Utc;

use fc_common::{
    EnhancedPoolMetrics, ProcessingTimeMetrics, WindowedMetrics,
};

/// A single metric sample
#[derive(Debug, Clone)]
struct MetricSample {
    /// Timestamp when the sample was recorded
    timestamp: Instant,
    /// Processing duration in milliseconds
    duration_ms: u64,
    /// Whether the operation succeeded
    success: bool,
}

/// Configuration for the metrics collector
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Maximum samples to retain for percentile calculations
    pub max_samples: usize,
    /// Duration of the short window (default: 5 minutes)
    pub short_window: Duration,
    /// Duration of the long window (default: 30 minutes)
    pub long_window: Duration,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            max_samples: 10000,
            short_window: Duration::from_secs(300),   // 5 minutes
            long_window: Duration::from_secs(1800),   // 30 minutes
        }
    }
}

/// Metrics collector for a processing pool
///
/// Uses a sliding window approach to track metrics over time.
/// Thread-safe for concurrent access from multiple workers.
pub struct PoolMetricsCollector {
    config: MetricsConfig,

    /// All-time counters
    total_success: AtomicU64,
    total_failure: AtomicU64,
    total_rate_limited: AtomicU64,

    /// Samples for percentile calculation (protected by RwLock)
    samples: RwLock<VecDeque<MetricSample>>,

    /// Rate-limited event timestamps for windowed counting
    rate_limited_events: RwLock<VecDeque<Instant>>,
}

impl PoolMetricsCollector {
    pub fn new() -> Self {
        Self::with_config(MetricsConfig::default())
    }

    pub fn with_config(config: MetricsConfig) -> Self {
        Self {
            config,
            total_success: AtomicU64::new(0),
            total_failure: AtomicU64::new(0),
            total_rate_limited: AtomicU64::new(0),
            samples: RwLock::new(VecDeque::with_capacity(10000)),
            rate_limited_events: RwLock::new(VecDeque::with_capacity(1000)),
        }
    }

    /// Record a successful message processing
    pub fn record_success(&self, duration_ms: u64) {
        self.total_success.fetch_add(1, Ordering::Relaxed);
        self.add_sample(duration_ms, true);
    }

    /// Record a failed message processing (permanent failure, e.g. ERROR_CONFIG, ERROR_CONNECTION)
    pub fn record_failure(&self, duration_ms: u64) {
        self.total_failure.fetch_add(1, Ordering::Relaxed);
        self.add_sample(duration_ms, false);
    }

    /// Record a transient error (ERROR_PROCESS — message will be retried, not a permanent failure).
    /// Matches Java's poolMetrics.recordProcessingTransient() which does NOT increment the failure counter.
    /// The message will reappear from the queue, so success rate should not be penalised.
    pub fn record_transient(&self, duration_ms: u64) {
        // Do not increment total_failure — transient errors are retried and not counted against success rate.
        // Still add the sample as a non-success so windowed success-rate reflects current processing state.
        self.add_sample(duration_ms, false);
    }

    /// Record a rate-limited event
    pub fn record_rate_limited(&self) {
        self.total_rate_limited.fetch_add(1, Ordering::Relaxed);

        let mut events = self.rate_limited_events.write();
        let now = Instant::now();

        // Remove old events beyond long window
        let cutoff = now - self.config.long_window;
        while events.front().map(|t| *t < cutoff).unwrap_or(false) {
            events.pop_front();
        }

        events.push_back(now);
    }

    /// Get all-time rate limited count
    pub fn total_rate_limited(&self) -> u64 {
        self.total_rate_limited.load(Ordering::Relaxed)
    }

    /// Add a sample to the sliding window
    fn add_sample(&self, duration_ms: u64, success: bool) {
        let sample = MetricSample {
            timestamp: Instant::now(),
            duration_ms,
            success,
        };

        let mut samples = self.samples.write();

        // Remove old samples beyond long window
        let cutoff = Instant::now() - self.config.long_window;
        while samples.front().map(|s| s.timestamp < cutoff).unwrap_or(false) {
            samples.pop_front();
        }

        // Add new sample
        samples.push_back(sample);

        // Enforce max samples (keep most recent)
        while samples.len() > self.config.max_samples {
            samples.pop_front();
        }
    }

    /// Get all-time success count
    pub fn total_success(&self) -> u64 {
        self.total_success.load(Ordering::Relaxed)
    }

    /// Get all-time failure count
    pub fn total_failure(&self) -> u64 {
        self.total_failure.load(Ordering::Relaxed)
    }

    /// Get enhanced metrics snapshot
    pub fn get_metrics(&self) -> EnhancedPoolMetrics {
        let samples = self.samples.read();
        let rate_limited_events = self.rate_limited_events.read();
        let now = Instant::now();

        let total_success = self.total_success.load(Ordering::Relaxed);
        let total_failure = self.total_failure.load(Ordering::Relaxed);
        let total_rate_limited = self.total_rate_limited.load(Ordering::Relaxed);
        let total = total_success + total_failure;

        let success_rate = if total > 0 {
            total_success as f64 / total as f64
        } else {
            1.0
        };

        // Calculate all-time processing time metrics from samples
        let all_durations: Vec<u64> = samples.iter().map(|s| s.duration_ms).collect();
        let processing_time = Self::calculate_processing_time_metrics(&all_durations);

        // Calculate windowed metrics
        let short_cutoff = now - self.config.short_window;
        let long_cutoff = now - self.config.long_window;

        let short_samples: Vec<&MetricSample> = samples
            .iter()
            .filter(|s| s.timestamp >= short_cutoff)
            .collect();

        let long_samples: Vec<&MetricSample> = samples
            .iter()
            .filter(|s| s.timestamp >= long_cutoff)
            .collect();

        // Count rate limited events in windows
        let rate_limited_5min = rate_limited_events
            .iter()
            .filter(|t| **t >= short_cutoff)
            .count() as u64;

        let rate_limited_30min = rate_limited_events
            .iter()
            .filter(|t| **t >= long_cutoff)
            .count() as u64;

        let mut last_5_min = Self::calculate_windowed_metrics(
            &short_samples,
            self.config.short_window,
        );
        last_5_min.rate_limited_count = rate_limited_5min;

        let mut last_30_min = Self::calculate_windowed_metrics(
            &long_samples,
            self.config.long_window,
        );
        last_30_min.rate_limited_count = rate_limited_30min;

        EnhancedPoolMetrics {
            total_success,
            total_failure,
            total_rate_limited,
            success_rate,
            processing_time,
            last_5_min,
            last_30_min,
        }
    }

    /// Calculate processing time metrics from durations
    fn calculate_processing_time_metrics(durations: &[u64]) -> ProcessingTimeMetrics {
        if durations.is_empty() {
            return ProcessingTimeMetrics::default();
        }

        let mut sorted: Vec<u64> = durations.to_vec();
        sorted.sort_unstable();

        let sum: u64 = sorted.iter().sum();
        let count = sorted.len() as u64;
        let avg = sum as f64 / count as f64;

        let min = sorted[0];
        let max = sorted[sorted.len() - 1];

        // Calculate percentiles
        let p50 = Self::percentile(&sorted, 50.0);
        let p95 = Self::percentile(&sorted, 95.0);
        let p99 = Self::percentile(&sorted, 99.0);

        ProcessingTimeMetrics {
            avg_ms: avg,
            min_ms: min,
            max_ms: max,
            p50_ms: p50,
            p95_ms: p95,
            p99_ms: p99,
            sample_count: count,
        }
    }

    /// Calculate windowed metrics from samples
    fn calculate_windowed_metrics(
        samples: &[&MetricSample],
        window_duration: Duration,
    ) -> WindowedMetrics {
        let success_count = samples.iter().filter(|s| s.success).count() as u64;
        let failure_count = samples.iter().filter(|s| !s.success).count() as u64;
        let total = success_count + failure_count;

        let success_rate = if total > 0 {
            success_count as f64 / total as f64
        } else {
            1.0
        };

        let window_secs = window_duration.as_secs_f64();
        let throughput_per_sec = if window_secs > 0.0 {
            total as f64 / window_secs
        } else {
            0.0
        };

        let durations: Vec<u64> = samples.iter().map(|s| s.duration_ms).collect();
        let processing_time = Self::calculate_processing_time_metrics(&durations);

        let window_start = Utc::now() - chrono::Duration::seconds(window_duration.as_secs() as i64);

        WindowedMetrics {
            success_count,
            failure_count,
            rate_limited_count: 0, // Set by caller from rate_limited_events
            success_rate,
            throughput_per_sec,
            processing_time,
            window_start,
            window_duration_secs: window_duration.as_secs(),
        }
    }

    /// Calculate a percentile value from sorted data
    fn percentile(sorted: &[u64], p: f64) -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        if sorted.len() == 1 {
            return sorted[0];
        }

        let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// Reset all metrics (useful for testing)
    pub fn reset(&self) {
        self.total_success.store(0, Ordering::Relaxed);
        self.total_failure.store(0, Ordering::Relaxed);
        self.total_rate_limited.store(0, Ordering::Relaxed);
        self.samples.write().clear();
        self.rate_limited_events.write().clear();
    }
}

impl Default for PoolMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_metrics() {
        let collector = PoolMetricsCollector::new();
        let metrics = collector.get_metrics();

        assert_eq!(metrics.total_success, 0);
        assert_eq!(metrics.total_failure, 0);
        assert_eq!(metrics.success_rate, 1.0); // No failures = 100% success
    }

    #[test]
    fn test_success_recording() {
        let collector = PoolMetricsCollector::new();

        collector.record_success(100);
        collector.record_success(200);
        collector.record_success(300);

        let metrics = collector.get_metrics();

        assert_eq!(metrics.total_success, 3);
        assert_eq!(metrics.total_failure, 0);
        assert_eq!(metrics.success_rate, 1.0);
        assert_eq!(metrics.processing_time.sample_count, 3);
        assert!((metrics.processing_time.avg_ms - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_failure_recording() {
        let collector = PoolMetricsCollector::new();

        collector.record_success(100);
        collector.record_failure(500);

        let metrics = collector.get_metrics();

        assert_eq!(metrics.total_success, 1);
        assert_eq!(metrics.total_failure, 1);
        assert_eq!(metrics.success_rate, 0.5);
    }

    #[test]
    fn test_percentile_calculation() {
        let sorted = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        // p50 at index (50/100 * 9) = 4.5, rounds to 5, which gives value 6
        assert_eq!(PoolMetricsCollector::percentile(&sorted, 50.0), 6);
        // p95 at index (95/100 * 9) = 8.55, rounds to 9, which gives value 10
        assert_eq!(PoolMetricsCollector::percentile(&sorted, 95.0), 10);
        // p0 at index 0, which gives value 1
        assert_eq!(PoolMetricsCollector::percentile(&sorted, 0.0), 1);
        // p100 at index 9, which gives value 10
        assert_eq!(PoolMetricsCollector::percentile(&sorted, 100.0), 10);
    }

    #[test]
    fn test_processing_time_metrics() {
        let durations = vec![100, 200, 300, 400, 500];
        let metrics = PoolMetricsCollector::calculate_processing_time_metrics(&durations);

        assert_eq!(metrics.min_ms, 100);
        assert_eq!(metrics.max_ms, 500);
        assert!((metrics.avg_ms - 300.0).abs() < 0.01);
        assert_eq!(metrics.sample_count, 5);
    }

    #[test]
    fn test_windowed_metrics() {
        let collector = PoolMetricsCollector::new();

        // Record some samples
        for i in 0..10 {
            if i % 3 == 0 {
                collector.record_failure(100 + i * 10);
            } else {
                collector.record_success(100 + i * 10);
            }
        }

        let metrics = collector.get_metrics();

        // All samples should be within the 5-minute window
        assert_eq!(metrics.last_5_min.success_count + metrics.last_5_min.failure_count, 10);
        assert!(metrics.last_5_min.throughput_per_sec > 0.0);
    }
}

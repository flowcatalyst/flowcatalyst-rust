//! Enhanced Metrics Collection
//!
//! Provides sliding window metrics for processing pools with:
//! - Success/failure counters
//! - Processing time tracking with percentiles (HdrHistogram for O(1) record/read)
//! - 5-minute and 30-minute time windows

use chrono::Utc;
use hdrhistogram::Histogram;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use fc_common::{EnhancedPoolMetrics, ProcessingTimeMetrics, WindowedMetrics};

/// A single metric sample (kept for windowed success/failure counting)
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
    /// Maximum samples to retain for windowed calculations
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
            short_window: Duration::from_secs(300), // 5 minutes
            long_window: Duration::from_secs(1800), // 30 minutes
        }
    }
}

/// Metrics collector for a processing pool.
///
/// Uses HdrHistogram for O(1) percentile reads on all-time latency data.
/// Uses a bounded VecDeque for windowed success/failure counting.
/// Thread-safe for concurrent access from multiple workers.
pub struct PoolMetricsCollector {
    config: MetricsConfig,

    /// All-time counters
    total_success: AtomicU64,
    total_failure: AtomicU64,
    total_rate_limited: AtomicU64,
    /// Messages ACKed without delivery because their group was suppressed
    /// by a target's `flushGroup` request (ledger R-53). Kept separate
    /// from `total_success` — no mediation happened, no duration to
    /// record — so a heavily-flushed pool reads busy-but-suppressed
    /// rather than idle.
    total_suppressed: AtomicU64,

    /// All-time latency histogram — O(1) record, O(1) percentile query.
    /// Covers 1ms to 15 minutes (900,000ms) with 3 significant digits.
    histogram: RwLock<Histogram<u64>>,

    /// Samples for windowed success/failure counting and windowed percentiles
    samples: RwLock<VecDeque<MetricSample>>,

    /// Rate-limited event timestamps for windowed counting
    rate_limited_events: RwLock<VecDeque<Instant>>,
}

impl PoolMetricsCollector {
    pub fn new() -> Self {
        Self::with_config(MetricsConfig::default())
    }

    pub fn with_config(config: MetricsConfig) -> Self {
        // 1ms to 900_000ms (15 minutes) with 3 significant digits
        let histogram = Histogram::new_with_bounds(1, 900_000, 3).expect("valid histogram bounds");

        Self {
            config,
            total_success: AtomicU64::new(0),
            total_failure: AtomicU64::new(0),
            total_rate_limited: AtomicU64::new(0),
            total_suppressed: AtomicU64::new(0),
            histogram: RwLock::new(histogram),
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

    /// Record a message ACKed without delivery because its group was
    /// suppressed by a target's `flushGroup` request (ledger R-53). No
    /// mediation happened, so there is no duration to record — this only
    /// bumps the counter.
    pub fn record_suppressed(&self) {
        self.total_suppressed.fetch_add(1, Ordering::Relaxed);
    }

    /// Get all-time suppressed-ACK count.
    pub fn total_suppressed(&self) -> u64 {
        self.total_suppressed.load(Ordering::Relaxed)
    }

    /// Add a sample to the sliding window and all-time histogram
    fn add_sample(&self, duration_ms: u64, success: bool) {
        // Record in HdrHistogram (clamp to histogram bounds)
        let clamped = duration_ms.clamp(1, 900_000);
        self.histogram.write().record(clamped).ok();

        let sample = MetricSample {
            timestamp: Instant::now(),
            duration_ms,
            success,
        };

        let mut samples = self.samples.write();

        // Remove old samples beyond long window
        let cutoff = Instant::now() - self.config.long_window;
        while samples
            .front()
            .map(|s| s.timestamp < cutoff)
            .unwrap_or(false)
        {
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

    /// Extract ProcessingTimeMetrics from an HdrHistogram
    fn metrics_from_histogram(hist: &Histogram<u64>) -> ProcessingTimeMetrics {
        if hist.is_empty() {
            return ProcessingTimeMetrics::default();
        }

        ProcessingTimeMetrics {
            avg_ms: hist.mean(),
            min_ms: hist.min(),
            max_ms: hist.max(),
            p50_ms: hist.value_at_quantile(0.50),
            p95_ms: hist.value_at_quantile(0.95),
            p99_ms: hist.value_at_quantile(0.99),
            sample_count: hist.len(),
        }
    }

    /// Build a temporary histogram from a slice of samples (for windowed percentiles)
    fn windowed_processing_time(samples: &[&MetricSample]) -> ProcessingTimeMetrics {
        if samples.is_empty() {
            return ProcessingTimeMetrics::default();
        }

        let mut hist =
            Histogram::<u64>::new_with_bounds(1, 900_000, 3).expect("valid histogram bounds");

        for s in samples {
            hist.record(s.duration_ms.clamp(1, 900_000)).ok();
        }

        Self::metrics_from_histogram(&hist)
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

        // All-time processing time from HdrHistogram — O(1) percentile reads
        let processing_time = Self::metrics_from_histogram(&self.histogram.read());

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

        let mut last_5_min =
            Self::calculate_windowed_metrics(&short_samples, self.config.short_window);
        last_5_min.rate_limited_count = rate_limited_5min;

        let mut last_30_min =
            Self::calculate_windowed_metrics(&long_samples, self.config.long_window);
        last_30_min.rate_limited_count = rate_limited_30min;

        EnhancedPoolMetrics {
            total_success,
            total_failure,
            total_rate_limited,
            total_suppressed: self.total_suppressed.load(Ordering::Relaxed),
            success_rate,
            processing_time,
            last_5_min,
            last_30_min,
        }
    }

    /// Calculate windowed metrics from samples (uses HdrHistogram for windowed percentiles)
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

        let processing_time = Self::windowed_processing_time(samples);

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

    /// Reset all metrics (useful for testing)
    pub fn reset(&self) {
        self.total_success.store(0, Ordering::Relaxed);
        self.total_failure.store(0, Ordering::Relaxed);
        self.total_rate_limited.store(0, Ordering::Relaxed);
        self.total_suppressed.store(0, Ordering::Relaxed);
        self.histogram.write().reset();
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
        assert!((metrics.processing_time.avg_ms - 200.0).abs() < 1.0);
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
    fn test_percentiles_from_histogram() {
        let collector = PoolMetricsCollector::new();

        for i in 1..=100 {
            collector.record_success(i);
        }

        let metrics = collector.get_metrics();

        // HdrHistogram values are approximate; check within tolerance
        assert!(metrics.processing_time.p50_ms >= 49 && metrics.processing_time.p50_ms <= 51);
        assert!(metrics.processing_time.p95_ms >= 94 && metrics.processing_time.p95_ms <= 96);
        assert!(metrics.processing_time.p99_ms >= 98 && metrics.processing_time.p99_ms <= 100);
    }

    #[test]
    fn test_processing_time_metrics() {
        let collector = PoolMetricsCollector::new();

        collector.record_success(100);
        collector.record_success(200);
        collector.record_success(300);
        collector.record_success(400);
        collector.record_success(500);

        let metrics = collector.get_metrics();

        assert_eq!(metrics.processing_time.min_ms, 100);
        assert_eq!(metrics.processing_time.max_ms, 500);
        assert!((metrics.processing_time.avg_ms - 300.0).abs() < 1.0);
        assert_eq!(metrics.processing_time.sample_count, 5);
    }

    #[test]
    fn test_windowed_metrics() {
        let collector = PoolMetricsCollector::new();

        // Record some samples
        for i in 0..10u64 {
            if i % 3 == 0 {
                collector.record_failure(100 + i * 10);
            } else {
                collector.record_success(100 + i * 10);
            }
        }

        let metrics = collector.get_metrics();

        // All samples should be within the 5-minute window
        assert_eq!(
            metrics.last_5_min.success_count + metrics.last_5_min.failure_count,
            10
        );
        assert!(metrics.last_5_min.throughput_per_sec > 0.0);
    }
}

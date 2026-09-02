//! Circuit Breaker Registry - Per-endpoint circuit breaker tracking and enforcement
//!
//! Single source of truth for circuit breakers, keyed by endpoint URL.
//! Used by ProcessPool to gate requests before mediation.
//! Compatible with Java's Resilience4j circuit breaker stats format.

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use utoipa::ToSchema;

/// Derive the circuit-breaker key for a mediation target (ledger R-12):
/// `scheme://host[:port]/path`, with the query string and fragment
/// stripped. Used at every breaker call site (`allow_request`,
/// `record_success`, `record_failure`) so a target is keyed by endpoint,
/// not by the exact URL string.
///
/// The query string is per-message data (e.g. a webhook signature nonce,
/// a message id) — keying on the raw URL would fragment the failure
/// signal across as many "endpoints" as there are distinct query strings,
/// so a genuinely dead endpoint might never trip its breaker. Two targets
/// that differ only by query therefore share one breaker; two that differ
/// by path do not — the path is part of what the target considers "this
/// endpoint".
///
/// An unparseable URL falls back to the raw string unchanged: still a
/// valid (if degenerate) key, and callers that reach this far already
/// treat an unparseable target as its own pre-flight rejection elsewhere
/// (see `mediator.rs`) — this helper never fails.
pub fn breaker_key(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("");
            let mut key = format!("{scheme}://{host}");
            if let Some(port) = parsed.port() {
                key.push(':');
                key.push_str(&port.to_string());
            }
            let path = parsed.path();
            key.push_str(path);
            key
        }
        Err(_) => url.to_string(),
    }
}

/// Circuit breaker state (matches Java Resilience4j states)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum CircuitBreakerState {
    /// Circuit is closed (normal operation)
    #[default]
    Closed,
    /// Circuit is open (rejecting requests)
    Open,
    /// Circuit is testing (allowing limited requests)
    HalfOpen,
}

/// Statistics for a single circuit breaker (matches Java format)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CircuitBreakerStats {
    /// Name/identifier of the circuit breaker (usually the endpoint URL)
    pub name: String,
    /// Current state: CLOSED, OPEN, HALF_OPEN
    pub state: CircuitBreakerState,
    /// Number of successful calls
    #[serde(rename = "successfulCalls")]
    pub successful_calls: u64,
    /// Number of failed calls
    #[serde(rename = "failedCalls")]
    pub failed_calls: u64,
    /// Number of calls rejected while open
    #[serde(rename = "rejectedCalls")]
    pub rejected_calls: u64,
    /// Failure rate (0.0 - 1.0)
    #[serde(rename = "failureRate")]
    pub failure_rate: f64,
    /// Number of buffered calls for rate calculation
    #[serde(rename = "bufferedCalls")]
    pub buffered_calls: u32,
    /// Size of the buffer
    #[serde(rename = "bufferSize")]
    pub buffer_size: u32,
}

/// Per-endpoint circuit breaker.
/// All state transitions are protected by a single Mutex to prevent race conditions.
/// Counters use atomics for lock-free increment (stats reads don't need the mutex).
struct EndpointCircuitBreaker {
    name: String,
    /// Mutex protects state, sliding window, half_open_success_count, and last_failure_time
    /// as a single unit — prevents race conditions in state transitions.
    inner: Mutex<BreakerInner>,
    /// Lock-free counters for stats (reads don't need the mutex)
    successful_calls: AtomicU64,
    failed_calls: AtomicU64,
    rejected_calls: AtomicU64,
    /// Last time any call was recorded (for idle eviction)
    last_activity: RwLock<Instant>,
    // Configuration
    config: CircuitBreakerConfig,
}

struct BreakerInner {
    state: CircuitBreakerState,
    last_failure_time: Option<Instant>,
    last_state_change: Instant,
    half_open_success_count: u32,
    /// Sliding window of recent results (true=success, false=failure)
    recent_results: VecDeque<bool>,
}

impl EndpointCircuitBreaker {
    fn new(name: String, config: &CircuitBreakerConfig) -> Self {
        Self {
            name,
            inner: Mutex::new(BreakerInner {
                state: CircuitBreakerState::Closed,
                last_failure_time: None,
                last_state_change: Instant::now(),
                half_open_success_count: 0,
                recent_results: VecDeque::with_capacity(config.buffer_size as usize),
            }),
            successful_calls: AtomicU64::new(0),
            failed_calls: AtomicU64::new(0),
            rejected_calls: AtomicU64::new(0),
            last_activity: RwLock::new(Instant::now()),
            config: config.clone(),
        }
    }

    /// Calculate failure rate from the sliding window
    fn failure_rate(results: &VecDeque<bool>) -> f64 {
        if results.is_empty() {
            return 0.0;
        }
        let failures = results.iter().filter(|&&s| !s).count();
        failures as f64 / results.len() as f64
    }

    fn push_result(inner: &mut BreakerInner, success: bool, buffer_size: u32) {
        if inner.recent_results.len() >= buffer_size as usize {
            inner.recent_results.pop_front();
        }
        inner.recent_results.push_back(success);
    }

    fn record_success(&self) {
        self.successful_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_activity.write() = Instant::now();

        let mut inner = self.inner.lock();
        Self::push_result(&mut inner, true, self.config.buffer_size);

        if inner.state == CircuitBreakerState::HalfOpen {
            inner.half_open_success_count += 1;
            if inner.half_open_success_count >= self.config.success_threshold {
                inner.state = CircuitBreakerState::Closed;
                inner.last_state_change = Instant::now();
                inner.recent_results.clear();
                inner.half_open_success_count = 0;
            }
        }
    }

    fn record_failure(&self) {
        self.failed_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_activity.write() = Instant::now();

        let mut inner = self.inner.lock();
        inner.last_failure_time = Some(Instant::now());
        Self::push_result(&mut inner, false, self.config.buffer_size);

        match inner.state {
            CircuitBreakerState::Closed => {
                if inner.recent_results.len() >= self.config.min_calls as usize {
                    let rate = Self::failure_rate(&inner.recent_results);
                    if rate >= self.config.failure_rate_threshold {
                        inner.state = CircuitBreakerState::Open;
                        inner.last_state_change = Instant::now();
                    }
                }
            }
            CircuitBreakerState::HalfOpen => {
                // Any failure in half-open immediately reopens
                inner.state = CircuitBreakerState::Open;
                inner.last_state_change = Instant::now();
                inner.half_open_success_count = 0;
            }
            CircuitBreakerState::Open => {}
        }
    }

    fn record_rejected(&self) {
        self.rejected_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_activity.write() = Instant::now();
    }

    fn allow_request(&self) -> bool {
        let mut inner = self.inner.lock();
        match inner.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                if let Some(last_failure) = inner.last_failure_time {
                    if last_failure.elapsed() >= self.config.reset_timeout {
                        inner.state = CircuitBreakerState::HalfOpen;
                        inner.last_state_change = Instant::now();
                        inner.half_open_success_count = 0;
                        return true;
                    }
                }
                false
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    fn get_stats(&self) -> CircuitBreakerStats {
        let inner = self.inner.lock();
        CircuitBreakerStats {
            name: self.name.clone(),
            state: inner.state,
            successful_calls: self.successful_calls.load(Ordering::Relaxed),
            failed_calls: self.failed_calls.load(Ordering::Relaxed),
            rejected_calls: self.rejected_calls.load(Ordering::Relaxed),
            failure_rate: Self::failure_rate(&inner.recent_results),
            buffered_calls: inner.recent_results.len() as u32,
            buffer_size: self.config.buffer_size,
        }
    }

    fn reset(&self) {
        let mut inner = self.inner.lock();
        inner.state = CircuitBreakerState::Closed;
        inner.last_state_change = Instant::now();
        inner.last_failure_time = None;
        inner.half_open_success_count = 0;
        inner.recent_results.clear();
    }
}

/// Configuration for circuit breaker registry
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure rate threshold (0.0-1.0) to trip the breaker. Java default: 0.5
    pub failure_rate_threshold: f64,
    /// Minimum calls in buffer before evaluating failure rate. Java default: 10
    pub min_calls: u32,
    /// Number of successes in half-open before closing. Java default: 3
    pub success_threshold: u32,
    /// Time before transitioning from open to half-open. Java default: 5s
    pub reset_timeout: Duration,
    /// Sliding window size for failure rate calculation. Java default: 100
    pub buffer_size: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_rate_threshold: 0.5,           // Java: failureRatio=0.5
            min_calls: 10,                         // Java: requestVolumeThreshold=10
            success_threshold: 3,                  // Java: successThreshold=3
            reset_timeout: Duration::from_secs(5), // Java: delay=5000
            buffer_size: 100,                      // Sliding window size
        }
    }
}

/// Registry for per-endpoint circuit breakers.
/// Thread-safe, keyed by endpoint URL. Used by ProcessPool for enforcement
/// and by the monitoring API for observability.
pub struct CircuitBreakerRegistry {
    breakers: RwLock<HashMap<String, Arc<EndpointCircuitBreaker>>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Get or create a circuit breaker for an endpoint
    fn get_or_create(&self, endpoint: &str) -> Arc<EndpointCircuitBreaker> {
        // Try read first (fast path — breaker already exists)
        {
            let breakers = self.breakers.read();
            if let Some(breaker) = breakers.get(endpoint) {
                return Arc::clone(breaker);
            }
        }

        // Slow path — create new breaker
        let mut breakers = self.breakers.write();
        // Double-check after acquiring write lock
        if let Some(breaker) = breakers.get(endpoint) {
            return Arc::clone(breaker);
        }
        let breaker = Arc::new(EndpointCircuitBreaker::new(
            endpoint.to_string(),
            &self.config,
        ));
        breakers.insert(endpoint.to_string(), Arc::clone(&breaker));
        breaker
    }

    /// Check if request should be allowed for endpoint.
    /// Records a rejection if the breaker is open.
    pub fn allow_request(&self, endpoint: &str) -> bool {
        let breaker = self.get_or_create(endpoint);
        let allowed = breaker.allow_request();
        if !allowed {
            breaker.record_rejected();
        }
        allowed
    }

    /// Record a successful call
    pub fn record_success(&self, endpoint: &str) {
        let breaker = self.get_or_create(endpoint);
        breaker.record_success();
    }

    /// Record a failed call
    pub fn record_failure(&self, endpoint: &str) {
        let breaker = self.get_or_create(endpoint);
        breaker.record_failure();
    }

    /// Get stats for all circuit breakers
    pub fn get_all_stats(&self) -> HashMap<String, CircuitBreakerStats> {
        let breakers = self.breakers.read();
        breakers
            .iter()
            .map(|(name, breaker)| (name.clone(), breaker.get_stats()))
            .collect()
    }

    /// Get stats for a specific circuit breaker
    pub fn get_stats(&self, endpoint: &str) -> Option<CircuitBreakerStats> {
        let breakers = self.breakers.read();
        breakers.get(endpoint).map(|b| b.get_stats())
    }

    /// Get state of a specific circuit breaker
    pub fn get_state(&self, endpoint: &str) -> Option<CircuitBreakerState> {
        let breakers = self.breakers.read();
        breakers.get(endpoint).map(|b| b.inner.lock().state)
    }

    /// Reset a specific circuit breaker
    pub fn reset(&self, endpoint: &str) -> bool {
        let breakers = self.breakers.read();
        if let Some(breaker) = breakers.get(endpoint) {
            breaker.reset();
            true
        } else {
            false
        }
    }

    /// Reset all circuit breakers
    pub fn reset_all(&self) {
        let breakers = self.breakers.read();
        for breaker in breakers.values() {
            breaker.reset();
        }
    }

    /// Evict circuit breakers that have been idle (no calls) for longer than `max_idle`.
    /// Returns the number of breakers evicted.
    pub fn evict_idle(&self, max_idle: Duration) -> usize {
        if self.breakers.read().is_empty() {
            return 0;
        }

        let idle_keys: Vec<String> = {
            let breakers = self.breakers.read();
            breakers
                .iter()
                .filter(|(_, b)| b.last_activity.read().elapsed() > max_idle)
                .map(|(k, _)| k.clone())
                .collect()
        };

        if idle_keys.is_empty() {
            return 0;
        }

        let mut breakers = self.breakers.write();
        let mut evicted = 0;
        for key in &idle_keys {
            breakers.remove(key);
            evicted += 1;
        }
        evicted
    }

    /// Get count of open circuit breakers
    pub fn open_count(&self) -> usize {
        let breakers = self.breakers.read();
        breakers
            .values()
            .filter(|b| b.inner.lock().state == CircuitBreakerState::Open)
            .count()
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_trips_on_failure_ratio() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            min_calls: 4,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(100),
            buffer_size: 10,
        });

        let endpoint = "http://test.com/api";

        // Should be closed initially
        assert!(registry.allow_request(endpoint));

        // 1 success + 2 failures = 3 calls, below min_calls threshold
        registry.record_success(endpoint);
        registry.record_failure(endpoint);
        registry.record_failure(endpoint);
        assert!(registry.allow_request(endpoint)); // Still closed (only 3 calls < 4 min_calls)

        // 4th call is a failure: now 1 success + 3 failures = 75% failure rate >= 50%
        registry.record_failure(endpoint);
        assert!(!registry.allow_request(endpoint)); // Now open

        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Open)
        );
    }

    #[test]
    fn test_circuit_breaker_stays_closed_below_threshold() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            min_calls: 4,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(100),
            buffer_size: 10,
        });

        let endpoint = "http://test.com/api";

        // 3 successes + 1 failure = 25% failure rate, below 50% threshold
        registry.record_success(endpoint);
        registry.record_success(endpoint);
        registry.record_success(endpoint);
        registry.record_failure(endpoint);

        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Closed)
        );
    }

    #[test]
    fn test_circuit_breaker_half_open_recovery() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            min_calls: 4,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(50),
            buffer_size: 10,
        });

        let endpoint = "http://test.com/api";

        // Trip the breaker
        for _ in 0..5 {
            registry.record_failure(endpoint);
        }
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Open)
        );

        // Wait for reset timeout
        std::thread::sleep(Duration::from_millis(60));

        // Should transition to half-open on next allow_request
        assert!(registry.allow_request(endpoint));
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::HalfOpen)
        );

        // Two successes should close it (success_threshold = 2)
        registry.record_success(endpoint);
        registry.record_success(endpoint);
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Closed)
        );
    }

    #[test]
    fn test_circuit_breaker_half_open_failure_reopens() {
        let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            min_calls: 4,
            success_threshold: 3,
            reset_timeout: Duration::from_millis(50),
            buffer_size: 10,
        });

        let endpoint = "http://test.com/api";

        // Trip the breaker
        for _ in 0..5 {
            registry.record_failure(endpoint);
        }
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Open)
        );

        // Wait for reset timeout
        std::thread::sleep(Duration::from_millis(60));

        // Transition to half-open
        assert!(registry.allow_request(endpoint));
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::HalfOpen)
        );

        // A failure in half-open should reopen
        registry.record_failure(endpoint);
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Open)
        );
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let registry = CircuitBreakerRegistry::default();
        let endpoint = "http://test.com/api";

        for _ in 0..15 {
            registry.record_failure(endpoint);
        }
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Open)
        );

        assert!(registry.reset(endpoint));
        assert_eq!(
            registry.get_state(endpoint),
            Some(CircuitBreakerState::Closed)
        );
    }

    #[test]
    fn test_get_all_stats() {
        let registry = CircuitBreakerRegistry::default();

        registry.record_success("http://api1.com");
        registry.record_success("http://api2.com");
        registry.record_failure("http://api2.com");

        let stats = registry.get_all_stats();
        assert_eq!(stats.len(), 2);

        let api1_stats = stats.get("http://api1.com").unwrap();
        assert_eq!(api1_stats.successful_calls, 1);
        assert_eq!(api1_stats.failed_calls, 0);

        let api2_stats = stats.get("http://api2.com").unwrap();
        assert_eq!(api2_stats.successful_calls, 1);
        assert_eq!(api2_stats.failed_calls, 1);
    }

    // ------------------------------------------------------------------
    // R-12: breaker key = origin + path, query stripped
    // ------------------------------------------------------------------

    #[test]
    fn breaker_key_strips_query_string() {
        let a = breaker_key("https://example.com/webhook?x=1");
        let b = breaker_key("https://example.com/webhook?x=2");
        assert_eq!(a, b, "distinct query strings must share one breaker key");
        assert_eq!(a, "https://example.com/webhook");
    }

    #[test]
    fn breaker_key_strips_fragment() {
        let a = breaker_key("https://example.com/webhook#a");
        let b = breaker_key("https://example.com/webhook#b");
        assert_eq!(a, b);
        assert_eq!(a, "https://example.com/webhook");
    }

    #[test]
    fn breaker_key_distinguishes_paths() {
        let a = breaker_key("https://example.com/webhook-a");
        let b = breaker_key("https://example.com/webhook-b");
        assert_ne!(a, b, "distinct paths must NOT share one breaker key");
    }

    #[test]
    fn breaker_key_distinguishes_hosts_and_ports() {
        let a = breaker_key("https://a.example.com/webhook");
        let b = breaker_key("https://b.example.com/webhook");
        assert_ne!(a, b);

        let c = breaker_key("https://example.com:8443/webhook");
        let d = breaker_key("https://example.com:9443/webhook");
        assert_ne!(c, d);
    }

    #[test]
    fn breaker_key_falls_back_to_raw_string_on_parse_failure() {
        // No authority at all — `url::Url::parse` fails.
        let key = breaker_key("http://");
        assert_eq!(key, "http://");
    }

    #[test]
    fn distinct_query_strings_share_one_registered_breaker() {
        let registry = CircuitBreakerRegistry::default();
        let key_a = breaker_key("https://example.com/webhook?msg=1");
        let key_b = breaker_key("https://example.com/webhook?msg=2");

        registry.record_success(&key_a);
        registry.record_failure(&key_b);

        // Both calls landed on the SAME breaker entry.
        let stats = registry.get_stats(&key_a).unwrap();
        assert_eq!(stats.successful_calls, 1);
        assert_eq!(stats.failed_calls, 1);
        assert_eq!(registry.get_all_stats().len(), 1);
    }

    #[test]
    fn test_double_check_get_or_create() {
        let registry = CircuitBreakerRegistry::default();
        let endpoint = "http://test.com/api";

        // First call creates
        registry.record_success(endpoint);
        // Second call reuses
        registry.record_success(endpoint);

        let stats = registry.get_stats(endpoint).unwrap();
        assert_eq!(stats.successful_calls, 2);
    }
}

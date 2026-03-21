//! Circuit Breaker Registry - Per-endpoint circuit breaker tracking
//!
//! Provides centralized tracking of circuit breakers for monitoring purposes.
//! Compatible with Java's Resilience4j circuit breaker stats format.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Circuit breaker state (matches Java Resilience4j states)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CircuitBreakerState {
    /// Circuit is closed (normal operation)
    Closed,
    /// Circuit is open (rejecting requests)
    Open,
    /// Circuit is testing (allowing limited requests)
    HalfOpen,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self::Closed
    }
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

/// Per-endpoint circuit breaker tracking
struct EndpointCircuitBreaker {
    name: String,
    state: RwLock<CircuitBreakerState>,
    successful_calls: AtomicU64,
    failed_calls: AtomicU64,
    rejected_calls: AtomicU64,
    last_failure_time: RwLock<Option<Instant>>,
    last_state_change: RwLock<Instant>,
    /// Last time any call (success, failure, or rejected) was recorded
    last_activity: RwLock<Instant>,
    half_open_success_count: RwLock<u32>,

    // Configuration
    failure_rate_threshold: f64,
    min_calls: u32,
    success_threshold: u32,
    reset_timeout: Duration,
    buffer_size: u32,

    // Sliding window for failure rate calculation
    recent_results: RwLock<Vec<bool>>,
}

impl EndpointCircuitBreaker {
    fn new(name: String, config: &CircuitBreakerConfig) -> Self {
        Self {
            name,
            state: RwLock::new(CircuitBreakerState::Closed),
            successful_calls: AtomicU64::new(0),
            failed_calls: AtomicU64::new(0),
            rejected_calls: AtomicU64::new(0),
            last_failure_time: RwLock::new(None),
            last_state_change: RwLock::new(Instant::now()),
            last_activity: RwLock::new(Instant::now()),
            half_open_success_count: RwLock::new(0),
            failure_rate_threshold: config.failure_rate_threshold,
            min_calls: config.min_calls,
            success_threshold: config.success_threshold,
            reset_timeout: config.reset_timeout,
            buffer_size: config.buffer_size,
            recent_results: RwLock::new(Vec::with_capacity(config.buffer_size as usize)),
        }
    }

    /// Calculate current failure rate from the sliding window
    fn failure_rate(results: &[bool]) -> f64 {
        if results.is_empty() {
            return 0.0;
        }
        let failures = results.iter().filter(|&&s| !s).count();
        failures as f64 / results.len() as f64
    }

    fn record_success(&self) {
        self.successful_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_activity.write() = Instant::now();

        let mut results = self.recent_results.write();
        if results.len() >= self.buffer_size as usize {
            results.remove(0);
        }
        results.push(true);

        let state = *self.state.read();
        if state == CircuitBreakerState::HalfOpen {
            let mut count = self.half_open_success_count.write();
            *count += 1;
            if *count >= self.success_threshold {
                *self.state.write() = CircuitBreakerState::Closed;
                *self.last_state_change.write() = Instant::now();
                results.clear(); // Reset window on close (matches Java/TypeScript)
                *count = 0;
            }
        }
    }

    fn record_failure(&self) {
        self.failed_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_activity.write() = Instant::now();
        *self.last_failure_time.write() = Some(Instant::now());

        let mut results = self.recent_results.write();
        if results.len() >= self.buffer_size as usize {
            results.remove(0);
        }
        results.push(false);

        let state = *self.state.read();
        match state {
            CircuitBreakerState::Closed => {
                // Ratio-based tripping: only evaluate when we have enough calls
                if results.len() >= self.min_calls as usize {
                    let rate = Self::failure_rate(&results);
                    if rate >= self.failure_rate_threshold {
                        *self.state.write() = CircuitBreakerState::Open;
                        *self.last_state_change.write() = Instant::now();
                    }
                }
            }
            CircuitBreakerState::HalfOpen => {
                // Any failure in half-open immediately reopens
                *self.state.write() = CircuitBreakerState::Open;
                *self.last_state_change.write() = Instant::now();
                *self.half_open_success_count.write() = 0;
            }
            CircuitBreakerState::Open => {}
        }
    }

    fn record_rejected(&self) {
        self.rejected_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_activity.write() = Instant::now();
    }

    fn allow_request(&self) -> bool {
        let state = *self.state.read();

        match state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                // Check if we should transition to half-open
                if let Some(last_failure) = *self.last_failure_time.read() {
                    if last_failure.elapsed() >= self.reset_timeout {
                        *self.state.write() = CircuitBreakerState::HalfOpen;
                        *self.last_state_change.write() = Instant::now();
                        return true;
                    }
                }
                false
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    fn get_stats(&self) -> CircuitBreakerStats {
        let results = self.recent_results.read();

        CircuitBreakerStats {
            name: self.name.clone(),
            state: *self.state.read(),
            successful_calls: self.successful_calls.load(Ordering::Relaxed),
            failed_calls: self.failed_calls.load(Ordering::Relaxed),
            rejected_calls: self.rejected_calls.load(Ordering::Relaxed),
            failure_rate: Self::failure_rate(&results),
            buffered_calls: results.len() as u32,
            buffer_size: self.buffer_size,
        }
    }

    fn reset(&self) {
        *self.state.write() = CircuitBreakerState::Closed;
        *self.last_state_change.write() = Instant::now();
        *self.last_failure_time.write() = None;
        *self.half_open_success_count.write() = 0;
        self.recent_results.write().clear();
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
            failure_rate_threshold: 0.5,  // Java: failureRatio=0.5
            min_calls: 10,                // Java: requestVolumeThreshold=10
            success_threshold: 3,         // Java: successThreshold=3
            reset_timeout: Duration::from_secs(5), // Java: delay=5000
            buffer_size: 100,             // Sliding window size
        }
    }
}

/// Registry for per-endpoint circuit breakers
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
        // Try read first
        {
            let breakers = self.breakers.read();
            if let Some(breaker) = breakers.get(endpoint) {
                return Arc::clone(breaker);
            }
        }

        // Create new
        let mut breakers = self.breakers.write();
        let breaker = Arc::new(EndpointCircuitBreaker::new(
            endpoint.to_string(),
            &self.config,
        ));
        breakers.insert(endpoint.to_string(), Arc::clone(&breaker));
        breaker
    }

    /// Check if request should be allowed for endpoint
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
        breakers.get(endpoint).map(|b| *b.state.read())
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
        // Skip when registry is empty (zero cost)
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
            .filter(|b| *b.state.read() == CircuitBreakerState::Open)
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
            min_calls: 4,        // Low threshold for testing
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

        assert_eq!(registry.get_state(endpoint), Some(CircuitBreakerState::Open));
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

        assert_eq!(registry.get_state(endpoint), Some(CircuitBreakerState::Closed));
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let registry = CircuitBreakerRegistry::default();
        let endpoint = "http://test.com/api";

        // Trip the breaker with many failures
        for _ in 0..15 {
            registry.record_failure(endpoint);
        }

        assert_eq!(registry.get_state(endpoint), Some(CircuitBreakerState::Open));

        // Reset it
        assert!(registry.reset(endpoint));
        assert_eq!(registry.get_state(endpoint), Some(CircuitBreakerState::Closed));
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
}

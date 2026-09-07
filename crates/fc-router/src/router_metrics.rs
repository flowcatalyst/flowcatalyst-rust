//! Metrics infrastructure for the message router
//!
//! Provides Prometheus-compatible metrics for:
//! - Message processing counts
//! - Mediation latency
//! - Pool statistics
//! - Queue sizes

use metrics::{counter, gauge, histogram};
use std::time::Duration;

/// Record a message being processed
pub fn record_message_processed(pool_code: &str, success: bool, result: &str) {
    counter!(
        "fc_messages_processed_total",
        "pool" => pool_code.to_string(),
        "success" => success.to_string(),
        "result" => result.to_string()
    )
    .increment(1);
}

/// Record mediation latency
pub fn record_mediation_latency(pool_code: &str, duration: Duration) {
    histogram!(
        "fc_mediation_duration_seconds",
        "pool" => pool_code.to_string()
    )
    .record(duration.as_secs_f64());
}

/// Record the HTTP protocol version a mediation request actually
/// negotiated with the target (item 3, owner ruling 2026-09-07: deployed
/// mode is supposed to speak h2c/h2, not silently fall back to HTTP/1.1 —
/// this is how an operator (or a bench run) confirms it did). Recorded
/// once per completed HTTP response, in `HttpMediator::mediate_once`,
/// using `reqwest::Response::version()`'s `Debug` form (`"HTTP/2.0"`,
/// `"HTTP/1.1"`, …) as the label so it reads the same as the bench rig's
/// own `proto_counts`.
pub fn record_mediation_http_version(version: &str) {
    counter!(
        "fc_mediation_http_version_total",
        "version" => version.to_string()
    )
    .increment(1);
}

/// Record rate limit exceeded
pub fn record_rate_limit_exceeded(pool_code: &str) {
    counter!(
        "fc_rate_limit_exceeded_total",
        "pool" => pool_code.to_string()
    )
    .increment(1);
}

/// Update pool queue size gauge
pub fn set_pool_queue_size(pool_code: &str, size: u32) {
    gauge!(
        "fc_pool_queue_size",
        "pool" => pool_code.to_string()
    )
    .set(size as f64);
}

/// Update pool active workers gauge
pub fn set_pool_active_workers(pool_code: &str, count: u32) {
    gauge!(
        "fc_pool_active_workers",
        "pool" => pool_code.to_string()
    )
    .set(count as f64);
}

/// Update pool message group count
pub fn set_pool_message_groups(pool_code: &str, count: u32) {
    gauge!(
        "fc_pool_message_groups",
        "pool" => pool_code.to_string()
    )
    .set(count as f64);
}

/// Record a message being submitted to a pool
pub fn record_message_submitted(pool_code: &str) {
    counter!(
        "fc_messages_submitted_total",
        "pool" => pool_code.to_string()
    )
    .increment(1);
}

/// Record a message being rejected (pool at capacity)
pub fn record_message_rejected(pool_code: &str, reason: &str) {
    counter!(
        "fc_messages_rejected_total",
        "pool" => pool_code.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
}

/// Update in-pipeline message count
pub fn set_in_pipeline_count(count: usize) {
    gauge!("fc_in_pipeline_messages").set(count as f64);
}

/// Record consumer poll
pub fn record_consumer_poll(consumer: &str, message_count: u32) {
    counter!(
        "fc_consumer_polls_total",
        "consumer" => consumer.to_string()
    )
    .increment(1);

    if message_count > 0 {
        counter!(
            "fc_consumer_messages_received_total",
            "consumer" => consumer.to_string()
        )
        .increment(message_count as u64);
    }
}

/// Record consumer error
pub fn record_consumer_error(consumer: &str, error_type: &str) {
    counter!(
        "fc_consumer_errors_total",
        "consumer" => consumer.to_string(),
        "type" => error_type.to_string()
    )
    .increment(1);
}

// --- Broker connectivity metrics (Java: BrokerHealthService) ---

/// Record a broker connectivity check attempt
pub fn record_broker_connection_attempt() {
    counter!("flowcatalyst_broker_connection_attempts").increment(1);
}

/// Record a successful broker connectivity check
pub fn record_broker_connection_success() {
    counter!("flowcatalyst_broker_connection_successes").increment(1);
}

/// Record a failed broker connectivity check
pub fn record_broker_connection_failure() {
    counter!("flowcatalyst_broker_connection_failures").increment(1);
}

/// Update the broker availability gauge (1.0 = available, 0.0 = unavailable)
pub fn set_broker_available(available: bool) {
    gauge!("flowcatalyst_broker_available").set(if available { 1.0 } else { 0.0 });
}

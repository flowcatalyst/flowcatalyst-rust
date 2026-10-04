//! Series recorded through the global `metrics` recorder as things happen:
//! the negotiated HTTP version and broker connectivity.
//!
//! The pool, queue and breaker series are not here. They are rendered from
//! snapshots at scrape time by `api::prometheus`, so a removed pool drops
//! out of the scrape instead of keeping its last value.

use metrics::{counter, gauge};

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

/// [`record_mediation_http_version`] for a `reqwest::Version`, labelled with
/// the same `Debug` form (`"HTTP/2.0"`, …) but from static strings, so the
/// per-response label costs no allocation.
pub fn record_mediation_http_version_of(version: reqwest::Version) {
    let label: &'static str = match version {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_11 => "HTTP/1.1",
        reqwest::Version::HTTP_2 => "HTTP/2.0",
        reqwest::Version::HTTP_3 => "HTTP/3.0",
        other => return record_mediation_http_version(&format!("{other:?}")),
    };
    counter!("fc_mediation_http_version_total", "version" => label).increment(1);
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

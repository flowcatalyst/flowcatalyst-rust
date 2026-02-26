//! Structured Logging Configuration
//!
//! Provides configurable logging with:
//! - JSON output for production (LOG_FORMAT=json)
//! - Human-readable output for development (default)
//! - Context fields via spans (request_id, correlation_id, etc.)
//!
//! # Usage
//!
//! ```rust,ignore
//! use fc_common::logging::init_logging;
//!
//! fn main() {
//!     init_logging("my-service");
//!
//!     // Use tracing macros with structured fields
//!     tracing::info!(user_id = %id, "User logged in");
//! }
//! ```
//!
//! # Environment Variables
//!
//! - `LOG_FORMAT`: Set to "json" for JSON output, anything else for text (default: text)
//! - `RUST_LOG`: Standard log level filter (default: info)
//!   Examples: `RUST_LOG=debug`, `RUST_LOG=fc_router=trace,tower_http=info`
//!
//! # Adding Context to Requests
//!
//! Use spans to add context that propagates through all nested log calls:
//!
//! ```rust,ignore
//! use tracing::{info_span, Instrument};
//!
//! async fn handle_request(req: Request) {
//!     let span = info_span!(
//!         "request",
//!         request_id = %req.id,
//!         correlation_id = %req.correlation_id,
//!         client_id = %req.client_id,
//!     );
//!
//!     async {
//!         // All logs here include the span fields
//!         tracing::info!("Processing request");
//!         do_work().await;
//!     }.instrument(span).await;
//! }
//! ```

use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Initialize logging with the given service name.
///
/// Reads LOG_FORMAT env var to determine output format:
/// - "json" -> JSON output (for production/log aggregation)
/// - anything else -> human-readable text (for development)
///
/// Reads RUST_LOG env var for log level filtering (defaults to INFO).
pub fn init_logging(_service_name: &str) {
    let log_format = std::env::var("LOG_FORMAT").unwrap_or_default();

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    if log_format.eq_ignore_ascii_case("json") {
        init_json_logging(env_filter);
    } else {
        init_text_logging(env_filter);
    }
}

/// Initialize JSON logging for production.
fn init_json_logging(env_filter: EnvFilter) {
    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(false)
                .with_target(true)
                .flatten_event(true)
                .with_span_events(FmtSpan::CLOSE)
        )
        .init();
}

/// Initialize human-readable text logging for development.
fn init_text_logging(env_filter: EnvFilter) {
    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_ansi(true)
        )
        .init();
}

/// Initialize logging with defaults (uses "flowcatalyst" as service name).
pub fn init_default_logging() {
    init_logging("flowcatalyst");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_filter_parsing() {
        // Just verify the filter can be created
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"));
        drop(filter);
    }
}

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
//! - `RUST_LOG`: Standard log level filter (default: Go's `FC_LOG_LEVEL`, else info)
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

use std::sync::atomic::{AtomicBool, Ordering};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer, Registry,
};

/// Initialize logging with the given service name.
///
/// Reads LOG_FORMAT env var to determine output format:
/// - "json" -> JSON output (for production/log aggregation)
/// - anything else -> human-readable text (for development)
///
/// Reads RUST_LOG env var for log level filtering; when it is unset, Go's
/// FC_LOG_LEVEL (debug/warn/error, default info).
///
/// Also installs the diagnostics panic hook ([`crate::diagnostics::init`])
/// and, when built in and switched on, the tokio-console and OTLP layers
/// (see [`extra_layers`]).
pub fn init_logging(service_name: &str) {
    let log_format = std::env::var("LOG_FORMAT").unwrap_or_default();
    install(service_name, log_format.eq_ignore_ascii_case("json"));
}

/// Initialize logging for a production server, as Go's fc-server logs:
/// JSON unless `LOG_FORMAT` says otherwise (`text`), and the level from
/// `RUST_LOG`, else Go's `FC_LOG_LEVEL`, else info.
pub fn init_production_logging(service_name: &str) {
    let format = std::env::var("LOG_FORMAT").unwrap_or_default();
    install(
        service_name,
        format.is_empty() || format.eq_ignore_ascii_case("json"),
    );
}

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(go_log_level(std::env::var("FC_LOG_LEVEL").ok())))
}

/// Go's `FC_LOG_LEVEL` (`internal/logging`), the fallback when `RUST_LOG`
/// is unset: `debug`, `warn`/`warning` and `error`; anything else is
/// `info`. Case-insensitive (Go matches the all-lower and all-upper
/// spellings; a mixed-case value here is read, not dropped to `info`).
fn go_log_level(raw: Option<String>) -> &'static str {
    match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("debug") => "debug",
        Some("warn" | "warning") => "warn",
        Some("error") => "error",
        _ => "info",
    }
}

/// Span lifecycle lines: none by default. The pipeline's spans (one per
/// message, job and poll tick) exist to put their ids on every line logged
/// inside them, not to log themselves; `FC_LOG_SPAN_EVENTS=close` adds a
/// line with each span's duration when it closes (debugging only: it is a
/// line per message).
fn span_events() -> FmtSpan {
    match std::env::var("FC_LOG_SPAN_EVENTS")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "close" => FmtSpan::CLOSE,
        "full" => FmtSpan::FULL,
        _ => FmtSpan::NONE,
    }
}

/// The JSON line layer: each event carries its span (`span`) and the whole
/// span stack (`spans`, outermost first) with their fields, so a line
/// logged anywhere inside a message's processing carries its
/// `message_id`, `pool` and `group`.
macro_rules! json_layer {
    () => {
        fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(false)
            .with_target(true)
            .flatten_event(true)
            .with_span_events(span_events())
    };
}

macro_rules! text_layer {
    () => {
        fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .with_ansi(true)
            .with_span_events(span_events())
    };
}

/// Install the subscriber: the env filter and the chosen line format,
/// plus [`extra_layers`]. With no extra layer the filter is global (as it
/// always was); with one it filters the log lines only, so tokio-console
/// still sees the runtime's own trace-level events.
fn install(service_name: &str, json: bool) {
    let extras = extra_layers(service_name);
    let result = if extras.is_empty() {
        if json {
            tracing_subscriber::registry()
                .with(env_filter())
                .with(json_layer!())
                .try_init()
        } else {
            tracing_subscriber::registry()
                .with(env_filter())
                .with(text_layer!())
                .try_init()
        }
    } else if json {
        tracing_subscriber::registry()
            .with(extras)
            .with(json_layer!().with_filter(env_filter()))
            .try_init()
    } else {
        tracing_subscriber::registry()
            .with(extras)
            .with(text_layer!().with_filter(env_filter()))
            .try_init()
    };
    if result.is_err() {
        // A second init in one process (tests, subcommands): keep the first.
        return;
    }
    crate::diagnostics::init();
}

type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync>;

static TOKIO_CONSOLE: AtomicBool = AtomicBool::new(false);

/// Whether the tokio-console layer is running in this process.
pub fn tokio_console_enabled() -> bool {
    TOKIO_CONSOLE.load(Ordering::Relaxed)
}

fn env_true(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The optional layers, each compiled in by a Cargo feature and switched on
/// by an environment variable, so a build that has them costs nothing
/// until an operator asks:
///
/// - `tokio-console` + `FC_TOKIO_CONSOLE=true`: the console's gRPC server
///   on `FC_TOKIO_CONSOLE_BIND` (default `127.0.0.1:6669`, localhost only;
///   reach it through an SSM port-forward).
/// - `otel` + `FC_OTEL_ENABLED=true`: spans exported over OTLP/HTTP to
///   `OTEL_EXPORTER_OTLP_ENDPOINT` (default `http://localhost:4318`), as
///   `OTEL_SERVICE_NAME` (default the binary's name).
///
/// A layer asked for by its variable in a build without its feature is
/// reported on stderr and skipped.
fn extra_layers(service_name: &str) -> Vec<BoxedLayer> {
    let mut layers: Vec<BoxedLayer> = Vec::new();
    if env_true("FC_TOKIO_CONSOLE") {
        match console_layer() {
            Some(layer) => {
                TOKIO_CONSOLE.store(true, Ordering::Relaxed);
                layers.push(layer);
            }
            None => eprintln!(
                "FC_TOKIO_CONSOLE is set but this build has no tokio-console support \
                 (build with --features tokio-console and RUSTFLAGS=\"--cfg tokio_unstable\")"
            ),
        }
    }
    if env_true("FC_OTEL_ENABLED") {
        match otel::layer(service_name) {
            Ok(Some(layer)) => layers.push(layer),
            Ok(None) => eprintln!(
                "FC_OTEL_ENABLED is set but this build has no OpenTelemetry support \
                 (build with --features otel)"
            ),
            Err(e) => eprintln!("OpenTelemetry export not started: {e}"),
        }
    }
    layers
}

#[cfg(feature = "tokio-console")]
fn console_layer() -> Option<BoxedLayer> {
    let bind = std::env::var("FC_TOKIO_CONSOLE_BIND")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:6669".to_string());
    let addr: std::net::SocketAddr = match bind.trim().parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("FC_TOKIO_CONSOLE_BIND={bind:?} is not an address ({e}); tokio-console off");
            return None;
        }
    };
    Some(Box::new(
        console_subscriber::ConsoleLayer::builder()
            .server_addr(addr)
            .spawn(),
    ))
}

#[cfg(not(feature = "tokio-console"))]
fn console_layer() -> Option<BoxedLayer> {
    None
}

/// Flush and stop whatever the optional layers buffer (the OTLP batch
/// exporter). Binaries call it last, after their shutdown.
pub fn shutdown() {
    otel::shutdown();
}

#[cfg(feature = "otel")]
mod otel {
    //! OTLP/HTTP (protobuf) span export through the workspace's reqwest.
    //! The SDK's batch processor runs on its own thread without a tokio
    //! runtime, so each export is spawned onto the runtime that installed
    //! the layer and awaited from there.

    use super::BoxedLayer;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_http::{Bytes, HttpError, Request, Response};
    use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
    use std::sync::OnceLock;
    use tracing_subscriber::Layer;

    static PROVIDER: OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> = OnceLock::new();

    #[derive(Debug)]
    struct Client {
        http: reqwest::Client,
        runtime: tokio::runtime::Handle,
    }

    #[async_trait::async_trait]
    impl opentelemetry_http::HttpClient for Client {
        async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
            let http = self.http.clone();
            self.runtime
                .spawn(async move {
                    let (parts, body) = request.into_parts();
                    let mut req = http.request(parts.method, parts.uri.to_string()).body(body);
                    for (name, value) in parts.headers.iter() {
                        req = req.header(name, value);
                    }
                    let resp = req.send().await?;
                    let status = resp.status();
                    let headers = resp.headers().clone();
                    let body = resp.bytes().await?;
                    let mut out = Response::builder().status(status);
                    for (name, value) in headers.iter() {
                        out = out.header(name, value);
                    }
                    Ok::<_, HttpError>(out.body(body)?)
                })
                .await?
        }
    }

    pub(super) fn layer(service_name: &str) -> Result<Option<BoxedLayer>, String> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "no tokio runtime to export from".to_string())?;
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "http://localhost:4318".to_string());
        let traces = format!("{}/v1/traces", endpoint.trim_end_matches('/'));
        let service = std::env::var("OTEL_SERVICE_NAME")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| service_name.to_string());
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_http_client(Client { http, runtime })
            .with_endpoint(traces)
            .build()
            .map_err(|e| e.to_string())?;
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(
                opentelemetry_sdk::Resource::builder()
                    .with_service_name(service)
                    .build(),
            )
            .build();
        let tracer = provider.tracer("flowcatalyst");
        let _ = PROVIDER.set(provider);
        Ok(Some(Box::new(
            tracing_opentelemetry::layer()
                .with_tracer(tracer)
                .with_filter(super::env_filter()),
        )))
    }

    pub(super) fn shutdown() {
        if let Some(p) = PROVIDER.get() {
            let _ = p.shutdown();
        }
    }
}

#[cfg(not(feature = "otel"))]
mod otel {
    use super::BoxedLayer;

    pub(super) fn layer(_: &str) -> Result<Option<BoxedLayer>, String> {
        Ok(None)
    }

    pub(super) fn shutdown() {}
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
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        drop(filter);
    }

    /// A buffer the JSON layer writes into.
    #[derive(Clone, Default)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Capture {
            self.clone()
        }
    }

    /// Correlation: a line logged anywhere inside a message's span carries
    /// the span's fields, and spans do not log themselves by default.
    #[test]
    fn json_lines_carry_the_enclosing_span_fields() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("info"))
            .with(json_layer!().with_writer(capture.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "router.dispatch",
                message_id = "msg-1",
                pool = "POOL-A",
                group = "g-7"
            );
            let _entered = span.enter();
            tracing::warn!(status = 503, "target unavailable");
        });
        let out = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1, "no span open/close lines: {out}");
        let line: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(line["span"]["message_id"], "msg-1");
        assert_eq!(line["span"]["pool"], "POOL-A");
        assert_eq!(line["spans"][0]["group"], "g-7");
        assert_eq!(line["status"], 503);
    }

    #[test]
    fn go_log_level_names() {
        assert_eq!(go_log_level(None), "info");
        assert_eq!(go_log_level(Some("DEBUG".into())), "debug");
        assert_eq!(go_log_level(Some("warning".into())), "warn");
        assert_eq!(go_log_level(Some("ERROR".into())), "error");
        assert_eq!(go_log_level(Some("Debug".into())), "debug");
        assert_eq!(go_log_level(Some("verbose".into())), "info");
    }
}

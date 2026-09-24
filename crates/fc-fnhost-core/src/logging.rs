//! Structured logging in the platform's one JSON line shape (Java
//! `server/Logging.java` + `GoJsonEncoder.java`, spec `docs/spec/logging.md`):
//! Go's `slog.NewJSONHandler` shape, so Go, Java and Rust logs aggregate on
//! the same field names.
//!
//! One JSON object per event on stderr, keys in this order: `time`, `level`,
//! `msg`, the span fields (Java's MDC; outermost span first), the event's
//! own fields in call order, then `logger` (the `tracing` target), `thread`,
//! and `err` / `stack` when the event carries them. A field that repeats a
//! reserved key or an earlier field is written `kv_<key>`.
//!
//! Configuration, as Java:
//! - level: `FC_LOG_LEVEL` (`debug`/`trace` → DEBUG, `warn`/`warning`,
//!   `error`, anything else INFO) wins outright; otherwise `RUST_LOG` (a full
//!   `tracing` filter here, the superset of Java's reading of it); default INFO.
//! - format: `FC_LOG_FORMAT`, alias `LOG_FORMAT`: `text`/`plain`/`console`/
//!   `pretty` or `json`; unset means text on an interactive stderr, JSON otherwise.

use std::fmt::{self, Write as _};
use std::io::{IsTerminal, Write};

use chrono::{Local, Offset};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Level, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::env::EnvReader;

const RESERVED: [&str; 7] = ["time", "level", "msg", "logger", "thread", "err", "stack"];

/// An event field that names the event's `logger` in place of its `tracing`
/// target, which must be a compile-time string. The WASM runtime logs a
/// guest's lines with it (`fc_logger = "fn.<address>"`, Java's per-function
/// logger); the JSON layer writes the value as `logger` and drops the field.
pub const LOGGER_FIELD: &str = "fc_logger";

/// The structured-log field names shared with the Go and Java platforms.
pub mod keys {
    pub const CORRELATION_ID: &str = "correlation_id";
    pub const CAUSATION_ID: &str = "causation_id";
    pub const PRINCIPAL_ID: &str = "principal_id";
    pub const EXECUTION_ID: &str = "execution_id";
    pub const FUNCTION: &str = "function";
    pub const VERSION: &str = "version";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Text,
}

/// `FC_LOG_FORMAT` / `LOG_FORMAT` → format.
pub fn format_of(raw: &str, stderr_is_terminal: bool) -> Format {
    match raw.trim().to_ascii_lowercase().as_str() {
        "text" | "plain" | "console" | "pretty" => Format::Text,
        "json" => Format::Json,
        _ if stderr_is_terminal => Format::Text,
        _ => Format::Json,
    }
}

/// The filter directive: `FC_LOG_LEVEL` wins outright, else `RUST_LOG`,
/// else `info`.
pub fn filter_directive(env: &EnvReader) -> String {
    let fc_level = env.get("FC_LOG_LEVEL");
    if !crate::java::is_blank(fc_level) {
        return match fc_level.trim().to_ascii_lowercase().as_str() {
            "debug" | "trace" => "debug",
            "warn" | "warning" => "warn",
            "error" => "error",
            _ => "info",
        }
        .to_owned();
    }
    match env.get("RUST_LOG").trim() {
        "" => "info".to_owned(),
        rust_log => rust_log.to_owned(),
    }
}

/// Installs the global subscriber. Safe to call more than once: later calls
/// are no-ops (tests start several hosts in one process).
pub fn init(env: &EnvReader) {
    let format = format_of(
        env.first_set(&["FC_LOG_FORMAT", "LOG_FORMAT"])
            .unwrap_or(""),
        std::io::stderr().is_terminal(),
    );
    let filter =
        EnvFilter::try_new(filter_directive(env)).unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    let _ = match format {
        Format::Json => registry
            .with(SlogJsonLayer::new(std::io::stderr))
            .try_init(),
        Format::Text => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(true),
            )
            .try_init(),
    };
}

/// The JSON line layer. Generic over its writer so tests can capture lines.
pub struct SlogJsonLayer<W> {
    make_writer: W,
}

impl<W> SlogJsonLayer<W> {
    pub fn new(make_writer: W) -> Self {
        Self { make_writer }
    }
}

/// A span's recorded fields, in record order (the MDC equivalent).
struct SpanFields(Vec<(String, JsonValue)>);

#[derive(Debug, Clone)]
enum JsonValue {
    Str(String),
    Raw(String),
}

impl JsonValue {
    fn into_text(self) -> String {
        match self {
            JsonValue::Str(s) | JsonValue::Raw(s) => s,
        }
    }
}

#[derive(Default)]
struct FieldCollector {
    message: Option<String>,
    err: Option<String>,
    stack: Option<String>,
    /// [`LOGGER_FIELD`], when the event carries it.
    logger: Option<String>,
    fields: Vec<(String, JsonValue)>,
}

impl FieldCollector {
    fn push(&mut self, field: &Field, value: JsonValue) {
        match field.name() {
            "message" => self.message = Some(value.into_text()),
            "err" if self.err.is_none() => self.err = Some(value.into_text()),
            "stack" if self.stack.is_none() => self.stack = Some(value.into_text()),
            LOGGER_FIELD if self.logger.is_none() => self.logger = Some(value.into_text()),
            name => self.fields.push((name.to_owned(), value)),
        }
    }
}

impl Visit for FieldCollector {
    fn record_f64(&mut self, field: &Field, value: f64) {
        let rendered = if value.is_finite() {
            // Java's Double.toString keeps a ".0" on integral values
            if value.fract() == 0.0 && value.abs() < 1e7 {
                JsonValue::Raw(format!("{value:.1}"))
            } else {
                JsonValue::Raw(format!("{value}"))
            }
        } else {
            JsonValue::Str(format!("{value}"))
        };
        self.push(field, rendered);
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field, JsonValue::Raw(value.to_string()));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field, JsonValue::Raw(value.to_string()));
    }
    fn record_i128(&mut self, field: &Field, value: i128) {
        self.push(field, JsonValue::Raw(value.to_string()));
    }
    fn record_u128(&mut self, field: &Field, value: u128) {
        self.push(field, JsonValue::Raw(value.to_string()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field, JsonValue::Raw(value.to_string()));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, JsonValue::Str(value.to_owned()));
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.push(field, JsonValue::Str(value.to_string()));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.push(field, JsonValue::Str(format!("{value:?}")));
    }
}

impl<S, W> Layer<S> for SlogJsonLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut collector = FieldCollector::default();
        attrs.record(&mut collector);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(collector.fields));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut collector = FieldCollector::default();
        values.record(&mut collector);
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            if let Some(fields) = extensions.get_mut::<SpanFields>() {
                for (key, value) in collector.fields {
                    match fields.0.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = value,
                        None => fields.0.push((key, value)),
                    }
                }
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut collector = FieldCollector::default();
        event.record(&mut collector);
        let mut mdc: Vec<(String, JsonValue)> = Vec::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(fields) = span.extensions().get::<SpanFields>() {
                    mdc.extend(fields.0.iter().cloned());
                }
            }
        }
        let thread = std::thread::current();
        let line = render_line(
            &now_rfc3339(),
            *event.metadata().level(),
            collector.message.as_deref().unwrap_or(""),
            &mdc,
            &collector.fields,
            collector
                .logger
                .as_deref()
                .unwrap_or(event.metadata().target()),
            thread.name().unwrap_or("unnamed"),
            collector.err.as_deref(),
            collector.stack.as_deref(),
        );
        let mut writer = self.make_writer.make_writer();
        let _ = writer.write_all(line.as_bytes());
    }
}

/// RFC 3339 in the process's zone with six fractional digits, `Z` when the
/// offset is zero.
fn now_rfc3339() -> String {
    let now = Local::now();
    let base = now.format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
    if now.offset().fix().local_minus_utc() == 0 {
        format!("{base}Z")
    } else {
        format!("{base}{}", now.format("%:z"))
    }
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::TRACE | Level::DEBUG => "DEBUG",
        Level::INFO => "INFO",
        Level::WARN => "WARN",
        Level::ERROR => "ERROR",
    }
}

#[allow(clippy::too_many_arguments)]
fn render_line(
    time: &str,
    level: Level,
    msg: &str,
    mdc: &[(String, JsonValue)],
    fields: &[(String, JsonValue)],
    logger: &str,
    thread: &str,
    err: Option<&str>,
    stack: Option<&str>,
) -> String {
    let mut out = String::with_capacity(256);
    out.push('{');
    append(&mut out, true, "time", &JsonValue::Str(time.to_owned()));
    append(
        &mut out,
        false,
        "level",
        &JsonValue::Str(level_name(level).to_owned()),
    );
    append(&mut out, false, "msg", &JsonValue::Str(msg.to_owned()));
    let mut seen: Vec<String> = RESERVED.iter().map(|s| (*s).to_owned()).collect();
    for (key, value) in mdc.iter().chain(fields.iter()) {
        let rendered = if seen.iter().any(|s| s == key) {
            format!("kv_{key}")
        } else {
            key.clone()
        };
        seen.push(key.clone());
        append(&mut out, false, &rendered, value);
    }
    append(
        &mut out,
        false,
        "logger",
        &JsonValue::Str(logger.to_owned()),
    );
    append(
        &mut out,
        false,
        "thread",
        &JsonValue::Str(thread.to_owned()),
    );
    if let Some(err) = err {
        append(&mut out, false, "err", &JsonValue::Str(err.to_owned()));
    }
    // Rust errors carry no stack trace; `stack` appears only when a caller
    // records one explicitly.
    if let Some(stack) = stack {
        append(&mut out, false, "stack", &JsonValue::Str(stack.to_owned()));
    }
    out.push_str("}\n");
    out
}

fn append(out: &mut String, first: bool, key: &str, value: &JsonValue) {
    if !first {
        out.push(',');
    }
    out.push('"');
    escape(out, key);
    out.push_str("\":");
    match value {
        JsonValue::Raw(raw) => out.push_str(raw),
        JsonValue::Str(s) => {
            out.push('"');
            escape(out, s);
            out.push('"');
        }
    }
}

/// RFC 8259 escaping exactly as `GoJsonEncoder.escape`.
fn escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use serde_json::Value;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn capture(f: impl FnOnce()) -> Vec<String> {
        let sink = Capture::default();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("trace"))
            .with(SlogJsonLayer::new(sink.clone()));
        tracing::subscriber::with_default(subscriber, f);
        let bytes = sink.0.lock().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn keys(line: &str) -> Vec<String> {
        let v: Value = serde_json::from_str(line).unwrap();
        v.as_object().unwrap().keys().cloned().collect()
    }

    #[test]
    fn a_plain_line_has_exactly_the_slog_keys_in_order() {
        let lines = capture(|| tracing::info!(target: "x", "hello"));
        assert_eq!(lines.len(), 1);
        assert_eq!(
            keys(&lines[0]),
            ["time", "level", "msg", "logger", "thread"]
        );
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["msg"], "hello");
        assert_eq!(v["logger"], "x");
    }

    #[test]
    fn the_logger_field_names_the_logger_and_is_not_written_as_a_field() {
        let lines = capture(|| tracing::warn!(target: "fn", fc_logger = "fn.a.b.c", "hi"));
        assert_eq!(
            keys(&lines[0]),
            ["time", "level", "msg", "logger", "thread"]
        );
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["logger"], "fn.a.b.c");
    }

    #[test]
    fn fields_keep_types_and_order_before_logger() {
        let lines = capture(|| tracing::info!(queue = "q1", attempts = 3, ok = true, "m"));
        assert_eq!(
            keys(&lines[0]),
            ["time", "level", "msg", "queue", "attempts", "ok", "logger", "thread"]
        );
        assert!(lines[0].contains(r#""queue":"q1","attempts":3,"ok":true"#));
    }

    #[test]
    fn span_fields_are_the_mdc_and_come_first() {
        let lines = capture(|| {
            let span = tracing::info_span!("req", correlation_id = "abc");
            let _g = span.enter();
            tracing::info!(k = 1, "m");
        });
        assert_eq!(
            keys(&lines[0])[3..5],
            ["correlation_id".to_owned(), "k".to_owned()]
        );
        let after = capture(|| tracing::info!("m"));
        assert!(!after[0].contains("correlation_id"));
    }

    #[test]
    fn err_goes_to_its_own_slot_and_collisions_are_renamed() {
        let lines = capture(|| tracing::warn!(err = "boom", msg = "x", "the message"));
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["msg"], "the message");
        assert_eq!(v["kv_msg"], "x");
        assert_eq!(v["err"], "boom");
        assert_eq!(keys(&lines[0]).last().unwrap(), "err");
    }

    #[test]
    fn trace_maps_to_debug_and_time_has_six_digits() {
        let lines = capture(|| tracing::trace!("t"));
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["level"], "DEBUG");
        let time = v["time"].as_str().unwrap();
        let frac = time.split('.').nth(1).unwrap();
        assert!(frac[..6].bytes().all(|b| b.is_ascii_digit()));
        assert!(frac.len() == 7 || frac.len() == 12, "{time}");
    }

    #[test]
    fn escaping_round_trips() {
        let text = "q\"b\\n\nc\u{0001}";
        let lines = capture(|| tracing::info!(v = text, "m"));
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["v"], text);
        assert!(lines[0].contains("\\u0001"));
    }

    #[test]
    fn level_and_format_resolution() {
        let env = EnvReader::from_pairs([("FC_LOG_LEVEL", "Warning"), ("RUST_LOG", "trace")]);
        assert_eq!(filter_directive(&env), "warn");
        let env = EnvReader::from_pairs([("RUST_LOG", "fc_fnhost_core=debug")]);
        assert_eq!(filter_directive(&env), "fc_fnhost_core=debug");
        assert_eq!(filter_directive(&EnvReader::default()), "info");
        assert_eq!(format_of("pretty", false), Format::Text);
        assert_eq!(format_of("JSON", true), Format::Json);
        assert_eq!(format_of("", true), Format::Text);
        assert_eq!(format_of("", false), Format::Json);
    }
}

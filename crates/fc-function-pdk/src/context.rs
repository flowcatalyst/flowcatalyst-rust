use std::fmt;
use std::rc::Rc;
use std::time::SystemTime;

#[cfg(feature = "flowcatalyst")]
use fc_function_abi::{Caller, EventEmitError, FunctionAddress, OutboundEvent};

use crate::backend::Backend;
use crate::http::Http;

/// Everything a function may reach beyond its request: Java's
/// `FunctionContext`. Cheap to clone; one per invocation.
///
/// Without the `flowcatalyst` feature only [`logger`](Self::logger),
/// [`http`](Self::http) and [`now`](Self::now) exist: the rest needs the
/// FlowCatalyst host.
#[derive(Clone)]
pub struct Context {
    backend: Rc<dyn Backend>,
}

impl Context {
    pub(crate) fn new(backend: Rc<dyn Backend>) -> Self {
        Self { backend }
    }

    /// The current invocation: its id, the function's address and version,
    /// the caller, and the ids emitted events default to.
    #[cfg(feature = "flowcatalyst")]
    pub fn invocation(&self) -> &Invocation {
        self.backend.invocation()
    }

    /// The function's address (Java `FunctionContext.address()`).
    #[cfg(feature = "flowcatalyst")]
    pub fn address(&self) -> &FunctionAddress {
        &self.invocation().address
    }

    /// The loaded version handling this call (Java `FunctionContext.version()`).
    #[cfg(feature = "flowcatalyst")]
    pub fn version(&self) -> i32 {
        self.invocation().version
    }

    /// The manifest-declared config values.
    #[cfg(feature = "flowcatalyst")]
    pub fn config(&self) -> Config {
        Config {
            backend: self.backend.clone(),
        }
    }

    /// The manifest-declared secrets.
    #[cfg(feature = "flowcatalyst")]
    pub fn secrets(&self) -> Secrets {
        Secrets {
            backend: self.backend.clone(),
        }
    }

    /// Publishing events on the function's behalf.
    #[cfg(feature = "flowcatalyst")]
    pub fn events(&self) -> Events {
        Events {
            backend: self.backend.clone(),
        }
    }

    /// The function's logger (`fn.<address>` on the host, with the
    /// invocation's fields on every line).
    pub fn logger(&self) -> Logger {
        Logger {
            backend: self.backend.clone(),
        }
    }

    /// Outbound HTTP, under the manifest's `httpAllow`.
    pub fn http(&self) -> Http {
        Http::new(self.backend.clone())
    }

    /// The current time. Read it here rather than from `SystemTime::now()`
    /// so a test can fix it ([`crate::testing::TestHost::now`]).
    pub fn now(&self) -> SystemTime {
        self.backend.now()
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context").finish_non_exhaustive()
    }
}

/// What the host knows about the current invocation beyond the HTTP request
/// (the WIT `invocation-context`).
#[cfg(feature = "flowcatalyst")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Invocation {
    /// Unique per attempt: the `execution_id` on every log line.
    pub invocation_id: String,
    /// The function's address.
    pub address: FunctionAddress,
    /// The version handling the call.
    pub version: i32,
    /// Who made the call, decided by the matched endpoint's `auth`.
    pub caller: Caller,
    /// The correlation id events emitted by this invocation default to: the
    /// inbound event's on a verified webhook delivery, else the
    /// `X-Correlation-Id` header, else the invocation id.
    pub correlation_id: String,
    /// The causation id events emitted by this invocation default to: the
    /// inbound event's id on a verified webhook delivery of an event.
    pub causation_id: Option<String>,
    /// The `Host` the call arrived on.
    pub original_host: Option<String>,
    /// The path as it arrived, before the host stripped its
    /// `/functions/{address}` or public-route prefix.
    pub original_path: Option<String>,
    /// The caller's address (the right-most trusted `X-Forwarded-For` entry
    /// on the public listener).
    pub remote_address: Option<String>,
    /// The parameters the matched endpoint's pattern bound, percent-decoded,
    /// in pattern order.
    pub path_params: Vec<(String, String)>,
}

#[cfg(feature = "flowcatalyst")]
impl Invocation {
    /// An invocation of `address` version 1, called anonymously, with the
    /// invocation id `test-invocation` as its correlation id. For tests;
    /// [`crate::testing::TestHost`] builds on it.
    pub fn new(address: FunctionAddress) -> Self {
        Self {
            invocation_id: "test-invocation".into(),
            address,
            version: 1,
            caller: Caller::Anonymous,
            correlation_id: "test-invocation".into(),
            causation_id: None,
            original_host: None,
            original_path: None,
            remote_address: None,
            path_params: Vec::new(),
        }
    }

    /// The value the endpoint's pattern bound to `{name}`.
    pub fn path_param(&self, name: &str) -> Option<&str> {
        self.path_params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A key the manifest does not declare, or the platform holds no value for.
/// The message is Java's (`config key not declared: KEY`).
#[cfg(feature = "flowcatalyst")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingKey {
    kind: &'static str,
    key: String,
}

#[cfg(feature = "flowcatalyst")]
impl MissingKey {
    /// The key that was asked for.
    pub fn key(&self) -> &str {
        &self.key
    }
}

#[cfg(feature = "flowcatalyst")]
impl fmt::Display for MissingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} key not declared: {}", self.kind, self.key)
    }
}

#[cfg(feature = "flowcatalyst")]
impl std::error::Error for MissingKey {}

/// The function's manifest-declared config (`manifest.config`), Java's
/// `Config`.
#[cfg(feature = "flowcatalyst")]
#[derive(Clone)]
pub struct Config {
    backend: Rc<dyn Backend>,
}

#[cfg(feature = "flowcatalyst")]
impl Config {
    /// The value, when the manifest declares `key` and the platform holds a
    /// value for it.
    pub fn get(&self, key: &str) -> Option<String> {
        self.backend.config(key)
    }

    /// The value, or [`MissingKey`].
    pub fn require(&self, key: &str) -> Result<String, MissingKey> {
        self.get(key).ok_or_else(|| MissingKey {
            kind: "config",
            key: key.to_owned(),
        })
    }
}

/// The function's manifest-declared secrets (`manifest.secrets`), Java's
/// `Secrets`. Kept apart from [`Config`] so the two can never be confused at
/// a call site. Never log a value.
#[cfg(feature = "flowcatalyst")]
#[derive(Clone)]
pub struct Secrets {
    backend: Rc<dyn Backend>,
}

#[cfg(feature = "flowcatalyst")]
impl Secrets {
    /// The value, when the manifest declares `key` and the platform holds a
    /// non-empty value for it.
    pub fn get(&self, key: &str) -> Option<String> {
        self.backend.secret(key)
    }

    /// The value, or [`MissingKey`].
    pub fn require(&self, key: &str) -> Result<String, MissingKey> {
        self.get(key).ok_or_else(|| MissingKey {
            kind: "secret",
            key: key.to_owned(),
        })
    }
}

/// Publishing events on the function's behalf, through the host (Java's
/// `Events`). The function's application must own each event's type.
#[cfg(feature = "flowcatalyst")]
#[derive(Clone)]
pub struct Events {
    backend: Rc<dyn Backend>,
}

#[cfg(feature = "flowcatalyst")]
impl Events {
    /// Publishes `event` and returns once the platform has accepted or
    /// refused it. A `None` correlation or causation id takes the
    /// invocation's ([`Invocation::correlation_id`],
    /// [`Invocation::causation_id`]). The payload must be JSON.
    pub fn emit(&self, event: &OutboundEvent) -> Result<(), EmitError> {
        self.backend.emit(event)
    }
}

/// Why an event was not published (the WIT `emit-error`; Java's
/// `EventEmitException` carries the same codes).
#[cfg(feature = "flowcatalyst")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// The host refused the event before it reached the platform. The code:
    /// [`emit_error::INVALID_EVENT_TYPE_REQUIRED`](crate::emit_error::INVALID_EVENT_TYPE_REQUIRED),
    /// [`emit_error::INVALID_EVENT_DATA_NOT_JSON`](crate::emit_error::INVALID_EVENT_DATA_NOT_JSON)
    /// or [`emit_error::DEDUP_ID_REQUIRED`](crate::emit_error::DEDUP_ID_REQUIRED).
    Invalid(String),
    /// The platform refused the event, with its own code (for example
    /// `EVENT_TYPE_NOT_OWNED` or `DEDUP_ID_DUPLICATE`) and HTTP status.
    Refused {
        /// The platform's error code.
        code: String,
        /// The HTTP status the platform answered with.
        status: u16,
    },
    /// The platform could not be reached. Worth a retry.
    Unavailable,
}

#[cfg(feature = "flowcatalyst")]
impl EmitError {
    /// The code: the host's, the platform's, or
    /// [`emit_error::UNAVAILABLE`](crate::emit_error::UNAVAILABLE).
    pub fn code(&self) -> &str {
        match self {
            EmitError::Invalid(code) => code,
            EmitError::Refused { code, .. } => code,
            EmitError::Unavailable => fc_function_abi::emit_error::UNAVAILABLE,
        }
    }

    /// The status: the platform's, `503` when it could not be reached, and
    /// `400` for the host's own refusal (a client error: resending the same
    /// event can never succeed).
    pub fn status(&self) -> u16 {
        match self {
            EmitError::Invalid(_) => 400,
            EmitError::Refused { status, .. } => *status,
            EmitError::Unavailable => 503,
        }
    }

    /// Whether sending the same event again may succeed: the platform was
    /// unreachable or answered a 5xx.
    pub fn is_retryable(&self) -> bool {
        self.status() >= 500
    }
}

/// `emit refused: <code> (<status>)`, as Java's `EventEmitException`.
#[cfg(feature = "flowcatalyst")]
impl fmt::Display for EmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "emit refused: {} ({})", self.code(), self.status())
    }
}

#[cfg(feature = "flowcatalyst")]
impl std::error::Error for EmitError {}

#[cfg(feature = "flowcatalyst")]
impl From<EmitError> for EventEmitError {
    fn from(error: EmitError) -> Self {
        EventEmitError::new(error.code(), error.status())
    }
}

/// A log line's level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// `TRACE`, `DEBUG`, `INFO`, `WARN` or `ERROR`.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// The function's logger. With the `flowcatalyst` feature a line goes to
/// the host's `log` interface; without it, to standard output (TRACE to
/// INFO) or standard error (WARN, ERROR) as `LEVEL message`.
#[derive(Clone)]
pub struct Logger {
    backend: Rc<dyn Backend>,
}

impl Logger {
    pub fn log(&self, level: Level, message: impl fmt::Display) {
        self.backend.log(level, &message.to_string());
    }

    pub fn trace(&self, message: impl fmt::Display) {
        self.log(Level::Trace, message);
    }

    pub fn debug(&self, message: impl fmt::Display) {
        self.log(Level::Debug, message);
    }

    pub fn info(&self, message: impl fmt::Display) {
        self.log(Level::Info, message);
    }

    pub fn warn(&self, message: impl fmt::Display) {
        self.log(Level::Warn, message);
    }

    pub fn error(&self, message: impl fmt::Display) {
        self.log(Level::Error, message);
    }
}

impl fmt::Debug for Logger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Logger").finish_non_exhaustive()
    }
}

#[cfg(all(test, feature = "flowcatalyst"))]
mod tests {
    use super::*;

    #[test]
    fn emit_errors_carry_javas_codes_and_statuses() {
        let invalid = EmitError::Invalid("DEDUP_ID_REQUIRED".into());
        assert_eq!(
            (invalid.code(), invalid.status(), invalid.is_retryable()),
            ("DEDUP_ID_REQUIRED", 400, false)
        );
        let refused = EmitError::Refused {
            code: "EVENT_TYPE_NOT_OWNED".into(),
            status: 403,
        };
        assert_eq!(
            refused.to_string(),
            "emit refused: EVENT_TYPE_NOT_OWNED (403)"
        );
        assert!(!refused.is_retryable());
        assert!(EmitError::Refused {
            code: "X".into(),
            status: 502
        }
        .is_retryable());
        assert_eq!(
            EventEmitError::from(EmitError::Unavailable),
            EventEmitError::unavailable()
        );
        assert!(EmitError::Unavailable.is_retryable());
    }

    #[test]
    fn missing_keys_read_as_java_words_them() {
        let m = MissingKey {
            kind: "config",
            key: "GREETING".into(),
        };
        assert_eq!(m.to_string(), "config key not declared: GREETING");
        assert_eq!(m.key(), "GREETING");
    }
}

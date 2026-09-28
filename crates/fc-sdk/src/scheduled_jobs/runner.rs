//! `ScheduledJobRunner` — consumer-side dispatch for platform-fired
//! scheduled-job webhooks.
//!
//! Mirrors `@flowcatalyst/sdk`'s TypeScript `ScheduledJobRunner`. Mount
//! [`ScheduledJobRunner::process`] on whatever HTTP framework you use
//! (Axum, Actix, Rocket, raw hyper) at the URL you set as `target_url`
//! on the job definition.
//!
//! Two outputs from [`process`](ScheduledJobRunner::process):
//!   1. The HTTP response shape (always 202 Accepted on a recognised envelope).
//!   2. A background future, started but not awaited — the actual handler
//!      execution + completion callback. The platform expects 202 within the
//!      dispatcher's `http_timeout` (default 10s); your handler should run
//!      asynchronously, not block the HTTP response.
//!
//! The runner enforces concurrency via an injected [`LockProvider`]. For
//! `concurrent: false` jobs (the platform doesn't enforce this — see
//! CLAUDE.md), the lock key defaults to `scheduled-job:{jobCode}`;
//! contended fires return immediately without invoking the handler.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::client::scheduled_jobs::{
    CompletionStatus, InstanceCompleteRequest, InstanceLogRequest, LogLevel,
};
use crate::client::{ClientError, FlowCatalystClient};
use crate::lock::LockHandle;
use crate::lock::LockProvider;
use std::error;

const DEFAULT_LOCK_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_RESULT_BYTES: usize = 10_000;

// ── Envelope ──────────────────────────────────────────────────────────────

/// Envelope POSTed by the platform's scheduler dispatcher. Mirrors
/// `ScheduledJobEnvelope` in the TypeScript SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJobEnvelope {
    pub job_id: String,
    pub job_code: String,
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<String>,
    pub fired_at: String,
    pub trigger_kind: TriggerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    pub tracks_completion: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TriggerKind {
    Cron,
    Manual,
}

// ── Handler context ──────────────────────────────────────────────────────

/// Context passed to user handlers. Wraps the envelope and a best-effort
/// `log` callback that posts a log line back to the platform.
#[derive(Clone)]
pub struct HandlerContext {
    pub envelope: ScheduledJobEnvelope,
    inner: Arc<HandlerContextInner>,
}

struct HandlerContextInner {
    client: FlowCatalystClient,
    instance_id: String,
    job_code: String,
    on_error: Option<OnErrorHook>,
}

impl HandlerContext {
    /// Append a structured log entry to this instance. Best-effort — errors
    /// are forwarded to the runner's `on_error` hook (if configured) and
    /// then swallowed; logging failures never fail the handler.
    pub async fn log(&self, message: impl Into<String>, opts: LogOptions) {
        let req = InstanceLogRequest {
            message: message.into(),
            level: opts.level,
            metadata: opts.metadata,
        };
        if let Err(e) = self
            .inner
            .client
            .scheduled_jobs()
            .log_for_instance(&self.inner.instance_id, &req)
            .await
        {
            if let Some(hook) = &self.inner.on_error {
                hook(
                    RunnerError::CallbackFailed(e),
                    &ScheduledJobEnvelope {
                        job_id: self.envelope.job_id.clone(),
                        job_code: self.inner.job_code.clone(),
                        instance_id: self.inner.instance_id.clone(),
                        scheduled_for: self.envelope.scheduled_for.clone(),
                        fired_at: self.envelope.fired_at.clone(),
                        trigger_kind: self.envelope.trigger_kind,
                        correlation_id: self.envelope.correlation_id.clone(),
                        payload: self.envelope.payload.clone(),
                        tracks_completion: self.envelope.tracks_completion,
                        timeout_seconds: self.envelope.timeout_seconds,
                    },
                );
            }
        }
    }
}

/// Optional fields for [`HandlerContext::log`]. Defaults: `INFO`, no metadata.
#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    pub level: LogLevel,
    pub metadata: Option<serde_json::Value>,
}

// ── Runner ──────────────────────────────────────────────────────────────

/// User handler. Returns `Ok(value)` on success (the value is stored as
/// `completion_result`); `Err` on failure (the error message is stored).
pub type HandlerFuture =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, HandlerError>> + Send>>;

/// Boxed handler. Closures `Fn(HandlerContext) -> HandlerFuture` are accepted;
/// see [`ScheduledJobRunner::handler`].
pub type BoxedHandler = Arc<dyn Fn(HandlerContext) -> HandlerFuture + Send + Sync + 'static>;

/// Error wrapper returned from handlers. Use [`HandlerError::msg`] for
/// ad-hoc string errors, or `?` to bubble any error that implements
/// `Into<HandlerError>`.
#[derive(Debug)]
pub struct HandlerError {
    pub message: String,
}

impl HandlerError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self { message: s.into() }
    }
}

impl<E: error::Error> From<E> for HandlerError {
    fn from(e: E) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

/// Errors surfaced via the `on_error` hook (post-handler-completion failures
/// — log/complete callbacks failing, lock release failing, etc.).
#[derive(Debug)]
pub enum RunnerError {
    CallbackFailed(ClientError),
    LockReleaseFailed(String),
    HandlerPanicked(String),
}

/// Hook fired on every uncaught handler / callback / release error. The
/// hook runs synchronously inside the background task; if you need async
/// reporting, dispatch to your own channel from here.
pub type OnErrorHook = Arc<dyn Fn(RunnerError, &ScheduledJobEnvelope) + Send + Sync + 'static>;

/// Result of [`ScheduledJobRunner::process`]. The runner is HTTP-framework-
/// agnostic; map this to your framework's response type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    /// Recognised envelope; handler dispatched in the background.
    /// Respond with HTTP 202 Accepted + body `{"ok": true}`.
    Accepted,
    /// Envelope was malformed. Respond with HTTP 400 Bad Request + the
    /// included message in `{"error": ...}`.
    BadRequest(String),
    /// Envelope is well-formed but no handler is registered for `job_code`.
    /// Respond with HTTP 404 Not Found + the included message in
    /// `{"error": ...}`.
    NotFound(String),
}

/// Options for [`ScheduledJobRunner::builder`].
pub struct RunnerOptions {
    pub lock_provider: Arc<dyn LockProvider>,
    pub lock_key: Box<dyn Fn(&ScheduledJobEnvelope) -> String + Send + Sync + 'static>,
    pub enforce_lock: bool,
    pub lock_ttl: Duration,
    pub on_error: Option<OnErrorHook>,
}

impl RunnerOptions {
    pub fn default_with_lock(lock_provider: Arc<dyn LockProvider>) -> Self {
        Self {
            lock_provider,
            lock_key: Box::new(|e| format!("scheduled-job:{}", e.job_code)),
            enforce_lock: true,
            lock_ttl: DEFAULT_LOCK_TTL,
            on_error: None,
        }
    }
}

/// Registry-of-handlers + envelope dispatcher.
///
/// Build via [`ScheduledJobRunner::builder`]; register handlers via
/// [`ScheduledJobRunner::handler`]; then expose the runner from your HTTP
/// route so each platform → SDK firing calls [`ScheduledJobRunner::process`].
pub struct ScheduledJobRunner {
    handlers: HashMap<String, BoxedHandler>,
    client: FlowCatalystClient,
    options: RunnerOptions,
}

impl ScheduledJobRunner {
    /// Start a builder. Pass the API client used for log/complete callbacks
    /// and a [`LockProvider`] (use `Arc::new(NoOpLockProvider)` if you don't
    /// need concurrency control).
    pub fn builder(
        client: FlowCatalystClient,
        lock_provider: Arc<dyn LockProvider>,
    ) -> ScheduledJobRunnerBuilder {
        ScheduledJobRunnerBuilder {
            client,
            handlers: HashMap::new(),
            options: RunnerOptions::default_with_lock(lock_provider),
        }
    }

    /// Process an inbound platform → SDK firing. Validates the envelope,
    /// kicks off the handler in the background, and returns a [`RunResult`]
    /// immediately. The actual handler execution + completion callback
    /// continues asynchronously via `tokio::spawn`.
    pub fn process(&self, envelope: serde_json::Value) -> RunResult {
        let env = match serde_json::from_value::<ScheduledJobEnvelope>(envelope) {
            Ok(e) => e,
            Err(err) => return RunResult::BadRequest(err.to_string()),
        };
        if !matches!(env.trigger_kind, TriggerKind::Cron | TriggerKind::Manual) {
            return RunResult::BadRequest("Invalid triggerKind".into());
        }
        let Some(handler) = self.handlers.get(&env.job_code).cloned() else {
            return RunResult::NotFound(format!(
                "No handler registered for code '{}'",
                env.job_code
            ));
        };

        let client = self.client.clone();
        let lock_provider = Arc::clone(&self.options.lock_provider);
        let enforce_lock = self.options.enforce_lock;
        let lock_ttl = self.options.lock_ttl;
        let lock_key = (self.options.lock_key)(&env);
        let on_error = self.options.on_error.clone();

        tokio::spawn(async move {
            run_in_background(
                env,
                handler,
                client,
                lock_provider,
                enforce_lock,
                lock_ttl,
                lock_key,
                on_error,
            )
            .await;
        });

        RunResult::Accepted
    }

    /// Listed registered codes (diagnostics only).
    pub fn registered_codes(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }
}

pub struct ScheduledJobRunnerBuilder {
    client: FlowCatalystClient,
    handlers: HashMap<String, BoxedHandler>,
    options: RunnerOptions,
}

impl ScheduledJobRunnerBuilder {
    /// Register a handler keyed by the job's `code`.
    ///
    /// Example:
    ///
    /// ```ignore
    /// .handler("daily-rollup", |ctx| Box::pin(async move {
    ///     ctx.log("starting", LogOptions::default()).await;
    ///     // … work …
    ///     Ok(serde_json::json!({ "processed": 42 }))
    /// }))
    /// ```
    pub fn handler<F>(mut self, code: impl Into<String>, handler: F) -> Self
    where
        F: Fn(HandlerContext) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers.insert(code.into(), Arc::new(handler));
        self
    }

    pub fn lock_key<F>(mut self, f: F) -> Self
    where
        F: Fn(&ScheduledJobEnvelope) -> String + Send + Sync + 'static,
    {
        self.options.lock_key = Box::new(f);
        self
    }

    pub fn enforce_lock(mut self, enforce: bool) -> Self {
        self.options.enforce_lock = enforce;
        self
    }

    pub fn lock_ttl(mut self, ttl: Duration) -> Self {
        self.options.lock_ttl = ttl;
        self
    }

    pub fn on_error<F>(mut self, hook: F) -> Self
    where
        F: Fn(RunnerError, &ScheduledJobEnvelope) + Send + Sync + 'static,
    {
        self.options.on_error = Some(Arc::new(hook));
        self
    }

    pub fn build(self) -> ScheduledJobRunner {
        ScheduledJobRunner {
            handlers: self.handlers,
            client: self.client,
            options: self.options,
        }
    }
}

// ── Background dispatch ──────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_in_background(
    envelope: ScheduledJobEnvelope,
    handler: BoxedHandler,
    client: FlowCatalystClient,
    lock_provider: Arc<dyn LockProvider>,
    enforce_lock: bool,
    lock_ttl: Duration,
    lock_key: String,
    on_error: Option<OnErrorHook>,
) {
    let mut lock_handle: Option<LockHandle> = None;

    if enforce_lock {
        match lock_provider.acquire(&lock_key, lock_ttl).await {
            Ok(Some(h)) => lock_handle = Some(h),
            Ok(None) => {
                // Lock contention — skip this firing. If the job tracks
                // completion, mark as FAILURE with a clear reason; otherwise
                // just no-op (the instance will sit DELIVERED forever).
                if envelope.tracks_completion {
                    let req = InstanceCompleteRequest {
                        status: CompletionStatus::Failure,
                        result: Some(serde_json::json!({
                            "skipped": true,
                            "reason": "lock-held",
                        })),
                    };
                    if let Err(e) = client
                        .scheduled_jobs()
                        .complete_instance(&envelope.instance_id, &req)
                        .await
                    {
                        invoke_on_error(&on_error, RunnerError::CallbackFailed(e), &envelope);
                    }
                }
                return;
            }
            Err(e) => {
                invoke_on_error(
                    &on_error,
                    RunnerError::LockReleaseFailed(format!("acquire failed: {e}")),
                    &envelope,
                );
                return;
            }
        }
    }

    let ctx = HandlerContext {
        envelope: envelope.clone(),
        inner: Arc::new(HandlerContextInner {
            client: client.clone(),
            instance_id: envelope.instance_id.clone(),
            job_code: envelope.job_code.clone(),
            on_error: on_error.clone(),
        }),
    };

    let outcome = handler(ctx).await;

    if envelope.tracks_completion {
        let req = match &outcome {
            Ok(value) => InstanceCompleteRequest {
                status: CompletionStatus::Success,
                result: Some(sanitise_result(value.clone())),
            },
            Err(err) => InstanceCompleteRequest {
                status: CompletionStatus::Failure,
                result: Some(serde_json::json!({ "error": err.message })),
            },
        };
        if let Err(e) = client
            .scheduled_jobs()
            .complete_instance(&envelope.instance_id, &req)
            .await
        {
            invoke_on_error(&on_error, RunnerError::CallbackFailed(e), &envelope);
        }
    }

    if let Err(err) = &outcome {
        invoke_on_error(
            &on_error,
            RunnerError::HandlerPanicked(err.message.clone()),
            &envelope,
        );
    }

    if let Some(handle) = lock_handle {
        handle.release().await;
    }
}

fn invoke_on_error(hook: &Option<OnErrorHook>, err: RunnerError, envelope: &ScheduledJobEnvelope) {
    if let Some(h) = hook {
        h(err, envelope);
    }
}

fn sanitise_result(v: serde_json::Value) -> serde_json::Value {
    match serde_json::to_string(&v) {
        Ok(json) if json.len() > MAX_RESULT_BYTES => serde_json::json!({
            "truncated": true,
            "preview": json.chars().take(MAX_RESULT_BYTES).collect::<String>(),
        }),
        _ => v,
    }
}

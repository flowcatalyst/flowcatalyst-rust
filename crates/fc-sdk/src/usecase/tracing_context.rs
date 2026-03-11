//! Tracing Context
//!
//! Thread-local propagation of correlation and causation IDs through
//! async/background jobs. Enables distributed tracing across service boundaries.

use std::cell::RefCell;

thread_local! {
    static TRACING_CONTEXT: RefCell<Option<TracingContext>> = RefCell::new(None);
}

/// Distributed tracing context for correlation and causation tracking.
///
/// Stored in thread-local storage and automatically picked up by
/// [`ExecutionContext::create()`](super::ExecutionContext::create) when available.
///
/// # Examples
///
/// ```
/// use fc_sdk::usecase::TracingContext;
///
/// // Run code with tracing context
/// TracingContext::run_with_context("corr-123", None, || {
///     // ExecutionContext::create() will use these IDs automatically
/// });
/// ```
#[derive(Debug, Clone)]
pub struct TracingContext {
    correlation_id: String,
    causation_id: Option<String>,
}

impl TracingContext {
    pub fn new(correlation_id: String, causation_id: Option<String>) -> Self {
        Self {
            correlation_id,
            causation_id,
        }
    }

    pub fn correlation_id(&self) -> String {
        self.correlation_id.clone()
    }

    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    /// Get the current thread-local tracing context (if set).
    pub fn current() -> Option<TracingContext> {
        TRACING_CONTEXT.with(|ctx| ctx.borrow().clone())
    }

    /// Get the current context or panic.
    pub fn require_current() -> TracingContext {
        Self::current().expect("TracingContext not set — use run_with_context or set_current")
    }

    /// Set the thread-local tracing context.
    pub fn set_current(ctx: TracingContext) {
        TRACING_CONTEXT.with(|c| {
            *c.borrow_mut() = Some(ctx);
        });
    }

    /// Clear the thread-local tracing context.
    pub fn clear_current() {
        TRACING_CONTEXT.with(|c| {
            *c.borrow_mut() = None;
        });
    }

    /// Run a closure with a tracing context set, restoring the previous context afterward.
    pub fn run_with_context<F, R>(
        correlation_id: impl Into<String>,
        causation_id: Option<String>,
        f: F,
    ) -> R
    where
        F: FnOnce() -> R,
    {
        let previous = Self::current();
        Self::set_current(TracingContext::new(correlation_id.into(), causation_id));
        let result = f();
        match previous {
            Some(prev) => Self::set_current(prev),
            None => Self::clear_current(),
        }
        result
    }

    /// Run an async closure with a tracing context set.
    pub async fn run_with_context_async<F, Fut, R>(
        correlation_id: impl Into<String>,
        causation_id: Option<String>,
        f: F,
    ) -> R
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = R>,
    {
        let previous = Self::current();
        Self::set_current(TracingContext::new(correlation_id.into(), causation_id));
        let result = f().await;
        match previous {
            Some(prev) => Self::set_current(prev),
            None => Self::clear_current(),
        }
        result
    }

    /// Run a closure with context derived from a parent event.
    pub fn run_from_event<F, R>(
        correlation_id: impl Into<String>,
        causing_event_id: impl Into<String>,
        f: F,
    ) -> R
    where
        F: FnOnce() -> R,
    {
        Self::run_with_context(correlation_id, Some(causing_event_id.into()), f)
    }
}

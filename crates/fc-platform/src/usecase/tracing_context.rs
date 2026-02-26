//! Tracing Context
//!
//! Context for distributed tracing. Holds correlation and causation IDs
//! for the current request or background job.

use std::cell::RefCell;
use crate::shared::tsid::TsidGenerator;

thread_local! {
    /// Thread-local storage for tracing context.
    static TRACING_CONTEXT: RefCell<Option<TracingContext>> = RefCell::new(None);
}

/// Context for distributed tracing.
///
/// This context holds correlation and causation IDs for the current request.
/// It can be populated from:
/// - HTTP headers (X-Correlation-ID, X-Causation-ID)
/// - Background job context
/// - Event-driven context when processing domain events
///
/// # Standard HTTP Headers
///
/// - `X-Correlation-ID` - Traces a request across services
/// - `X-Causation-ID` - References the event that caused this request
#[derive(Debug, Clone)]
pub struct TracingContext {
    correlation_id: Option<String>,
    causation_id: Option<String>,
}

impl TracingContext {
    /// Create a new empty tracing context.
    pub fn new() -> Self {
        Self {
            correlation_id: None,
            causation_id: None,
        }
    }

    /// Create a tracing context with specific IDs.
    pub fn with_ids(correlation_id: Option<String>, causation_id: Option<String>) -> Self {
        Self {
            correlation_id,
            causation_id,
        }
    }

    /// Get the correlation ID, generating one if not set.
    pub fn correlation_id(&self) -> String {
        self.correlation_id
            .clone()
            .unwrap_or_else(|| format!("trace-{}", TsidGenerator::generate()))
    }

    /// Get the correlation ID if set, without generating.
    pub fn correlation_id_opt(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// Set the correlation ID.
    pub fn set_correlation_id(&mut self, id: String) {
        self.correlation_id = Some(id);
    }

    /// Get the causation ID (may be None for fresh requests).
    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    /// Set the causation ID.
    pub fn set_causation_id(&mut self, id: String) {
        self.causation_id = Some(id);
    }

    /// Check if a correlation ID has been explicitly set.
    pub fn has_correlation_id(&self) -> bool {
        self.correlation_id.is_some()
    }

    /// Check if a causation ID has been set.
    pub fn has_causation_id(&self) -> bool {
        self.causation_id.is_some()
    }

    // ========================================================================
    // Static methods for thread-local access
    // ========================================================================

    /// Get the current tracing context from thread-local storage.
    /// Returns None if not in a tracing context.
    pub fn current() -> Option<TracingContext> {
        TRACING_CONTEXT.with(|ctx| ctx.borrow().clone())
    }

    /// Get the current tracing context, panicking if not available.
    ///
    /// Use this in background jobs to enforce that tracing context was set up.
    pub fn require_current() -> TracingContext {
        Self::current().expect(
            "No TracingContext available. Background jobs must be executed via \
             TracingContext::run_with_context()",
        )
    }

    /// Run a task with a specific tracing context on the current thread.
    ///
    /// This is useful for background jobs running on async tasks
    /// that need to propagate tracing context.
    ///
    /// # Example
    ///
    /// ```ignore
    /// TracingContext::run_with_context("corr-123", Some("cause-456"), || {
    ///     // Background job code here
    ///     // TracingContext::current() will return the context
    /// });
    /// ```
    pub fn run_with_context<F, R>(
        correlation_id: impl Into<String>,
        causation_id: Option<String>,
        f: F,
    ) -> R
    where
        F: FnOnce() -> R,
    {
        let ctx = TracingContext {
            correlation_id: Some(correlation_id.into()),
            causation_id,
        };

        TRACING_CONTEXT.with(|cell| {
            let prev = cell.borrow().clone();
            *cell.borrow_mut() = Some(ctx);

            let result = f();

            *cell.borrow_mut() = prev;
            result
        })
    }

    /// Run a task with a specific tracing context (async version).
    ///
    /// Note: For truly async contexts, consider using task-local storage
    /// or passing the context explicitly through the async chain.
    pub async fn run_with_context_async<F, Fut, R>(
        correlation_id: impl Into<String>,
        causation_id: Option<String>,
        f: F,
    ) -> R
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = R>,
    {
        let ctx = TracingContext {
            correlation_id: Some(correlation_id.into()),
            causation_id,
        };

        TRACING_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx);
        });

        let result = f().await;

        TRACING_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });

        result
    }

    /// Run a task continuing from a parent event's context.
    ///
    /// The parent event's correlation_id is preserved, and its event_id
    /// becomes the causation_id.
    pub fn run_from_event<F, R>(
        parent_correlation_id: &str,
        parent_event_id: &str,
        f: F,
    ) -> R
    where
        F: FnOnce() -> R,
    {
        Self::run_with_context(
            parent_correlation_id.to_string(),
            Some(parent_event_id.to_string()),
            f,
        )
    }

    /// Set the current thread-local context.
    pub fn set_current(ctx: TracingContext) {
        TRACING_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx);
        });
    }

    /// Clear the current thread-local context.
    pub fn clear_current() {
        TRACING_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }
}

impl Default for TracingContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_context_new() {
        let ctx = TracingContext::new();
        assert!(!ctx.has_correlation_id());
        assert!(!ctx.has_causation_id());
    }

    #[test]
    fn test_correlation_id_generation() {
        let ctx = TracingContext::new();
        let id = ctx.correlation_id();
        assert!(id.starts_with("trace-"));
    }

    #[test]
    fn test_run_with_context() {
        let result = TracingContext::run_with_context("test-corr-123", Some("test-cause-456".to_string()), || {
            let ctx = TracingContext::current().expect("should have context");
            assert_eq!(ctx.correlation_id(), "test-corr-123");
            assert_eq!(ctx.causation_id(), Some("test-cause-456"));
            42
        });
        assert_eq!(result, 42);

        // Context should be cleared after run
        assert!(TracingContext::current().is_none());
    }

    #[test]
    fn test_nested_contexts() {
        TracingContext::run_with_context("outer", None, || {
            let outer = TracingContext::current().unwrap();
            assert_eq!(outer.correlation_id(), "outer");

            TracingContext::run_with_context("inner", Some("cause".to_string()), || {
                let inner = TracingContext::current().unwrap();
                assert_eq!(inner.correlation_id(), "inner");
                assert_eq!(inner.causation_id(), Some("cause"));
            });

            // Should restore outer context
            let restored = TracingContext::current().unwrap();
            assert_eq!(restored.correlation_id(), "outer");
        });
    }
}

//! Tracing Context
//!
//! Task-local propagation of correlation and causation IDs through async
//! work. Enables distributed tracing across service boundaries.
//!
//! The context is a [`tokio::task_local!`], so it follows the *task* across
//! `.await` points and worker threads, and is scoped: it is set only for the
//! duration of the future (or closure) passed to [`TracingContext::scope`] /
//! [`TracingContext::sync_scope`] and is restored when that returns. Spawned
//! tasks do not inherit it; wrap the spawned future in `scope` if they need it.

use std::future::Future;

tokio::task_local! {
    static TRACING_CONTEXT: TracingContext;
}

/// Distributed tracing context for correlation and causation tracking.
///
/// Picked up automatically by
/// [`ExecutionContext::create()`](super::ExecutionContext::create) while a
/// scope is active.
///
/// # Examples
///
/// ```
/// use fc_sdk::usecase::{ExecutionContext, TracingContext};
///
/// # tokio_test::block_on(async {
/// let ctx = TracingContext::new("corr-123", None);
/// let exec = ctx
///     .scope(async {
///         // Any `.await` in here still sees the same context.
///         ExecutionContext::create("user-1")
///     })
///     .await;
/// assert_eq!(exec.correlation_id, "corr-123");
/// # });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracingContext {
    correlation_id: String,
    causation_id: Option<String>,
}

impl TracingContext {
    pub fn new(correlation_id: impl Into<String>, causation_id: Option<String>) -> Self {
        Self {
            correlation_id: correlation_id.into(),
            causation_id,
        }
    }

    /// Context for work caused by a parent event: keeps the correlation id
    /// and records the parent event as the cause.
    pub fn for_event(
        correlation_id: impl Into<String>,
        causing_event_id: impl Into<String>,
    ) -> Self {
        Self::new(correlation_id, Some(causing_event_id.into()))
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    /// The context of the enclosing [`scope`](Self::scope) /
    /// [`sync_scope`](Self::sync_scope), if any.
    pub fn current() -> Option<TracingContext> {
        TRACING_CONTEXT.try_with(Clone::clone).ok()
    }

    /// Run `fut` with this context set. The context stays attached to the
    /// future across `.await` points, whichever thread polls it.
    pub async fn scope<F: Future>(self, fut: F) -> F::Output {
        TRACING_CONTEXT.scope(self, fut).await
    }

    /// Run a synchronous closure with this context set.
    pub fn sync_scope<R>(self, f: impl FnOnce() -> R) -> R {
        TRACING_CONTEXT.sync_scope(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_accessors() {
        let tc = TracingContext::new("corr-1", Some("cause-1".into()));
        assert_eq!(tc.correlation_id(), "corr-1");
        assert_eq!(tc.causation_id(), Some("cause-1"));
    }

    #[test]
    fn new_without_causation() {
        let tc = TracingContext::new("corr-2", None);
        assert_eq!(tc.correlation_id(), "corr-2");
        assert!(tc.causation_id().is_none());
    }

    #[test]
    fn for_event_sets_causation() {
        let tc = TracingContext::for_event("corr-evt", "evt_parent_id");
        assert_eq!(tc.correlation_id(), "corr-evt");
        assert_eq!(tc.causation_id(), Some("evt_parent_id"));
    }

    #[test]
    fn current_is_none_outside_a_scope() {
        assert!(TracingContext::current().is_none());
    }

    #[test]
    fn sync_scope_sets_and_restores() {
        let result =
            TracingContext::new("inner-corr", Some("inner-cause".into())).sync_scope(|| {
                let tc = TracingContext::current().unwrap();
                assert_eq!(tc.correlation_id(), "inner-corr");
                assert_eq!(tc.causation_id(), Some("inner-cause"));
                42
            });
        assert_eq!(result, 42);
        assert!(TracingContext::current().is_none());
    }

    #[test]
    fn nested_sync_scopes_restore_the_outer_context() {
        TracingContext::new("outer", None).sync_scope(|| {
            assert_eq!(TracingContext::current().unwrap().correlation_id(), "outer");

            TracingContext::new("inner", Some("cause".into())).sync_scope(|| {
                let tc = TracingContext::current().unwrap();
                assert_eq!(tc.correlation_id(), "inner");
                assert_eq!(tc.causation_id(), Some("cause"));
            });

            assert_eq!(TracingContext::current().unwrap().correlation_id(), "outer");
        });
        assert!(TracingContext::current().is_none());
    }

    #[tokio::test]
    async fn scope_sets_and_restores() {
        let result = TracingContext::new("async-corr", None)
            .scope(async {
                tokio::task::yield_now().await;
                TracingContext::current()
                    .unwrap()
                    .correlation_id()
                    .to_string()
            })
            .await;
        assert_eq!(result, "async-corr");
        assert!(TracingContext::current().is_none());
    }

    /// The bug the thread-local version had: two tasks interleaving on the
    /// same worker thread overwrote each other's context. With a task-local
    /// each task keeps its own across every `.await`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_tasks_keep_their_own_context() {
        let tasks: Vec<_> = (0..16)
            .map(|i| {
                tokio::spawn(
                    TracingContext::new(format!("corr-{i}"), None).scope(async move {
                        for _ in 0..10 {
                            tokio::task::yield_now().await;
                            assert_eq!(
                                TracingContext::current().unwrap().correlation_id(),
                                format!("corr-{i}")
                            );
                        }
                    }),
                )
            })
            .collect();
        for t in tasks {
            t.await.unwrap();
        }
    }

    #[tokio::test]
    async fn spawned_tasks_do_not_inherit_the_context() {
        TracingContext::new("parent", None)
            .scope(async {
                let seen = tokio::spawn(async { TracingContext::current() })
                    .await
                    .unwrap();
                assert!(seen.is_none());
            })
            .await;
    }
}

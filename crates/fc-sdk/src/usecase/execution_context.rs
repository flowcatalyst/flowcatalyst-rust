//! Execution Context
//!
//! Context for a use case execution. Carries tracing IDs and principal
//! information through the execution of a use case.

use super::domain_event::DomainEvent;
use super::tracing_context::TracingContext;
use crate::tsid;
use chrono::{DateTime, Utc};

/// Context for a use case execution.
///
/// Carries tracing IDs and principal information through the execution
/// of a use case. This context is used to populate domain event metadata.
///
/// # Examples
///
/// ```
/// use fc_sdk::usecase::ExecutionContext;
///
/// // Fresh request (generates new IDs)
/// let ctx = ExecutionContext::create("user-123");
///
/// // With specific correlation ID from upstream
/// let ctx = ExecutionContext::with_correlation("user-123", "trace-from-gateway");
///
/// // Child context within same execution
/// let child = ctx.with_causation("evt-456");
/// ```
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Unique ID for this execution (generated)
    pub execution_id: String,
    /// ID for distributed tracing (usually from original request)
    pub correlation_id: String,
    /// ID of the parent event that caused this execution (if any)
    pub causation_id: Option<String>,
    /// ID of the principal performing the action
    pub principal_id: String,
    /// When the execution was initiated
    pub initiated_at: DateTime<Utc>,
}

impl ExecutionContext {
    /// Create a new execution context for a fresh request.
    ///
    /// Picks up the task-local [`TracingContext`] when called inside
    /// [`TracingContext::scope`] / [`TracingContext::sync_scope`].
    pub fn create(principal_id: impl Into<String>) -> Self {
        if let Some(tracing_ctx) = TracingContext::current() {
            return Self::from_tracing_context(&tracing_ctx, principal_id);
        }

        let exec_id = format!("exec-{}", tsid::generate_untyped());
        Self {
            execution_id: exec_id.clone(),
            correlation_id: exec_id,
            causation_id: None,
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create an execution context from a [`TracingContext`].
    ///
    /// Preferred when running within an HTTP request where TracingContext
    /// has been populated from headers.
    pub fn from_tracing_context(
        tracing_context: &TracingContext,
        principal_id: impl Into<String>,
    ) -> Self {
        let exec_id = format!("exec-{}", tsid::generate_untyped());
        Self {
            execution_id: exec_id,
            correlation_id: tracing_context.correlation_id().to_string(),
            causation_id: tracing_context.causation_id().map(|s| s.to_string()),
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create a new execution context with a specific correlation ID.
    pub fn with_correlation(
        principal_id: impl Into<String>,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            execution_id: format!("exec-{}", tsid::generate_untyped()),
            correlation_id: correlation_id.into(),
            causation_id: None,
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create a new execution context from a parent event.
    ///
    /// The parent event's ID becomes the causation_id, and the
    /// correlation_id is preserved.
    pub fn from_parent_event<E: DomainEvent>(parent: &E, principal_id: impl Into<String>) -> Self {
        Self {
            execution_id: format!("exec-{}", tsid::generate_untyped()),
            correlation_id: parent.metadata().correlation_id.clone(),
            causation_id: Some(parent.metadata().event_id.clone()),
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create an execution context from an [`AuthContext`](crate::auth::AuthContext).
    ///
    /// Bridges the auth layer to the use case layer by extracting the
    /// principal_id from the validated token claims.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use fc_sdk::usecase::ExecutionContext;
    ///
    /// let ctx = ExecutionContext::from_auth(&auth_context);
    /// let result = use_case.execute(command, ctx).await;
    /// ```
    #[cfg(feature = "auth")]
    pub fn from_auth(auth: &crate::auth::AuthContext) -> Self {
        Self::create(auth.principal_id())
    }

    /// Create an execution context from [`AccessTokenClaims`](crate::auth::AccessTokenClaims).
    ///
    /// Use this when you have the raw claims without a full AuthContext.
    #[cfg(feature = "auth")]
    pub fn from_claims(claims: &crate::auth::AccessTokenClaims) -> Self {
        Self::create(claims.principal_id())
    }

    /// Create a child context within the same execution.
    pub fn with_causation(&self, causing_event_id: impl Into<String>) -> Self {
        Self {
            execution_id: self.execution_id.clone(),
            correlation_id: self.correlation_id.clone(),
            causation_id: Some(causing_event_id.into()),
            principal_id: self.principal_id.clone(),
            initiated_at: Utc::now(),
        }
    }

    /// Create a new context with a different principal.
    pub fn with_principal(&self, principal_id: impl Into<String>) -> Self {
        Self {
            execution_id: self.execution_id.clone(),
            correlation_id: self.correlation_id.clone(),
            causation_id: self.causation_id.clone(),
            principal_id: principal_id.into(),
            initiated_at: self.initiated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_generates_ids_and_sets_principal() {
        let ctx = ExecutionContext::create("prn_user1");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert!(!ctx.execution_id.is_empty());
        // Without tracing context, correlation_id equals execution_id
        assert_eq!(ctx.correlation_id, ctx.execution_id);
        assert!(ctx.causation_id.is_none());
        assert_eq!(ctx.principal_id, "prn_user1");
        assert!(ctx.initiated_at <= chrono::Utc::now());
    }

    #[test]
    fn create_picks_up_tracing_context() {
        TracingContext::new("trace-corr-123", Some("cause-evt-1".into())).sync_scope(|| {
            let ctx = ExecutionContext::create("prn_test");

            assert!(ctx.execution_id.starts_with("exec-"));
            assert_eq!(ctx.correlation_id, "trace-corr-123");
            assert_eq!(ctx.causation_id.as_deref(), Some("cause-evt-1"));
            assert_eq!(ctx.principal_id, "prn_test");
        });
    }

    #[tokio::test]
    async fn create_picks_up_tracing_context_across_awaits() {
        let ctx = TracingContext::new("async-corr", Some("evt_cause".into()))
            .scope(async {
                tokio::task::yield_now().await;
                ExecutionContext::create("prn_async")
            })
            .await;
        assert_eq!(ctx.correlation_id, "async-corr");
        assert_eq!(ctx.causation_id.as_deref(), Some("evt_cause"));

        // Outside the scope a fresh correlation id is generated again.
        let fresh = ExecutionContext::create("prn_async");
        assert_eq!(fresh.correlation_id, fresh.execution_id);
    }

    #[test]
    fn create_picks_up_tracing_context_without_causation() {
        TracingContext::new("corr-only", None).sync_scope(|| {
            let ctx = ExecutionContext::create("prn");
            assert_eq!(ctx.correlation_id, "corr-only");
            assert!(ctx.causation_id.is_none());
        });
    }

    #[test]
    fn with_correlation_sets_specific_correlation_id() {
        let ctx = ExecutionContext::with_correlation("prn_gw", "gateway-trace-456");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert_eq!(ctx.correlation_id, "gateway-trace-456");
        assert!(ctx.causation_id.is_none());
        assert_eq!(ctx.principal_id, "prn_gw");
    }

    #[test]
    fn from_tracing_context() {
        let tc = TracingContext::new("tc-corr", Some("tc-cause".into()));
        let ctx = ExecutionContext::from_tracing_context(&tc, "prn_tc");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert_eq!(ctx.correlation_id, "tc-corr");
        assert_eq!(ctx.causation_id.as_deref(), Some("tc-cause"));
        assert_eq!(ctx.principal_id, "prn_tc");
    }

    #[test]
    fn with_causation_creates_child_context() {
        let parent = ExecutionContext::with_correlation("prn_parent", "corr-parent");
        let child = parent.with_causation("evt_parent_id");

        // execution_id and correlation_id are preserved
        assert_eq!(child.execution_id, parent.execution_id);
        assert_eq!(child.correlation_id, parent.correlation_id);
        // causation_id is set to the parent event
        assert_eq!(child.causation_id.as_deref(), Some("evt_parent_id"));
        // principal is preserved
        assert_eq!(child.principal_id, parent.principal_id);
    }

    #[test]
    fn with_principal_creates_context_with_different_principal() {
        let ctx = ExecutionContext::with_correlation("prn_original", "corr-1");
        let switched = ctx.with_principal("prn_delegate");

        assert_eq!(switched.execution_id, ctx.execution_id);
        assert_eq!(switched.correlation_id, ctx.correlation_id);
        assert_eq!(switched.causation_id, ctx.causation_id);
        assert_eq!(switched.principal_id, "prn_delegate");
        // initiated_at is preserved
        assert_eq!(switched.initiated_at, ctx.initiated_at);
    }

    #[test]
    fn from_parent_event_chains_correlation_and_causation() {
        use crate::usecase::EventMetadata;
        use serde::Serialize;

        #[derive(Debug, Clone, Serialize)]
        struct FakeEvent {
            pub metadata: EventMetadata,
        }
        crate::impl_domain_event!(FakeEvent);

        let parent_meta = EventMetadata {
            event_id: "evt_parent_123".into(),
            event_type: "shop:order:created".into(),
            spec_version: "1.0".into(),
            source: "shop".into(),
            subject: "orders.order.1".into(),
            time: chrono::Utc::now(),
            execution_id: "exec-parent".into(),
            correlation_id: "corr-chain".into(),
            causation_id: None,
            principal_id: "prn_orig".into(),
            message_group: "orders:order:1".into(),
        };
        let parent_event = FakeEvent {
            metadata: parent_meta,
        };

        let ctx = ExecutionContext::from_parent_event(&parent_event, "prn_handler");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert_ne!(ctx.execution_id, "exec-parent"); // new execution
        assert_eq!(ctx.correlation_id, "corr-chain"); // preserved
        assert_eq!(ctx.causation_id.as_deref(), Some("evt_parent_123")); // parent's ID
        assert_eq!(ctx.principal_id, "prn_handler");
    }

    #[test]
    fn unique_execution_ids() {
        let a = ExecutionContext::create("prn");
        let b = ExecutionContext::create("prn");
        assert_ne!(a.execution_id, b.execution_id);
    }

    #[test]
    fn clone_is_independent() {
        let ctx = ExecutionContext::with_correlation("prn", "corr");
        let cloned = ctx.clone();
        assert_eq!(ctx.execution_id, cloned.execution_id);
        assert_eq!(ctx.principal_id, cloned.principal_id);
    }
}

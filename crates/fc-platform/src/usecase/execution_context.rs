//! Execution Context
//!
//! Context for a use case execution. Carries tracing IDs and principal
//! information through the execution of a use case.

use chrono::{DateTime, Utc};
use crate::shared::tsid::TsidGenerator;
use super::tracing_context::TracingContext;
use super::domain_event::DomainEvent;

/// Context for a use case execution.
///
/// Carries tracing IDs and principal information through the execution
/// of a use case. This context is used to populate domain event metadata.
///
/// The execution context enables:
/// - Distributed tracing via correlation_id
/// - Causal chain tracking via causation_id
/// - Process/saga tracking via execution_id
/// - Audit trail via principal_id
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
    /// The execution_id and correlation_id are both set to a new TSID.
    /// Use this for API-initiated requests when no tracing context is available.
    ///
    /// **Prefer using [`from_tracing_context`] when a TracingContext
    /// is available**, as it will preserve correlation/causation from HTTP headers
    /// or background job context.
    pub fn create(principal_id: impl Into<String>) -> Self {
        // Check if there's a thread-local TracingContext
        if let Some(tracing_ctx) = TracingContext::current() {
            return Self::from_tracing_context(&tracing_ctx, principal_id);
        }

        let exec_id = format!("exec-{}", TsidGenerator::generate());
        Self {
            execution_id: exec_id.clone(),
            correlation_id: exec_id, // correlation starts as execution ID
            causation_id: None,      // no causation for fresh requests
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create an execution context from a TracingContext.
    ///
    /// This is the preferred method when running within an HTTP request
    /// where TracingContext has been populated from headers.
    pub fn from_tracing_context(
        tracing_context: &TracingContext,
        principal_id: impl Into<String>,
    ) -> Self {
        let exec_id = format!("exec-{}", TsidGenerator::generate());
        Self {
            execution_id: exec_id,
            correlation_id: tracing_context.correlation_id(),
            causation_id: tracing_context.causation_id().map(|s| s.to_string()),
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create a new execution context with a specific correlation ID.
    ///
    /// Use this when you have an existing correlation ID from an
    /// upstream system or request header.
    pub fn with_correlation(
        principal_id: impl Into<String>,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            execution_id: format!("exec-{}", TsidGenerator::generate()),
            correlation_id: correlation_id.into(),
            causation_id: None,
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create a new execution context from a parent event.
    ///
    /// Use this when reacting to an event and creating a new execution.
    /// The parent event's ID becomes the causation_id, and the correlation_id
    /// is preserved.
    pub fn from_parent_event<E: DomainEvent>(
        parent: &E,
        principal_id: impl Into<String>,
    ) -> Self {
        Self {
            execution_id: format!("exec-{}", TsidGenerator::generate()),
            correlation_id: parent.correlation_id().to_string(),
            causation_id: Some(parent.event_id().to_string()),
            principal_id: principal_id.into(),
            initiated_at: Utc::now(),
        }
    }

    /// Create a child context within the same execution.
    ///
    /// Use this when an execution needs to perform sub-operations
    /// that should share the same execution_id but have different causation.
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
    ///
    /// Use this for system-initiated operations that run on behalf of
    /// a different principal than the original request.
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
    fn test_create_context() {
        let ctx = ExecutionContext::create("user-123");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert_eq!(ctx.principal_id, "user-123");
        // correlation_id starts as execution_id for fresh requests
        assert_eq!(ctx.correlation_id, ctx.execution_id);
        assert!(ctx.causation_id.is_none());
    }

    #[test]
    fn test_with_correlation() {
        let ctx = ExecutionContext::with_correlation("user-123", "corr-456");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert_eq!(ctx.correlation_id, "corr-456");
        assert_eq!(ctx.principal_id, "user-123");
    }

    #[test]
    fn test_with_causation() {
        let ctx = ExecutionContext::create("user-123");
        let child = ctx.with_causation("evt-789");

        assert_eq!(child.execution_id, ctx.execution_id);
        assert_eq!(child.correlation_id, ctx.correlation_id);
        assert_eq!(child.causation_id, Some("evt-789".to_string()));
    }

    #[test]
    fn test_with_principal() {
        let ctx = ExecutionContext::create("user-123");
        let new_ctx = ctx.with_principal("system");

        assert_eq!(new_ctx.execution_id, ctx.execution_id);
        assert_eq!(new_ctx.principal_id, "system");
    }

    #[test]
    fn test_from_tracing_context() {
        TracingContext::run_with_context("trace-123", Some("cause-456".to_string()), || {
            let ctx = ExecutionContext::create("user-789");
            assert_eq!(ctx.correlation_id, "trace-123");
            assert_eq!(ctx.causation_id, Some("cause-456".to_string()));
            assert_eq!(ctx.principal_id, "user-789");
        });
    }
}

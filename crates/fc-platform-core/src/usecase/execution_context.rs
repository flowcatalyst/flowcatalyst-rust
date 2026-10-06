//! Execution Context
//!
//! Context for a use case execution. Carries tracing IDs, the principal
//! recorded on events and audit rows, and the [`Caller`] whose authority the
//! use case checks in `authorize`.

use super::caller::Caller;
use super::domain_event::DomainEvent;
use crate::shared::authorization_service::{ApplicationScope, AuthContext};
use crate::shared::tsid;
use chrono::{DateTime, Utc};

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
/// - Resource-level authorization via [`caller`](Self::caller)
///
/// There is no way to build one without a caller: an authenticated request
/// uses [`from_auth`](Self::from_auth), a platform-internal path
/// [`system`](Self::system). The caller field is private, so a struct
/// literal can't skip it.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Unique ID for this execution (generated)
    pub execution_id: String,
    /// ID for distributed tracing (usually from original request)
    pub correlation_id: String,
    /// ID of the parent event that caused this execution (if any)
    pub causation_id: Option<String>,
    /// ID of the principal recorded as performing the action (event
    /// metadata, audit rows)
    pub principal_id: String,
    /// When the execution was initiated
    pub initiated_at: DateTime<Utc>,
    /// Whose authority the use case checks.
    caller: Caller,
}

impl ExecutionContext {
    /// A fresh execution: execution_id and correlation_id are both a new
    /// TSID, with no causation.
    fn fresh(principal_id: String, caller: Caller) -> Self {
        let exec_id = format!("exec-{}", tsid::generate_untyped());
        Self {
            execution_id: exec_id.clone(),
            correlation_id: exec_id, // correlation starts as execution ID
            causation_id: None,      // no causation for fresh requests
            principal_id,
            initiated_at: Utc::now(),
            caller,
        }
    }

    /// Create an execution context from an authenticated request context:
    /// the principal recorded is the caller's, and the caller carries its
    /// full authority (tier, clients, permissions, credential).
    pub fn from_auth(auth: &AuthContext) -> Self {
        Self::fresh(auth.principal_id.clone(), Caller::from_auth(auth))
    }

    /// A platform-internal execution ([`Caller::system`]): startup sync,
    /// bootstrap, login and password-reset outcomes recorded before any
    /// principal is authenticated. `principal_id` is only what the event and
    /// audit rows record as the actor (`"system"`, or the principal the
    /// platform acts for); it grants nothing.
    pub fn system(principal_id: impl Into<String>) -> Self {
        Self::fresh(principal_id.into(), Caller::system())
    }

    /// A context for a [`Caller`] already built (a principal with the
    /// application scope its handler resolved): the principal recorded is the
    /// caller's, `"system"` for the system caller.
    pub fn from_caller(caller: Caller) -> Self {
        let principal_id = caller.principal_id().unwrap_or("system").to_string();
        Self::fresh(principal_id, caller)
    }

    /// Attach the caller's resolved application scope (see
    /// [`Caller::with_application_scope`]).
    pub fn with_application_scope(mut self, scope: ApplicationScope) -> Self {
        self.caller = self.caller.with_application_scope(scope);
        self
    }

    /// Continue an existing trace: the same context under an upstream
    /// correlation ID.
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = correlation_id.into();
        self
    }

    /// A new execution reacting to `parent`, for the same caller: the
    /// parent event's ID becomes the causation_id, and the correlation_id is
    /// preserved.
    pub fn from_parent_event<E: DomainEvent>(parent: &E, ctx: &ExecutionContext) -> Self {
        Self {
            execution_id: format!("exec-{}", tsid::generate_untyped()),
            correlation_id: parent.metadata().correlation_id.clone(),
            causation_id: Some(parent.metadata().event_id.clone()),
            principal_id: ctx.principal_id.clone(),
            initiated_at: Utc::now(),
            caller: ctx.caller.clone(),
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
            caller: self.caller.clone(),
        }
    }

    /// Whose authority the use case checks.
    pub fn caller(&self) -> &Caller {
        &self.caller
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal_kind::PrincipalType;
    use crate::principal_kind::UserScope;
    use crate::shared::authorization_service::Credential;
    use crate::shared::id::ApplicationId;

    #[test]
    fn test_system_context() {
        let ctx = ExecutionContext::system("user-123");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert_eq!(ctx.principal_id, "user-123");
        // correlation_id starts as execution_id for fresh requests
        assert_eq!(ctx.correlation_id, ctx.execution_id);
        assert!(ctx.causation_id.is_none());
        assert!(ctx.caller().is_system());
    }

    #[test]
    fn from_auth_records_the_principal_and_carries_its_authority() {
        let auth = AuthContext {
            principal_id: "prn_1".into(),
            principal_type: PrincipalType::User,
            scope: UserScope::Client,
            email: None,
            name: "Test".into(),
            accessible_clients: vec!["clt_a".into()],
            permissions: Default::default(),
            roles: vec![],
            credential: Credential::BearerToken,
        };
        let ctx = ExecutionContext::from_auth(&auth);
        assert_eq!(ctx.principal_id, "prn_1");
        assert_eq!(ctx.caller().principal_id(), Some("prn_1"));
        assert!(!ctx.caller().is_system());
        assert!(ctx.caller().application_scope().is_none());
        let scoped = ctx.with_application_scope(ApplicationScope::All);
        assert!(scoped
            .caller()
            .allows_application(&ApplicationId::parse("app_1").unwrap()));
    }

    #[test]
    fn test_with_correlation_id() {
        let ctx = ExecutionContext::system("user-123").with_correlation_id("corr-456");

        assert!(ctx.execution_id.starts_with("exec-"));
        assert_eq!(ctx.correlation_id, "corr-456");
        assert_eq!(ctx.principal_id, "user-123");
    }

    #[test]
    fn test_with_causation() {
        let ctx = ExecutionContext::system("user-123");
        let child = ctx.with_causation("evt-789");

        assert_eq!(child.execution_id, ctx.execution_id);
        assert_eq!(child.correlation_id, ctx.correlation_id);
        assert_eq!(child.causation_id, Some("evt-789".to_string()));
        assert!(child.caller().is_system());
    }
}

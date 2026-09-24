//! Principal Domain Events

use crate::impl_domain_event;
use crate::principal::entity::UserScope;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

/// Event emitted when a new user is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub email: String,
    pub email_domain: String,
    pub name: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub is_anchor_user: bool,
}

impl_domain_event!(UserCreated);

impl UserCreated {
    const EVENT_TYPE: &'static str = "platform:iam:user:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    /// Build the event for a newly created user.
    ///
    /// Derives `email_domain` from `email`, and `is_anchor_user` from `scope`.
    pub fn new(
        ctx: &ExecutionContext,
        principal_id: &str,
        email: &str,
        name: &str,
        scope: UserScope,
        client_id: Option<&str>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
            email_domain: extract_email_domain(email),
            name: name.to_string(),
            scope: format!("{:?}", scope).to_uppercase(),
            client_id: client_id.map(String::from),
            is_anchor_user: scope == UserScope::Anchor,
        }
    }
}

/// Extract the domain part from an email address.
fn extract_email_domain(email: &str) -> String {
    email
        .split('@')
        .nth(1)
        .map(|s| s.to_lowercase())
        .unwrap_or_default()
}

/// Event emitted when a user is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl_domain_event!(UserUpdated);

impl UserUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:user:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        principal_id: &str,
        name: Option<&str>,
        email: Option<&str>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            name: name.map(String::from),
            email: email.map(String::from),
        }
    }
}

/// Event emitted when a user is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserActivated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
}

impl_domain_event!(UserActivated);

impl UserActivated {
    const EVENT_TYPE: &'static str = "platform:iam:user:activated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
        }
    }
}

/// Event emitted when a user is deactivated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeactivated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl_domain_event!(UserDeactivated);

impl UserDeactivated {
    const EVENT_TYPE: &'static str = "platform:iam:user:deactivated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, reason: Option<&str>) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            reason: reason.map(String::from),
        }
    }
}

/// Event emitted when a user is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
}

impl_domain_event!(UserDeleted);

impl UserDeleted {
    const EVENT_TYPE: &'static str = "platform:iam:user:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
        }
    }
}

/// Event emitted when roles are assigned to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolesAssigned {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub roles: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl_domain_event!(RolesAssigned);

impl RolesAssigned {
    const EVENT_TYPE: &'static str = "platform:iam:user:roles-assigned";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        principal_id: &str,
        roles: Vec<String>,
        added: Vec<String>,
        removed: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            roles,
            added,
            removed,
        }
    }
}

/// Event emitted when client access is granted to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessGranted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub client_id: String,
}

impl_domain_event!(ClientAccessGranted);

impl ClientAccessGranted {
    const EVENT_TYPE: &'static str = "platform:iam:user:client-access-granted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, client_id: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            client_id: client_id.to_string(),
        }
    }
}

/// Event emitted when client access is revoked from a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessRevoked {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub client_id: String,
}

impl_domain_event!(ClientAccessRevoked);

impl ClientAccessRevoked {
    const EVENT_TYPE: &'static str = "platform:iam:user:client-access-revoked";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, client_id: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            client_id: client_id.to_string(),
        }
    }
}

/// FlowCatalyst claims embedded in UserLoggedIn event data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowcatalystClaims {
    pub email: String,
    #[serde(rename = "type")]
    pub principal_type: String,
    pub roles: Vec<String>,
    pub clients: Vec<String>,
    pub applications: Vec<String>,
}

/// Federated (external IDP) claims embedded in UserLoggedIn event data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FederatedClaims {
    pub access_token: serde_json::Value,
    pub id_token: serde_json::Value,
}

/// Event emitted when a user logs in via OIDC.
/// Matches the TypeScript `UserLoggedInData` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLoggedIn {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub user_id: String,
    pub email: String,
    pub login_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_provider_code: Option<String>,
    pub flowcatalyst_claims: FlowcatalystClaims,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub federated_claims: Option<FederatedClaims>,
}

impl_domain_event!(UserLoggedIn);

impl UserLoggedIn {
    const EVENT_TYPE: &'static str = "platform:iam:user:logged-in";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        user_id: &str,
        email: &str,
        login_method: &str,
        identity_provider_code: Option<&str>,
        flowcatalyst_claims: FlowcatalystClaims,
        federated_claims: Option<FederatedClaims>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", user_id),
                format!("platform:user:{}", user_id),
            ),
            user_id: user_id.to_string(),
            email: email.to_string(),
            login_method: login_method.to_string(),
            identity_provider_code: identity_provider_code.map(String::from),
            flowcatalyst_claims,
            federated_claims,
        }
    }
}

/// Event emitted when principals are synced from an application SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalsSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deactivated: u32,
    pub synced_emails: Vec<String>,
}

impl_domain_event!(PrincipalsSynced);

impl PrincipalsSynced {
    const EVENT_TYPE: &'static str = "platform:iam:principals:synced";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        application_code: &str,
        created: u32,
        updated: u32,
        deactivated: u32,
        synced_emails: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.application.{}", application_code),
                format!("platform:application:{}", application_code),
            ),
            application_code: application_code.to_string(),
            created,
            updated,
            deactivated,
            synced_emails,
        }
    }
}

/// Event emitted when application access is assigned to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationAccessAssigned {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub user_id: String,
    pub application_ids: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl_domain_event!(ApplicationAccessAssigned);

impl ApplicationAccessAssigned {
    const EVENT_TYPE: &'static str = "platform:iam:user:application-access-assigned";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        user_id: &str,
        application_ids: Vec<String>,
        added: Vec<String>,
        removed: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", user_id),
                format!("platform:user:{}", user_id),
            ),
            user_id: user_id.to_string(),
            application_ids,
            added,
            removed,
        }
    }
}

/// Event emitted when a password reset is requested.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetRequested {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub email: String,
}

impl_domain_event!(PasswordResetRequested);

impl PasswordResetRequested {
    const EVENT_TYPE: &'static str = "platform:iam:user:password-reset-requested";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(principal_id: &str, email: &str) -> Self {
        // Password reset is unauthenticated — attribute it to "system".
        Self {
            metadata: EventMetadata::from_ctx(
                &ExecutionContext::create("system"),
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
        }
    }
}

/// Event emitted when a user's password is reset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetCompleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub email: String,
}

impl_domain_event!(PasswordResetCompleted);

impl PasswordResetCompleted {
    const EVENT_TYPE: &'static str = "platform:iam:user:password-reset-completed";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(principal_id: &str, email: &str) -> Self {
        // Password reset is unauthenticated — attribute it to "system".
        Self {
            metadata: EventMetadata::from_ctx(
                &ExecutionContext::create("system"),
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
        }
    }

    /// Emit a password-reset event attributed to an authenticated caller
    /// (e.g. an admin invoking the reset endpoint). Preserves the caller's
    /// execution/correlation IDs so audit logs and downstream projections can
    /// trace the action back to them.
    pub fn from_ctx(ctx: &ExecutionContext, principal_id: &str, email: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.user.{}", principal_id),
                format!("platform:user:{}", principal_id),
            ),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserCreated::new(
            &ctx,
            "user-1",
            "user@example.com",
            "Test User",
            UserScope::Client,
            Some("client-1"),
        );

        assert_eq!(event.metadata.event_type, "platform:iam:user:created");
        assert_eq!(event.principal_id, "user-1");
        assert_eq!(event.email, "user@example.com");
        assert_eq!(event.email_domain, "example.com");
        assert_eq!(event.scope, "CLIENT");
        assert!(!event.is_anchor_user);
    }

    #[test]
    fn test_user_created_copies_ctx_and_derives_anchor_flag() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserCreated::new(
            &ctx,
            "user-1",
            "john.doe@acme.org",
            "John Doe",
            UserScope::Anchor,
            None,
        );

        assert_eq!(event.email_domain, "acme.org");
        assert_eq!(event.name, "John Doe");
        assert_eq!(event.scope, "ANCHOR");
        assert!(event.is_anchor_user);
        // Verify tracing context was copied
        assert_eq!(event.metadata.execution_id, ctx.execution_id);
        assert_eq!(event.metadata.correlation_id, ctx.correlation_id);
    }

    #[test]
    fn test_email_domain_extraction() {
        assert_eq!(extract_email_domain("user@example.com"), "example.com");
        assert_eq!(
            extract_email_domain("user@SUB.Example.COM"),
            "sub.example.com"
        );
        assert_eq!(extract_email_domain("invalid-email"), "");
        assert_eq!(extract_email_domain(""), "");
    }

    #[test]
    fn test_user_deactivated_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserDeactivated::new(&ctx, "user-1", Some("Policy violation"));

        assert_eq!(event.metadata.event_type, "platform:iam:user:deactivated");
        assert_eq!(event.reason, Some("Policy violation".to_string()));
    }
}

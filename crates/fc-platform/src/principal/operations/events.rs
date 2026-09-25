//! Principal Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/principal/operations/events.go`): type
//! `platform:iam:user:*`, source `platform:iam`, subject
//! `platform.principal.{id}`, group `platform:principal:{id}` (except
//! `logged-in`, which Go puts on `platform.user.{id}` / `platform:user:{id}`),
//! and each payload carries exactly Go's `ToDataJSON` fields.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:iam";

fn metadata(ctx: &ExecutionContext, event_type: &str, principal_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.principal.{}", principal_id),
        format!("platform:principal:{}", principal_id),
    )
}

/// `{principalId, email}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
    pub email: String,
}

impl_domain_event!(UserCreated);

impl UserCreated {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:created";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, email: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
        }
    }
}

/// `{principalId, name}`: the user's name after the change.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
    pub name: String,
}

impl_domain_event!(UserUpdated);

impl UserUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:updated";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{principalId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserActivated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
}

impl_domain_event!(UserActivated);

impl UserActivated {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:activated";

    pub fn new(ctx: &ExecutionContext, principal_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
        }
    }
}

/// `{principalId}`. A deactivation reason is audited with the command, not
/// carried in the event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeactivated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
}

impl_domain_event!(UserDeactivated);

impl UserDeactivated {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:deactivated";

    pub fn new(ctx: &ExecutionContext, principal_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
        }
    }
}

/// `{principalId, email}`: `email` is `""` for a principal without a user
/// identity, as Go.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
    pub email: String,
}

impl_domain_event!(UserDeleted);

impl UserDeleted {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:deleted";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, email: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
        }
    }
}

/// `{principalId, roles, added, removed}`: empty lists are `[]` (Go's
/// `defaultEmpty`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolesAssigned {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
    pub roles: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl_domain_event!(RolesAssigned);

impl RolesAssigned {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:roles-assigned";

    pub fn new(
        ctx: &ExecutionContext,
        principal_id: &str,
        roles: Vec<String>,
        added: Vec<String>,
        removed: Vec<String>,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
            roles,
            added,
            removed,
        }
    }
}

/// `{principalId, clientId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessGranted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
    pub client_id: String,
}

impl_domain_event!(ClientAccessGranted);

impl ClientAccessGranted {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:client-access-granted";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, client_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
            client_id: client_id.to_string(),
        }
    }
}

/// `{principalId, clientId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessRevoked {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
    pub client_id: String,
}

impl_domain_event!(ClientAccessRevoked);

impl ClientAccessRevoked {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:client-access-revoked";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, client_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
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

/// A user logged in via OIDC: `{userId, email, loginMethod,
/// identityProviderCode?, flowcatalystClaims, federatedClaims?}` on subject
/// `platform.user.{userId}` and group `platform:user:{userId}` (Go's
/// `UserLoggedInSubject` / `UserLoggedInMessageGroup`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLoggedIn {
    #[serde(skip)]
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
    pub const EVENT_TYPE: &'static str = "platform:iam:user:logged-in";

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
                SPEC_VERSION,
                SOURCE,
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

/// The rollup of a principal sync:
/// `{applicationCode, created, updated, deactivated, syncedEmails}` on
/// subject `platform.principals.{applicationCode}` and group
/// `platform:principals:{applicationCode}` (bare `platform.principals` /
/// `platform:principals` for the platform-level sync).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalsSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deactivated: u32,
    pub synced_emails: Vec<String>,
}

impl_domain_event!(PrincipalsSynced);

impl PrincipalsSynced {
    pub const EVENT_TYPE: &'static str = "platform:iam:principals:synced";

    /// Metadata for this event, raised inside `ctx` for a sync of
    /// `application_code`.
    pub fn metadata_for(ctx: &ExecutionContext, application_code: &str) -> EventMetadata {
        if application_code.is_empty() {
            return Self::metadata_for_platform(ctx);
        }
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            SPEC_VERSION,
            SOURCE,
            format!("platform.principals.{}", application_code),
            format!("platform:principals:{}", application_code),
        )
    }

    /// Metadata for the platform-level sync (no application): Go's subject
    /// `platform.principals` and group `platform:principals`
    /// (principal/operations/events.go:444-464).
    pub fn metadata_for_platform(ctx: &ExecutionContext) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            SPEC_VERSION,
            SOURCE,
            "platform.principals".to_string(),
            "platform:principals".to_string(),
        )
    }
}

/// `{userId, applicationIds, added, removed}`: empty lists are `[]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationAccessAssigned {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub user_id: String,
    pub application_ids: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl_domain_event!(ApplicationAccessAssigned);

impl ApplicationAccessAssigned {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:application-access-assigned";

    pub fn new(
        ctx: &ExecutionContext,
        user_id: &str,
        application_ids: Vec<String>,
        added: Vec<String>,
        removed: Vec<String>,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, user_id),
            user_id: user_id.to_string(),
            application_ids,
            added,
            removed,
        }
    }
}

/// A password reset was requested (self-service). Go emits no event for a
/// request; this one is Rust's own, on the user family's subject:
/// `{principalId, email}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetRequested {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
    pub email: String,
}

impl_domain_event!(PasswordResetRequested);

impl PasswordResetRequested {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:password-reset-requested";

    pub fn new(principal_id: &str, email: &str) -> Self {
        // Password reset is unauthenticated — attribute it to "system".
        Self {
            metadata: metadata(
                &ExecutionContext::create("system"),
                Self::EVENT_TYPE,
                principal_id,
            ),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
        }
    }
}

/// A user's password was reset: `{principalId}` (Go's `UserPasswordReset`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetCompleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub principal_id: String,
}

impl_domain_event!(PasswordResetCompleted);

impl PasswordResetCompleted {
    pub const EVENT_TYPE: &'static str = "platform:iam:user:password-reset-completed";

    /// The event attributed to the caller in `ctx` (an admin, or the
    /// principal completing a self-service reset), keeping its trace ids.
    pub fn from_ctx(ctx: &ExecutionContext, principal_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, principal_id),
            principal_id: principal_id.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserCreated::new(&ctx, "user-1", "user@example.com");

        assert_eq!(event.metadata.event_type, "platform:iam:user:created");
        assert_eq!(event.metadata.subject, "platform.principal.user-1");
        assert_eq!(event.metadata.message_group, "platform:principal:user-1");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"principalId": "user-1", "email": "user@example.com"})
        );
        // Tracing context was copied
        assert_eq!(event.metadata.execution_id, ctx.execution_id);
        assert_eq!(event.metadata.correlation_id, ctx.correlation_id);
    }

    #[test]
    fn test_user_deactivated_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserDeactivated::new(&ctx, "user-1");

        assert_eq!(event.metadata.event_type, "platform:iam:user:deactivated");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"principalId": "user-1"})
        );
    }

    #[test]
    fn logged_in_keeps_the_user_subject() {
        let ctx = ExecutionContext::create("usr_1");
        let event = UserLoggedIn::new(
            &ctx,
            "usr_1",
            "a@b.c",
            "OIDC",
            None,
            FlowcatalystClaims {
                email: "a@b.c".into(),
                principal_type: "USER".into(),
                roles: vec![],
                clients: vec![],
                applications: vec![],
            },
            None,
        );
        assert_eq!(event.metadata.subject, "platform.user.usr_1");
        assert_eq!(event.metadata.message_group, "platform:user:usr_1");
    }

    #[test]
    fn principals_synced_subject_is_per_application() {
        let ctx = ExecutionContext::create("usr_1");
        let meta = PrincipalsSynced::metadata_for(&ctx, "hr");
        assert_eq!(meta.subject, "platform.principals.hr");
        assert_eq!(meta.message_group, "platform:principals:hr");
        let meta = PrincipalsSynced::metadata_for(&ctx, "");
        assert_eq!(meta.subject, "platform.principals");
    }
}

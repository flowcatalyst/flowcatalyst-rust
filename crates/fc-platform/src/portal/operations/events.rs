//! Portal identity plane domain events (Go `portalidentity/events.go`): the
//! event types, source, subjects and message groups are Go's.

use serde::Serialize;

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;

pub const IDENTITY_ENSURED: &str = "platform:portal:identity:ensured";
pub const IDENTITY_STATUS_SET: &str = "platform:portal:identity:status-set";
pub const IDENTITY_DELETED: &str = "platform:portal:identity:deleted";
pub const IDENTITY_APP_GRANTED: &str = "platform:portal:identity:app-granted";
pub const IDENTITY_APP_REVOKED: &str = "platform:portal:identity:app-revoked";
pub const APP_CREATED: &str = "platform:portal:app:created";
pub const APP_UPDATED: &str = "platform:portal:app:updated";
pub const APP_DELETED: &str = "platform:portal:app:deleted";

/// Go `EventSource`.
pub const EVENT_SOURCE: &str = "platform:portal";
const SPEC_VERSION: &str = "1.0";

fn identity_metadata(ctx: &ExecutionContext, event_type: &str, identity_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        EVENT_SOURCE,
        format!("platform.portal-identity.{identity_id}"),
        format!("platform:portal-identity:{identity_id}"),
    )
}

fn app_metadata(ctx: &ExecutionContext, event_type: &str, app_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        EVENT_SOURCE,
        format!("platform.portal-app.{app_id}"),
        format!("platform:portal-app:{app_id}"),
    )
}

/// An identity was created, or re-ensured (reactivated), in a client's
/// portal context.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityEnsured {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub identity_id: String,
    pub client_id: String,
    pub email: String,
    pub created: bool,
    pub source: String,
    /// The portal app granted by this ensure, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_app_code: Option<String>,
}

impl_domain_event!(IdentityEnsured);

impl IdentityEnsured {
    pub fn new(
        ctx: &ExecutionContext,
        identity_id: &str,
        client_id: &str,
        email: &str,
        created: bool,
        source: &str,
    ) -> Self {
        Self {
            metadata: identity_metadata(ctx, IDENTITY_ENSURED, identity_id),
            identity_id: identity_id.to_string(),
            client_id: client_id.to_string(),
            email: email.to_string(),
            created,
            source: source.to_string(),
            portal_app_id: None,
            portal_app_code: None,
        }
    }
}

/// An identity was suspended or reactivated.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityStatusSet {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub identity_id: String,
    pub client_id: String,
    pub status: String,
}

impl_domain_event!(IdentityStatusSet);

impl IdentityStatusSet {
    pub fn new(ctx: &ExecutionContext, identity_id: &str, client_id: &str, status: &str) -> Self {
        Self {
            metadata: identity_metadata(ctx, IDENTITY_STATUS_SET, identity_id),
            identity_id: identity_id.to_string(),
            client_id: client_id.to_string(),
            status: status.to_string(),
        }
    }
}

/// An identity was removed (offboarding).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub identity_id: String,
    pub client_id: String,
    pub email: String,
}

impl_domain_event!(IdentityDeleted);

impl IdentityDeleted {
    pub fn new(ctx: &ExecutionContext, identity_id: &str, client_id: &str, email: &str) -> Self {
        Self {
            metadata: identity_metadata(ctx, IDENTITY_DELETED, identity_id),
            identity_id: identity_id.to_string(),
            client_id: client_id.to_string(),
            email: email.to_string(),
        }
    }
}

/// An identity gained access to a portal app.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityAppGranted {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub identity_id: String,
    pub client_id: String,
    pub portal_app_id: String,
    pub portal_app_code: String,
    pub source: String,
}

impl_domain_event!(IdentityAppGranted);

impl IdentityAppGranted {
    pub fn new(
        ctx: &ExecutionContext,
        identity_id: &str,
        client_id: &str,
        app_id: &str,
        app_code: &str,
        source: &str,
    ) -> Self {
        Self {
            metadata: identity_metadata(ctx, IDENTITY_APP_GRANTED, identity_id),
            identity_id: identity_id.to_string(),
            client_id: client_id.to_string(),
            portal_app_id: app_id.to_string(),
            portal_app_code: app_code.to_string(),
            source: source.to_string(),
        }
    }
}

/// An identity lost access to a portal app.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityAppRevoked {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub identity_id: String,
    pub client_id: String,
    pub portal_app_id: String,
    pub portal_app_code: String,
}

impl_domain_event!(IdentityAppRevoked);

impl IdentityAppRevoked {
    pub fn new(
        ctx: &ExecutionContext,
        identity_id: &str,
        client_id: &str,
        app_id: &str,
        app_code: &str,
    ) -> Self {
        Self {
            metadata: identity_metadata(ctx, IDENTITY_APP_REVOKED, identity_id),
            identity_id: identity_id.to_string(),
            client_id: client_id.to_string(),
            portal_app_id: app_id.to_string(),
            portal_app_code: app_code.to_string(),
        }
    }
}

/// The portal-app lifecycle events (created / updated / deleted) share one
/// shape; the metadata's event type selects which (Go `AppChanged`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalAppChanged {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub portal_app_id: String,
    pub client_id: String,
    pub code: String,
    pub name: String,
    /// What went with a deleted app: the public ids of the OAuth clients
    /// that fronted it. Not part of the event body.
    #[serde(skip)]
    pub deleted_oauth_client_ids: Vec<String>,
}

impl_domain_event!(PortalAppChanged);

impl PortalAppChanged {
    pub fn new(ctx: &ExecutionContext, event_type: &str, app: &crate::portal::PortalApp) -> Self {
        Self {
            metadata: app_metadata(ctx, event_type, &app.id),
            portal_app_id: app.id.clone(),
            client_id: app.client_id.clone(),
            code: app.code.clone(),
            name: app.name.clone(),
            deleted_oauth_client_ids: Vec::new(),
        }
    }
}

/// The last grant of a bulk assignment, carrying the assignment's totals
/// (not part of the event body).
#[derive(Debug, Clone, Serialize)]
pub struct AssignedToApp {
    #[serde(flatten)]
    pub granted: IdentityAppGranted,
    #[serde(skip)]
    pub app_code: String,
    #[serde(skip)]
    pub identity_ids: Vec<String>,
}

impl_domain_event!(AssignedToApp => granted);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_events_use_gos_envelope() {
        let ctx = ExecutionContext::create("prn_1");
        let e = IdentityEnsured::new(&ctx, "ptu_1", "clt_1", "a@b.c", true, "INVITE");
        assert_eq!(e.metadata.event_type, "platform:portal:identity:ensured");
        assert_eq!(e.metadata.source, "platform:portal");
        assert_eq!(e.metadata.subject, "platform.portal-identity.ptu_1");
        assert_eq!(e.metadata.message_group, "platform:portal-identity:ptu_1");
        let body = serde_json::to_value(&e).unwrap();
        assert_eq!(body["identityId"], "ptu_1");
        assert!(body.get("portalAppId").is_none());
    }

    #[test]
    fn app_events_use_gos_envelope() {
        let ctx = ExecutionContext::create("prn_1");
        let app = crate::portal::PortalApp::new("clt_1", "Suppliers", "Suppliers Portal");
        let e = PortalAppChanged::new(&ctx, APP_CREATED, &app);
        assert_eq!(
            e.metadata.subject,
            format!("platform.portal-app.{}", app.id)
        );
        assert_eq!(
            e.metadata.message_group,
            format!("platform:portal-app:{}", app.id)
        );
        let body = serde_json::to_value(&e).unwrap();
        assert_eq!(body["code"], "suppliers");
        assert!(body.get("deletedOauthClientIds").is_none());
    }
}

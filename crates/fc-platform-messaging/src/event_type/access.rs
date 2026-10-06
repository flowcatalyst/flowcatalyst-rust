//! Resource-level access to an existing event type, shared by the BFF
//! handlers and the server-rendered `fc-web` UI. Permission checks
//! (`checks::can_read_event_types` / `can_write_event_types`) come first;
//! these decide whether the caller may touch *this* event type.

use std::collections::HashMap;
use std::sync::Arc;

use crate::event_type::entity::EventType;
use fc_platform_core::directory::{ApplicationAccess, ApplicationDirectory};
use fc_platform_core::shared::authorization_service::{ApplicationScope, AuthContext};
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::id::ClientId;

/// A client-owned event type is visible to callers with access to that
/// client; an anchor-level one to everyone who passed the permission check.
pub fn ensure_visible(auth: &AuthContext, event_type: &EventType) -> Result<(), PlatformError> {
    match event_type.client_id.as_ref() {
        Some(cid) if !auth.can_access_client(cid) => {
            Err(PlatformError::forbidden("No access to this event type"))
        }
        _ => Ok(()),
    }
}

/// As [`ensure_visible`], and an anchor-level event type may only be
/// changed by an anchor user. `verb` names the change in the refusal
/// ("modify", "delete", "archive").
pub fn ensure_modifiable(
    auth: &AuthContext,
    event_type: &EventType,
    verb: &str,
) -> Result<(), PlatformError> {
    ensure_visible(auth, event_type)?;
    if event_type.client_id.is_none() && !auth.is_anchor() {
        return Err(PlatformError::forbidden(format!(
            "Only anchor users can {verb} anchor-level event types"
        )));
    }
    Ok(())
}

/// Whether the caller may create an event type owned by `client_id`: a
/// client-owned one needs access to that client, an anchor-level one
/// (`None`) an anchor user.
pub fn ensure_can_create(
    auth: &AuthContext,
    client_id: Option<&ClientId>,
) -> Result<(), PlatformError> {
    match client_id {
        Some(cid) if !auth.can_access_client(cid) => Err(PlatformError::forbidden(format!(
            "No access to client: {}",
            cid
        ))),
        Some(_) => Ok(()),
        None if !auth.is_anchor() => Err(PlatformError::forbidden(
            "Only anchor users can create anchor-level event types",
        )),
        None => Ok(()),
    }
}

/// Confines an application service account to the event types of the
/// applications it is bound to (Go `appAccess` / `requireEventTypeAppAccess`).
/// An event type belongs to the application its code names; that application
/// is looked up and checked against the caller's application scope. An
/// unknown application is not accessible. Applied only to callers admitted by
/// the application-service permissions alone; holders of the messaging
/// permissions skip it.
#[derive(Clone)]
pub struct ApplicationConfinement {
    applications: Arc<dyn ApplicationDirectory>,
    access: Arc<dyn ApplicationAccess>,
}

impl ApplicationConfinement {
    pub fn new(
        applications: Arc<dyn ApplicationDirectory>,
        access: Arc<dyn ApplicationAccess>,
    ) -> Self {
        Self {
            applications,
            access,
        }
    }

    /// The caller's application scope.
    pub async fn scope(&self, auth: &AuthContext) -> Result<ApplicationScope, PlatformError> {
        self.access.scope_for(&auth.principal_id).await
    }

    /// Whether `scope` covers the application with code `code`. Answers are
    /// remembered in `seen` for the life of one request, so a list resolves
    /// each application once.
    pub async fn reaches(
        &self,
        scope: &ApplicationScope,
        code: &str,
        seen: &mut HashMap<String, bool>,
    ) -> Result<bool, PlatformError> {
        if let Some(known) = seen.get(code) {
            return Ok(*known);
        }
        let reaches = match self.applications.find_by_code(code).await? {
            Some(application) => scope.allows(&application.id),
            None => false,
        };
        seen.insert(code.to_string(), reaches);
        Ok(reaches)
    }

    /// Refuses a confined caller an event type that belongs to an
    /// application it is not bound to.
    pub async fn require(
        &self,
        auth: &AuthContext,
        event_type: &EventType,
    ) -> Result<(), PlatformError> {
        let scope = self.scope(auth).await?;
        if self
            .reaches(&scope, &event_type.application, &mut HashMap::new())
            .await?
        {
            Ok(())
        } else {
            Err(PlatformError::forbidden("No access to this event type"))
        }
    }
}

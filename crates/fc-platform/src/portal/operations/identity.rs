//! Portal identity use cases (Go `portalidentity/operations.go`): ensure,
//! grant/revoke an app, set status, delete.
//!
//! Authorization is the handler's: the portal-users API gates on the
//! client-delegable portal permissions for the command's client, and the JIT
//! path runs as the system actor inside an authenticated SSO callback (Go's
//! operations are `Authorize: Public` for the same reason).

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::{
    IdentityAppGranted, IdentityAppRevoked, IdentityDeleted, IdentityEnsured, IdentityStatusSet,
};
use super::{load_client_app, not_found};
use crate::portal::entity::{IdentitySource, IdentityStatus, PortalApp, PortalIdentity};
use crate::portal::repository::{PortalAppRepository, PortalIdentityRepository};
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::ClientRepository;

// ── Ensure ────────────────────────────────────────────────────────────────

/// Go `portalidentity.EnsureCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureCommand {
    pub client_id: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source: String,
    /// Grants the identity this portal app (which must belong to the client
    /// and be active). The grant's source is `source`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_app_id: Option<String>,
}

impl crate::usecase::AuditMasked for EnsureCommand {}

/// Idempotently create — or reactivate — the (client, email) identity, grant
/// the named app, and emit `IdentityEnsured`. Re-ensuring keeps the id,
/// source, created_at, any password and any grants; a DISABLED identity
/// converges back to ACTIVE.
pub struct EnsurePortalIdentityUseCase<U: UnitOfWork> {
    identities: Arc<PortalIdentityRepository>,
    apps: Arc<PortalAppRepository>,
    clients: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> EnsurePortalIdentityUseCase<U> {
    pub fn new(
        identities: Arc<PortalIdentityRepository>,
        apps: Arc<PortalAppRepository>,
        clients: Arc<ClientRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            identities,
            apps,
            clients,
            unit_of_work,
        }
    }

    async fn prepare(
        &self,
        cmd: &EnsureCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PortalIdentity, IdentityEnsured), UseCaseError> {
        if self.clients.find_by_id(&cmd.client_id).await?.is_none() {
            return Err(not_found("Client", &cmd.client_id));
        }
        let app: Option<PortalApp> = match cmd.portal_app_id.as_deref().filter(|s| !s.is_empty()) {
            Some(app_id) => Some(load_client_app(&self.apps, &cmd.client_id, app_id).await?),
            None => None,
        };
        let source = if cmd.source == "JIT" {
            IdentitySource::Jit
        } else {
            IdentitySource::Invite
        };
        let existing = self
            .identities
            .find_by_client_and_email(&cmd.client_id, &cmd.email)
            .await?;
        let created = existing.is_none();
        let trimmed_name = cmd.name.as_deref().map(str::trim);
        let mut ident = match existing {
            None => PortalIdentity::new(
                &cmd.client_id,
                &cmd.email,
                trimmed_name.unwrap_or(""),
                source,
            ),
            Some(mut ident) => {
                ident.status = IdentityStatus::Active;
                if let Some(name) = trimmed_name.filter(|n| !n.is_empty()) {
                    ident.name = name.to_string();
                }
                ident
            }
        };
        let mut event = IdentityEnsured::new(
            ctx,
            &ident.id,
            &ident.client_id,
            &ident.email,
            created,
            ident.source.as_str(),
        );
        if let Some(app) = app {
            ident.grant(&app.id, source);
            event.portal_app_id = Some(app.id);
            event.portal_app_code = Some(app.code);
        }
        Ok((ident, event))
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for EnsurePortalIdentityUseCase<U> {
    type Command = EnsureCommand;
    type Event = IdentityEnsured;

    async fn validate(&self, cmd: &EnsureCommand) -> Result<(), UseCaseError> {
        if cmd.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "clientId is required",
            ));
        }
        let email = cmd.email.trim();
        if email.is_empty() {
            return Err(UseCaseError::validation(
                "EMAIL_REQUIRED",
                "email is required",
            ));
        }
        match email.find('@') {
            Some(at) if at > 0 && at < email.len() - 1 => Ok(()),
            _ => Err(UseCaseError::validation(
                "EMAIL_INVALID",
                "email is not valid",
            )),
        }
    }

    async fn authorize(&self, _: &EnsureCommand, _: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: EnsureCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityEnsured> {
        let (ident, event) = match self.prepare(&cmd, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit(&ident, &*self.identities, event, &cmd)
            .await
    }
}

// ── Grant / revoke an app ─────────────────────────────────────────────────

/// Go `portalidentity.AppGrantCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppGrantCommand {
    pub client_id: String,
    pub identity_id: String,
    pub portal_app_id: String,
}

impl crate::usecase::AuditMasked for AppGrantCommand {}

fn validate_app_grant(cmd: &AppGrantCommand) -> Result<(), UseCaseError> {
    if cmd.client_id.trim().is_empty()
        || cmd.identity_id.trim().is_empty()
        || cmd.portal_app_id.trim().is_empty()
    {
        return Err(UseCaseError::validation(
            "TARGET_REQUIRED",
            "clientId, identityId and portalAppId are required",
        ));
    }
    Ok(())
}

async fn load_grant_targets(
    identities: &PortalIdentityRepository,
    apps: &PortalAppRepository,
    cmd: &AppGrantCommand,
) -> Result<(PortalIdentity, PortalApp), UseCaseError> {
    let ident = identities
        .find_by_id(&cmd.identity_id)
        .await?
        .filter(|i| i.client_id == cmd.client_id)
        .ok_or_else(|| not_found("PortalIdentity", &cmd.identity_id))?;
    let app = apps
        .find_by_id(&cmd.portal_app_id)
        .await?
        .filter(|a| a.client_id == cmd.client_id)
        .ok_or_else(|| not_found("PortalApp", &cmd.portal_app_id))?;
    Ok((ident, app))
}

/// Give an existing identity access to one of its client's apps and emit
/// `IdentityAppGranted`. Idempotent.
pub struct GrantPortalIdentityAppUseCase<U: UnitOfWork> {
    identities: Arc<PortalIdentityRepository>,
    apps: Arc<PortalAppRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> GrantPortalIdentityAppUseCase<U> {
    pub fn new(
        identities: Arc<PortalIdentityRepository>,
        apps: Arc<PortalAppRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            identities,
            apps,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for GrantPortalIdentityAppUseCase<U> {
    type Command = AppGrantCommand;
    type Event = IdentityAppGranted;

    async fn validate(&self, cmd: &AppGrantCommand) -> Result<(), UseCaseError> {
        validate_app_grant(cmd)
    }

    async fn authorize(
        &self,
        _: &AppGrantCommand,
        _: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: AppGrantCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityAppGranted> {
        let (mut ident, app) = match load_grant_targets(&self.identities, &self.apps, &cmd).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        ident.grant(&app.id, IdentitySource::Admin);
        let event = IdentityAppGranted::new(
            &ctx,
            &ident.id,
            &ident.client_id,
            &app.id,
            &app.code,
            IdentitySource::Admin.as_str(),
        );
        self.unit_of_work
            .commit(&ident, &*self.identities, event, &cmd)
            .await
    }
}

/// Remove an identity's access to one app (the identity and its access to
/// the client's other portals stay) and emit `IdentityAppRevoked`.
/// Idempotent.
pub struct RevokePortalIdentityAppUseCase<U: UnitOfWork> {
    identities: Arc<PortalIdentityRepository>,
    apps: Arc<PortalAppRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RevokePortalIdentityAppUseCase<U> {
    pub fn new(
        identities: Arc<PortalIdentityRepository>,
        apps: Arc<PortalAppRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            identities,
            apps,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokePortalIdentityAppUseCase<U> {
    type Command = AppGrantCommand;
    type Event = IdentityAppRevoked;

    async fn validate(&self, cmd: &AppGrantCommand) -> Result<(), UseCaseError> {
        validate_app_grant(cmd)
    }

    async fn authorize(
        &self,
        _: &AppGrantCommand,
        _: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: AppGrantCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityAppRevoked> {
        let (mut ident, app) = match load_grant_targets(&self.identities, &self.apps, &cmd).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        ident.revoke(&app.id);
        let event = IdentityAppRevoked::new(&ctx, &ident.id, &ident.client_id, &app.id, &app.code);
        self.unit_of_work
            .commit(&ident, &*self.identities, event, &cmd)
            .await
    }
}

// ── Set status ────────────────────────────────────────────────────────────

/// Go `portalidentity.SetStatusCommand`: `id` targets the identity directly;
/// `clientId` + `email` is the fallback shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetStatusCommand {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub status: String,
}

impl crate::usecase::AuditMasked for SetStatusCommand {}

/// Suspend (DISABLED) or reactivate (ACTIVE) an identity and emit
/// `IdentityStatusSet`.
pub struct SetPortalIdentityStatusUseCase<U: UnitOfWork> {
    identities: Arc<PortalIdentityRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SetPortalIdentityStatusUseCase<U> {
    pub fn new(identities: Arc<PortalIdentityRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            identities,
            unit_of_work,
        }
    }

    async fn prepare(
        &self,
        cmd: &SetStatusCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PortalIdentity, IdentityStatusSet), UseCaseError> {
        let found = if !cmd.id.trim().is_empty() {
            self.identities.find_by_id(&cmd.id).await?
        } else {
            self.identities
                .find_by_client_and_email(&cmd.client_id, &cmd.email)
                .await?
        };
        let mut ident = found.ok_or_else(|| not_found("PortalIdentity", &cmd.id))?;
        // The admin surface is per-client: another client's identity is not
        // mutable through this client's gate.
        if !cmd.client_id.is_empty() && ident.client_id != cmd.client_id {
            return Err(not_found("PortalIdentity", &cmd.id));
        }
        ident.status = IdentityStatus::parse(&cmd.status).unwrap_or(IdentityStatus::Active);
        let event = IdentityStatusSet::new(ctx, &ident.id, &ident.client_id, &cmd.status);
        Ok((ident, event))
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SetPortalIdentityStatusUseCase<U> {
    type Command = SetStatusCommand;
    type Event = IdentityStatusSet;

    async fn validate(&self, cmd: &SetStatusCommand) -> Result<(), UseCaseError> {
        if cmd.id.trim().is_empty()
            && (cmd.client_id.trim().is_empty() || cmd.email.trim().is_empty())
        {
            return Err(UseCaseError::validation(
                "TARGET_REQUIRED",
                "id, or clientId + email, is required",
            ));
        }
        if IdentityStatus::parse(&cmd.status).is_none() {
            return Err(UseCaseError::validation(
                "STATUS_INVALID",
                "status must be ACTIVE or DISABLED",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _: &SetStatusCommand,
        _: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: SetStatusCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityStatusSet> {
        let (ident, event) = match self.prepare(&cmd, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit(&ident, &*self.identities, event, &cmd)
            .await
    }
}

// ── Delete ────────────────────────────────────────────────────────────────

/// Go `portalidentity.DeleteCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCommand {
    pub client_id: String,
    pub id: String,
}

impl crate::usecase::AuditMasked for DeleteCommand {}

/// Remove the identity — offboarding is deleting the row.
pub struct DeletePortalIdentityUseCase<U: UnitOfWork> {
    identities: Arc<PortalIdentityRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeletePortalIdentityUseCase<U> {
    pub fn new(identities: Arc<PortalIdentityRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            identities,
            unit_of_work,
        }
    }

    async fn prepare(
        &self,
        cmd: &DeleteCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PortalIdentity, IdentityDeleted), UseCaseError> {
        let ident = self
            .identities
            .find_by_id(&cmd.id)
            .await?
            .filter(|i| cmd.client_id.is_empty() || i.client_id == cmd.client_id)
            .ok_or_else(|| not_found("PortalIdentity", &cmd.id))?;
        let event = IdentityDeleted::new(ctx, &ident.id, &ident.client_id, &ident.email);
        Ok((ident, event))
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeletePortalIdentityUseCase<U> {
    type Command = DeleteCommand;
    type Event = IdentityDeleted;

    async fn validate(&self, cmd: &DeleteCommand) -> Result<(), UseCaseError> {
        if cmd.id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        Ok(())
    }

    async fn authorize(&self, _: &DeleteCommand, _: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: DeleteCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityDeleted> {
        let (ident, event) = match self.prepare(&cmd, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit_delete(&ident, &*self.identities, event, &cmd)
            .await
    }
}

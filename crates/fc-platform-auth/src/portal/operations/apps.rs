//! Portal app use cases (Go `portalidentity/{app_operations,
//! assign_unassigned}.go`).
//!
//! The multi-aggregate ones (create with its OAuth client, delete with its
//! OAuth clients, bulk assignment) commit several times; the handler runs
//! them inside `PgUnitOfWork::run`, so every commit lands in one transaction
//! (Go's `TxOperation`).

use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::OAuthClientId;
use fc_platform_core::shared::id::OptionIdExt;
use fc_platform_core::shared::id::PortalAppId;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::{
    AssignedToApp, IdentityAppGranted, PortalAppChanged, APP_CREATED, APP_DELETED, APP_UPDATED,
};
use super::{find_client_app, load_client_app, not_found};
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};
use fc_platform_iam::auth::oauth_entity::{GrantType, OAuthClient, OAuthClientType};
use fc_platform_iam::auth::operations::events::{OAuthClientCreated, OAuthClientDeleted};
use fc_platform_iam::portal;
use fc_platform_iam::portal::entity::{
    normalize_app_code, trimmed_or_none, valid_app_code, IdentitySource, PortalApp,
};
use fc_platform_iam::portal::repository::{
    PortalAppRepository, PortalIdentityRepository, PortalOAuthClientReader,
};
use fc_platform_iam::{
    auth::oauth_client_repository::OAuthClientRepository, client::repository::ClientRepository,
};

// ── Create (with its portal OAuth client) ─────────────────────────────────

/// Go `CreateAppWithOAuthClientCommand` (its embedded `CreateAppCommand`
/// flattened, as Go's JSON is). The ids and the secret ref are minted by the
/// handler so it can report them; they are not part of the audited command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAppWithOAuthClientCommand {
    pub client_id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The portal's OAuth callback URL(s), registered on the new OAuth
    /// client.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redirect_uris: Vec<String>,
    /// CONFIDENTIAL (default) or PUBLIC.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_type: String,
    #[serde(skip_serializing, default = "OAuthClientId::generate")]
    pub oauth_client_row_id: OAuthClientId,
    #[serde(skip)]
    pub oauth_client_id: String,
    /// `hashed:v1:` ref of the generated secret (CONFIDENTIAL only).
    #[serde(skip)]
    pub client_secret_ref: Option<String>,
}

impl AuditMasked for CreateAppWithOAuthClientCommand {}

/// Parse Go's `clientType` (exact `PUBLIC` / `CONFIDENTIAL`).
pub fn parse_client_type(s: &str) -> Option<OAuthClientType> {
    match s {
        "PUBLIC" => Some(OAuthClientType::Public),
        "CONFIDENTIAL" => Some(OAuthClientType::Confidential),
        _ => None,
    }
}

/// An absolute http(s) URL with a concrete host and no fragment (Go
/// `validRedirectURI`).
pub fn valid_redirect_uri(raw: &str) -> bool {
    match reqwest::Url::parse(raw.trim()) {
        Ok(u) => {
            (u.scheme() == "https" || u.scheme() == "http")
                && u.host_str()
                    .is_some_and(|h| !h.is_empty() && !h.contains('*'))
                && u.fragment().is_none_or(str::is_empty)
        }
        Err(_) => false,
    }
}

fn validate_create_app(client_id: &str, code: &str, name: &str) -> Result<(), UseCaseError> {
    if client_id.trim().is_empty() {
        return Err(UseCaseError::validation(
            "CLIENT_ID_REQUIRED",
            "clientId is required",
        ));
    }
    if !valid_app_code(&normalize_app_code(code)) {
        return Err(UseCaseError::validation(
            "CODE_INVALID",
            "code must be 1-100 lower-case letters, digits, '-' or '_', starting with a letter or digit",
        ));
    }
    if name.trim().is_empty() {
        return Err(UseCaseError::validation(
            "NAME_REQUIRED",
            "name is required",
        ));
    }
    Ok(())
}

/// Register a portal app AND provision its portal OAuth client — flagged for
/// the app's client, linked to the app, authorization_code only, PKCE
/// required — so a portal is ready to wire up the moment it exists. Either
/// both rows land or neither does (run inside `PgUnitOfWork::run`).
pub struct CreatePortalAppWithOAuthClientUseCase<U: UnitOfWork> {
    apps: Arc<PortalAppRepository>,
    clients: Arc<ClientRepository>,
    oauth_clients: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreatePortalAppWithOAuthClientUseCase<U> {
    pub fn new(
        apps: Arc<PortalAppRepository>,
        clients: Arc<ClientRepository>,
        oauth_clients: Arc<OAuthClientRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            apps,
            clients,
            oauth_clients,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreatePortalAppWithOAuthClientUseCase<U> {
    type Command = CreateAppWithOAuthClientCommand;
    type Event = PortalAppChanged;

    async fn validate(&self, cmd: &CreateAppWithOAuthClientCommand) -> Result<(), UseCaseError> {
        validate_create_app(&cmd.client_id, &cmd.code, &cmd.name)?;
        if !cmd.client_type.is_empty() && parse_client_type(&cmd.client_type).is_none() {
            return Err(UseCaseError::validation(
                "INVALID_CLIENT_TYPE",
                "clientType must be PUBLIC or CONFIDENTIAL",
            ));
        }
        for raw in &cmd.redirect_uris {
            if !valid_redirect_uri(raw) {
                return Err(UseCaseError::validation(
                    "REDIRECT_URI_INVALID",
                    format!("redirectUris must be absolute http(s) URLs without wildcards: {raw}"),
                ));
            }
        }
        Ok(())
    }

    /// Go `CanManagePortalUsers` for the command's client: an anchor, or a
    /// caller holding that client and `platform:iam:portal-user:manage` (403
    /// `SCOPE_FORBIDDEN` / `PERMISSION_REQUIRED`). The `/api/portal` handlers
    /// check it first, where Go's controller does (after the body names the
    /// client); the portal login runs as the system caller.
    async fn authorize(
        &self,
        command: &CreateAppWithOAuthClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(portal::can_write_portal_users(
            ctx.caller(),
            &command.client_id,
        )?)
    }

    async fn execute(
        &self,
        cmd: CreateAppWithOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<PortalAppChanged>, UseCaseError> {
        let client_id = ClientId::from_wire(cmd.client_id.as_str());
        if self.clients.find_by_id(&client_id).await?.is_none() {
            return Err(not_found("Client", &cmd.client_id));
        }
        if let Some(existing) = self
            .apps
            .find_by_client_and_code(&client_id, &cmd.code)
            .await?
        {
            return Err(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!(
                    "portal app code '{}' already exists for this client",
                    existing.code
                ),
            ));
        }
        let mut app = PortalApp::new(&client_id, &cmd.code, &cmd.name);
        app.description = trimmed_or_none(cmd.description.as_deref());

        let client_type =
            parse_client_type(&cmd.client_type).unwrap_or(OAuthClientType::Confidential);
        let mut oc = OAuthClient::builder()
            .client_id(&cmd.oauth_client_id)
            .client_name(format!("{} (portal)", app.name))
            .id(cmd.oauth_client_row_id.clone())
            .client_type(client_type)
            .redirect_uris(
                cmd.redirect_uris
                    .iter()
                    .map(|u| u.trim())
                    .filter(|u| !u.is_empty())
                    .map(String::from)
                    .collect(),
            )
            // Portal logins never get refresh tokens.
            .grant_types(vec![GrantType::AuthorizationCode])
            .default_scopes(vec!["openid".into(), "profile".into(), "email".into()])
            .pkce_required(true)
            .portal_client_id(app.client_id.clone())
            .portal_app_id(app.id.clone())
            .build();
        if client_type == OAuthClientType::Confidential {
            let secret_ref = cmd.client_secret_ref.clone().ok_or_else(|| {
                UseCaseError::internal("SECRET", "no client secret was generated")
            })?;
            oc.set_secret_ref(secret_ref);
        }

        let app_event = PortalAppChanged::new(&ctx, APP_CREATED, &app);
        let oc_event = OAuthClientCreated::new(&ctx, &oc.id, &oc.client_id, &oc.client_name);
        let app_event = self
            .unit_of_work
            .commit(&app, &*self.apps, app_event, &cmd)
            .await?;
        self.unit_of_work
            .commit(&oc, &*self.oauth_clients, oc_event, &cmd)
            .await
            .map(|_| app_event)
    }
}

// ── Update ────────────────────────────────────────────────────────────────

/// Go `UpdateAppCommand`. The code is immutable (portal apps are configured
/// with it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppCommand {
    pub client_id: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

impl AuditMasked for UpdateAppCommand {}

/// Rename/describe/(de)activate a portal app and emit the updated event. An
/// inactive app refuses logins and new grants.
pub struct UpdatePortalAppUseCase<U: UnitOfWork> {
    apps: Arc<PortalAppRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdatePortalAppUseCase<U> {
    pub fn new(apps: Arc<PortalAppRepository>, unit_of_work: Arc<U>) -> Self {
        Self { apps, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdatePortalAppUseCase<U> {
    type Command = UpdateAppCommand;
    type Event = PortalAppChanged;

    async fn validate(&self, cmd: &UpdateAppCommand) -> Result<(), UseCaseError> {
        if cmd.id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        if cmd.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name cannot be empty",
            ));
        }
        Ok(())
    }

    /// Go `CanManagePortalUsers` for the command's client: an anchor, or a
    /// caller holding that client and `platform:iam:portal-user:manage` (403
    /// `SCOPE_FORBIDDEN` / `PERMISSION_REQUIRED`). The `/api/portal` handlers
    /// check it first, where Go's controller does (after the body names the
    /// client); the portal login runs as the system caller.
    async fn authorize(
        &self,
        command: &UpdateAppCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(portal::can_write_portal_users(
            ctx.caller(),
            &command.client_id,
        )?)
    }

    async fn execute(
        &self,
        cmd: UpdateAppCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<PortalAppChanged>, UseCaseError> {
        let mut app = find_client_app(
            &self.apps,
            &cmd.client_id,
            &PortalAppId::from_wire(cmd.id.as_str()),
        )
        .await?;
        if let Some(name) = &cmd.name {
            app.name = name.trim().to_string();
        }
        if cmd.description.is_some() {
            app.description = trimmed_or_none(cmd.description.as_deref());
        }
        if let Some(active) = cmd.active {
            app.active = active;
        }
        let event = PortalAppChanged::new(&ctx, APP_UPDATED, &app);
        self.unit_of_work
            .commit(&app, &*self.apps, event, &cmd)
            .await
    }
}

// ── Delete (with its OAuth clients) ───────────────────────────────────────

/// Go `DeleteAppCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAppCommand {
    pub client_id: String,
    pub id: String,
}

impl AuditMasked for DeleteAppCommand {}

/// Remove a portal app, its grants (FK cascade) AND every OAuth client
/// linked to it, in one transaction. The OAuth clients go too rather than
/// being unlinked: an unlinked portal OAuth client is a legacy client-wide
/// portal, so unlinking would silently widen who can sign in through it.
pub struct DeletePortalAppUseCase<U: UnitOfWork> {
    apps: Arc<PortalAppRepository>,
    portal_oauth: Arc<PortalOAuthClientReader>,
    oauth_clients: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeletePortalAppUseCase<U> {
    pub fn new(
        apps: Arc<PortalAppRepository>,
        portal_oauth: Arc<PortalOAuthClientReader>,
        oauth_clients: Arc<OAuthClientRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            apps,
            portal_oauth,
            oauth_clients,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeletePortalAppUseCase<U> {
    type Command = DeleteAppCommand;
    type Event = PortalAppChanged;

    async fn validate(&self, cmd: &DeleteAppCommand) -> Result<(), UseCaseError> {
        if cmd.id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        Ok(())
    }

    /// Go `CanManagePortalUsers` for the command's client: an anchor, or a
    /// caller holding that client and `platform:iam:portal-user:manage` (403
    /// `SCOPE_FORBIDDEN` / `PERMISSION_REQUIRED`). The `/api/portal` handlers
    /// check it first, where Go's controller does (after the body names the
    /// client); the portal login runs as the system caller.
    async fn authorize(
        &self,
        command: &DeleteAppCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(portal::can_write_portal_users(
            ctx.caller(),
            &command.client_id,
        )?)
    }

    async fn execute(
        &self,
        cmd: DeleteAppCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<PortalAppChanged>, UseCaseError> {
        let app = find_client_app(
            &self.apps,
            &cmd.client_id,
            &PortalAppId::from_wire(cmd.id.as_str()),
        )
        .await?;
        let portal_clients = self
            .portal_oauth
            .find_by_portal_client(&app.client_id)
            .await?;
        let mut deleted = Vec::new();
        for pc in portal_clients
            .iter()
            .filter(|c| c.portal_app_id.as_id_str() == Some(app.id.as_str()))
        {
            // The delete keys on the row id (and the public id for the
            // client cache); the rest of the aggregate is not needed.
            let mut oc = OAuthClient::new(&pc.client_id, &pc.client_name);
            oc.id = pc.id.clone();
            let event = OAuthClientDeleted::new(&ctx, &oc.id, &oc.client_id);
            self.unit_of_work
                .commit_delete(&oc, &*self.oauth_clients, event, &cmd)
                .await?;
            deleted.push(pc.client_id.clone());
        }
        let mut event = PortalAppChanged::new(&ctx, APP_DELETED, &app);
        event.deleted_oauth_client_ids = deleted;
        self.unit_of_work
            .commit_delete(&app, &*self.apps, event, &cmd)
            .await
    }
}

// ── Assign unassigned identities ──────────────────────────────────────────

/// Go `AssignUnassignedCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignUnassignedCommand {
    pub client_id: String,
    pub portal_app_id: String,
}

impl AuditMasked for AssignUnassignedCommand {}

/// The code of the no-op (nobody to assign) — intercepted by the handler,
/// which answers 200 with `assigned: 0` as Go does.
pub const NOTHING_TO_ASSIGN: &str = "NOTHING_TO_ASSIGN";

/// Grant a portal app to every one of the client's identities that holds NO
/// portal app, one `IdentityAppGranted` (source ADMIN) per identity.
/// Identities holding any grant are untouched; status is not changed.
pub struct AssignUnassignedPortalIdentitiesUseCase<U: UnitOfWork> {
    identities: Arc<PortalIdentityRepository>,
    apps: Arc<PortalAppRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AssignUnassignedPortalIdentitiesUseCase<U> {
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
impl<U: UnitOfWork> UseCase for AssignUnassignedPortalIdentitiesUseCase<U> {
    type Command = AssignUnassignedCommand;
    type Event = AssignedToApp;

    async fn validate(&self, cmd: &AssignUnassignedCommand) -> Result<(), UseCaseError> {
        if cmd.client_id.trim().is_empty() || cmd.portal_app_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "TARGET_REQUIRED",
                "clientId and portalAppId are required",
            ));
        }
        Ok(())
    }

    /// Go `CanManagePortalUsers` for the command's client: an anchor, or a
    /// caller holding that client and `platform:iam:portal-user:manage` (403
    /// `SCOPE_FORBIDDEN` / `PERMISSION_REQUIRED`). The `/api/portal` handlers
    /// check it first, where Go's controller does (after the body names the
    /// client); the portal login runs as the system caller.
    async fn authorize(
        &self,
        command: &AssignUnassignedCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(portal::can_write_portal_users(
            ctx.caller(),
            &command.client_id,
        )?)
    }

    async fn execute(
        &self,
        cmd: AssignUnassignedCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<AssignedToApp>, UseCaseError> {
        let client_id = ClientId::from_wire(cmd.client_id.as_str());
        let app = load_client_app(
            &self.apps,
            &client_id,
            &PortalAppId::from_wire(cmd.portal_app_id.as_str()),
        )
        .await?;
        let mut idents = self.identities.find_unassigned(&client_id).await?;
        let Some(mut last) = idents.pop() else {
            let mut details = HashMap::new();
            details.insert("portalAppCode".to_string(), serde_json::json!(app.code));
            return Err(UseCaseError::unchanged(
                NOTHING_TO_ASSIGN,
                "no portal user is without a portal app",
                details,
            ));
        };
        let mut assigned = Vec::with_capacity(idents.len() + 1);
        let admin = IdentitySource::Admin.as_str();
        for ident in idents.iter_mut() {
            ident.grant(&app.id, IdentitySource::Admin);
            let event = IdentityAppGranted::new(
                &ctx,
                &ident.id,
                &ident.client_id,
                &app.id,
                &app.code,
                admin,
            );
            self.unit_of_work
                .commit(&*ident, &*self.identities, event, &cmd)
                .await?;
            assigned.push(ident.id.clone());
        }
        last.grant(&app.id, IdentitySource::Admin);
        assigned.push(last.id.clone());
        let event = AssignedToApp {
            granted: IdentityAppGranted::new(
                &ctx,
                &last.id,
                &last.client_id,
                &app.id,
                &app.code,
                admin,
            ),
            app_code: app.code.clone(),
            identity_ids: assigned,
        };
        self.unit_of_work
            .commit(&last, &*self.identities, event, &cmd)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uris_follow_go() {
        assert!(valid_redirect_uri("https://portal.example.com/callback"));
        assert!(valid_redirect_uri("http://localhost:3000/cb"));
        assert!(!valid_redirect_uri("https://*.example.com/callback"));
        assert!(!valid_redirect_uri("ftp://example.com/cb"));
        assert!(!valid_redirect_uri("https://example.com/cb#frag"));
        assert!(!valid_redirect_uri("/relative"));
    }

    #[test]
    fn client_types_are_exact() {
        assert_eq!(parse_client_type("PUBLIC"), Some(OAuthClientType::Public));
        assert_eq!(
            parse_client_type("CONFIDENTIAL"),
            Some(OAuthClientType::Confidential)
        );
        assert_eq!(parse_client_type("public"), None);
        assert_eq!(parse_client_type("PARTNER"), None);
    }

    #[test]
    fn the_audited_create_command_is_gos_shape() {
        let cmd = CreateAppWithOAuthClientCommand {
            client_id: "clt_1".into(),
            code: "c".into(),
            name: "N".into(),
            description: None,
            redirect_uris: vec![],
            client_type: String::new(),
            oauth_client_row_id: OAuthClientId::parse("oac_1").unwrap(),
            oauth_client_id: "oac_2".into(),
            client_secret_ref: Some("hashed:v1:x".into()),
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"clientId": "clt_1", "code": "c", "name": "N"})
        );
    }
}

//! Create OAuth Client Use Case.
//!
//! Builds an `OAuthClient` from the command and persists it via the
//! OAuth client repository. Emits `OAuthClientCreated` through the UoW.
//! Secret material is opaque to this use case: callers hash the plaintext
//! (`EncryptionService::hash_secret`) and pass only the stored
//! `client_secret_ref`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::OAuthClientCreated;
use crate::auth::oauth_client_repository::OAuthClientRepository;
use crate::auth::oauth_entity::{GrantType, OAuthClient, OAuthClientType};
use crate::portal;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientCommand {
    pub oauth_client_id: String,
    pub client_id: String,
    pub client_name: String,
    pub client_type: OAuthClientType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_ref: Option<String>,
    pub redirect_uris: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_logout_redirect_uris: Vec<String>,
    pub grant_types: Vec<GrantType>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_scopes: Vec<String>,
    pub pkce_required: bool,
    pub application_ids: Vec<String>,
    pub allowed_origins: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_principal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Portal entry point owned by this tenant client (Go `PortalClientID`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portal_client_id: Option<String>,
    /// The portal app this portal client fronts (Go `PortalAppID`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portal_app_id: Option<String>,
    /// Authority-bearing interactive tokens (Go `APIAccess`).
    #[serde(default)]
    pub api_access: bool,
}

impl AuditMasked for CreateOAuthClientCommand {}

pub struct CreateOAuthClientUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateOAuthClientUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            oauth_client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateOAuthClientUseCase<U> {
    type Command = CreateOAuthClientCommand;
    type Event = OAuthClientCreated;

    async fn validate(&self, command: &CreateOAuthClientCommand) -> Result<(), UseCaseError> {
        if command.oauth_client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "OAUTH_CLIENT_ID_REQUIRED",
                "OAuth client id is required",
            ));
        }
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client id is required",
            ));
        }
        if command.client_name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_NAME_REQUIRED",
                "clientName is required",
            ));
        }
        Ok(())
    }

    /// OAuth clients are platform-owner data, written by anchors only (Go's
    /// `Can*OAuthClients` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &CreateOAuthClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: CreateOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<OAuthClientCreated>, UseCaseError> {
        let exists = self
            .oauth_client_repo
            .exists_by_client_id(&command.client_id)
            .await?;
        if exists {
            return Err(UseCaseError::business_rule(
                "OAUTH_CLIENT_EXISTS",
                format!(
                    "OAuth client with clientId '{}' already exists",
                    command.client_id
                ),
            ));
        }

        let client = OAuthClient::builder()
            .client_id(&command.client_id)
            .client_name(&command.client_name)
            .id(command.oauth_client_id.clone())
            .client_type(command.client_type)
            .maybe_client_secret_ref(command.client_secret_ref.clone())
            .redirect_uris(command.redirect_uris.clone())
            .post_logout_redirect_uris(command.post_logout_redirect_uris.clone())
            .grant_types(command.grant_types.clone())
            // An empty list is the default (none).
            .default_scopes(command.default_scopes.clone())
            .pkce_required(command.pkce_required)
            .application_ids(command.application_ids.clone())
            .allowed_origins(command.allowed_origins.clone())
            .maybe_service_account_principal_id(command.service_account_principal_id.clone())
            .maybe_created_by(command.created_by.clone())
            .maybe_portal_client_id(portal::trimmed_or_none(command.portal_client_id.as_deref()))
            .maybe_portal_app_id(portal::trimmed_or_none(command.portal_app_id.as_deref()))
            .api_access(command.api_access)
            .build();
        portal::validate_oauth_client_plane(&client)?;

        let event =
            OAuthClientCreated::new(&ctx, &client.id, &client.client_id, &client.client_name);

        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

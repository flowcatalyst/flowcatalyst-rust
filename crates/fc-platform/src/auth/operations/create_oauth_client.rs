//! Create OAuth Client Use Case.
//!
//! Builds an `OAuthClient` from the command and persists it via the
//! OAuth client repository. Emits `OAuthClientCreated` through the UoW.
//! Secret material is opaque to this use case: callers encrypt the
//! plaintext and pass only the already-encrypted `client_secret_ref`.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::oauth_entity::{GrantType, OAuthClient, OAuthClientType};
use crate::OAuthClientRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::OAuthClientCreated;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientCommand {
    pub oauth_client_id: String,
    pub client_id: String,
    pub client_name: String,
    pub client_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_ref: Option<String>,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_scopes: Vec<String>,
    pub pkce_required: bool,
    pub application_ids: Vec<String>,
    pub allowed_origins: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_principal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

pub struct CreateOAuthClientUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateOAuthClientUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self { oauth_client_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateOAuthClientUseCase<U> {
    type Command = CreateOAuthClientCommand;
    type Event = OAuthClientCreated;

    async fn validate(&self, command: &CreateOAuthClientCommand) -> Result<(), UseCaseError> {
        if command.oauth_client_id.trim().is_empty() {
            return Err(UseCaseError::validation("OAUTH_CLIENT_ID_REQUIRED", "OAuth client id is required"));
        }
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation("CLIENT_ID_REQUIRED", "Client id is required"));
        }
        if command.client_name.trim().is_empty() {
            return Err(UseCaseError::validation("CLIENT_NAME_REQUIRED", "Client name is required"));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &CreateOAuthClientCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<OAuthClientCreated> {
        let exists = match self.oauth_client_repo.exists_by_client_id(&command.client_id).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(UseCaseError::commit(format!(
                "check client_id uniqueness: {}", e,
            ))),
        };
        if exists {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "OAUTH_CLIENT_EXISTS",
                format!("OAuth client with clientId '{}' already exists", command.client_id),
            ));
        }

        let mut client = OAuthClient::new(&command.client_id, &command.client_name);
        client.id = command.oauth_client_id.clone();
        client.client_type = OAuthClientType::from_str(&command.client_type);
        client.client_secret_ref = command.client_secret_ref.clone();
        client.redirect_uris = command.redirect_uris.clone();
        client.grant_types = command.grant_types.iter()
            .filter_map(|g| GrantType::from_str(g))
            .collect();
        if !command.default_scopes.is_empty() {
            client.default_scopes = command.default_scopes.clone();
        }
        client.pkce_required = command.pkce_required;
        client.application_ids = command.application_ids.clone();
        client.allowed_origins = command.allowed_origins.clone();
        client.service_account_principal_id = command.service_account_principal_id.clone();
        client.created_by = command.created_by.clone();

        let event = OAuthClientCreated::new(&ctx, &client.id, &client.client_id);

        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

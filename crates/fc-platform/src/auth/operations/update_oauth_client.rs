//! Update OAuth Client Use Case (generic field update).
//!
//! Handles partial updates of an OAuth client. The narrower activate /
//! deactivate / rotate-secret operations live in their own use cases so
//! the emitted event is specific to the action taken.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::OAuthClientUpdated;
use crate::auth::oauth_entity::{GrantType, OAuthClient};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::OAuthClientRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOAuthClientCommand {
    pub oauth_client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_logout_redirect_uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<GrantType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkce_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

pub struct UpdateOAuthClientUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateOAuthClientUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            oauth_client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateOAuthClientUseCase<U> {
    type Command = UpdateOAuthClientCommand;
    type Event = OAuthClientUpdated;

    async fn validate(&self, command: &UpdateOAuthClientCommand) -> Result<(), UseCaseError> {
        if command.oauth_client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "OAUTH_CLIENT_ID_REQUIRED",
                "OAuth client id is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateOAuthClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<OAuthClientUpdated> {
        let (client, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateOAuthClientUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateOAuthClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(OAuthClient, OAuthClientUpdated), UseCaseError> {
        let mut client = self
            .oauth_client_repo
            .find_by_id(&command.oauth_client_id)
            .await
            .or_not_found(
                "OAUTH_CLIENT_NOT_FOUND",
                format!("OAuth client '{}' not found", command.oauth_client_id),
            )?;

        if let Some(ref name) = command.client_name {
            client.client_name = name.clone();
        }
        if let Some(ref uris) = command.redirect_uris {
            client.redirect_uris = uris.clone();
        }
        if let Some(ref uris) = command.post_logout_redirect_uris {
            client.post_logout_redirect_uris = uris.clone();
        }
        if let Some(ref grants) = command.grant_types {
            client.grant_types = grants.clone();
        }
        if let Some(pkce) = command.pkce_required {
            client.pkce_required = pkce;
        }
        if let Some(ref apps) = command.application_ids {
            client.application_ids = apps.clone();
        }
        if let Some(ref origins) = command.allowed_origins {
            client.allowed_origins = origins.clone();
        }
        if let Some(active) = command.active {
            client.active = active;
        }
        client.updated_at = chrono::Utc::now();

        let event = OAuthClientUpdated::new(ctx, &client.id, &client.client_id);
        Ok((client, event))
    }
}

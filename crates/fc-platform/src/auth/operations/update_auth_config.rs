//! Update ClientAuthConfig Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::AuthConfigUpdated;
use crate::auth::config_entity::{AuthConfigType, AuthProvider, ClientAuthConfig};
use crate::auth::config_repository::ClientAuthConfigRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAuthConfigCommand {
    pub auth_config_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<AuthProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_multi_tenant: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
    /// Replaces the full list when `Some`; None leaves existing IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_client_ids: Option<Vec<String>>,
    /// `ANCHOR` / `PARTNER` / `CLIENT`. Used by the /config-type endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_type: Option<AuthConfigType>,
}

impl crate::usecase::AuditMasked for UpdateAuthConfigCommand {}

pub struct UpdateAuthConfigUseCase<U: UnitOfWork> {
    auth_config_repo: Arc<ClientAuthConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateAuthConfigUseCase<U> {
    pub fn new(auth_config_repo: Arc<ClientAuthConfigRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            auth_config_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateAuthConfigUseCase<U> {
    type Command = UpdateAuthConfigCommand;
    type Event = AuthConfigUpdated;

    async fn validate(&self, command: &UpdateAuthConfigCommand) -> Result<(), UseCaseError> {
        if command.auth_config_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ID_REQUIRED",
                "Auth config ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateAuthConfigCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateAuthConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AuthConfigUpdated> {
        let (config, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&config, &*self.auth_config_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateAuthConfigUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateAuthConfigCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ClientAuthConfig, AuthConfigUpdated), UseCaseError> {
        let mut config = self
            .auth_config_repo
            .find_by_id(&command.auth_config_id)
            .await
            .or_not_found(
                "AUTH_CONFIG_NOT_FOUND",
                format!("Auth config '{}' not found", command.auth_config_id),
            )?;

        if let Some(ref client_id) = command.primary_client_id {
            config.primary_client_id = Some(client_id.clone());
        }
        if let Some(provider) = command.auth_provider {
            config.auth_provider = provider;
        }
        if let Some(ref url) = command.oidc_issuer_url {
            config.oidc_issuer_url = Some(url.clone());
        }
        if let Some(ref id) = command.oidc_client_id {
            config.oidc_client_id = Some(id.clone());
        }
        if let Some(multi) = command.oidc_multi_tenant {
            config.oidc_multi_tenant = multi;
        }
        if let Some(ref pattern) = command.oidc_issuer_pattern {
            config.oidc_issuer_pattern = Some(pattern.clone());
        }
        if let Some(ref secret_ref) = command.oidc_client_secret_ref {
            config.oidc_client_secret_ref = Some(secret_ref.clone());
        }
        if let Some(ref ids) = command.additional_client_ids {
            config.additional_client_ids = ids.clone();
        }
        if let Some(ct) = command.config_type {
            config.config_type = ct;
        }

        config.updated_at = chrono::Utc::now();

        let event = AuthConfigUpdated::new(ctx, &config.id, &config.email_domain);
        Ok((config, event))
    }
}

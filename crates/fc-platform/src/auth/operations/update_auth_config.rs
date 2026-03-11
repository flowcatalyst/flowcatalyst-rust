//! Update ClientAuthConfig Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::auth::config_entity::AuthProvider;
use crate::auth::config_repository::ClientAuthConfigRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::AuthConfigUpdated;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAuthConfigCommand {
    pub auth_config_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
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
}

pub struct UpdateAuthConfigUseCase<U: UnitOfWork> {
    auth_config_repo: Arc<ClientAuthConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateAuthConfigUseCase<U> {
    pub fn new(auth_config_repo: Arc<ClientAuthConfigRepository>, unit_of_work: Arc<U>) -> Self {
        Self { auth_config_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: UpdateAuthConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AuthConfigUpdated> {
        if command.auth_config_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "ID_REQUIRED",
                "Auth config ID is required",
            ));
        }

        let mut config = match self.auth_config_repo.find_by_id(&command.auth_config_id).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "AUTH_CONFIG_NOT_FOUND",
                    format!("Auth config '{}' not found", command.auth_config_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch auth config: {}", e
                )));
            }
        };

        if let Some(ref client_id) = command.primary_client_id {
            config.primary_client_id = Some(client_id.clone());
        }
        if let Some(ref provider) = command.auth_provider {
            config.auth_provider = AuthProvider::from_str(provider);
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

        config.updated_at = chrono::Utc::now();

        let event = AuthConfigUpdated::new(&ctx, &config.id, &config.email_domain);

        self.unit_of_work.commit(&config, event, &command).await
    }
}

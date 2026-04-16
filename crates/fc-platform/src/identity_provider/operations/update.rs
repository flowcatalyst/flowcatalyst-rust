//! Update Identity Provider Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::IdentityProviderRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use super::events::IdentityProviderUpdated;

/// Command for updating an existing identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIdentityProviderCommand {
    pub idp_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
}

/// Use case for updating an existing identity provider.
pub struct UpdateIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateIdentityProviderUseCase<U> {
    pub fn new(idp_repo: Arc<IdentityProviderRepository>, unit_of_work: Arc<U>) -> Self {
        Self { idp_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateIdentityProviderUseCase<U> {
    type Command = UpdateIdentityProviderCommand;
    type Event = IdentityProviderUpdated;

    async fn validate(&self, _command: &UpdateIdentityProviderCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(&self, _command: &UpdateIdentityProviderCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityProviderUpdated> {
        // Fetch existing identity provider
        let mut idp = match self.idp_repo.find_by_id(&command.idp_id).await {
            Ok(Some(idp)) => idp,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "NOT_FOUND",
                    format!("Identity provider with ID '{}' not found", command.idp_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch identity provider: {}", e
                )));
            }
        };

        // Track name change for event
        let mut updated_name: Option<&str> = None;

        // Selectively update fields that are Some
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name != idp.name {
                idp.name = name.to_string();
                updated_name = Some(name);
            }
        }

        if let Some(ref issuer_url) = command.oidc_issuer_url {
            idp.oidc_issuer_url = Some(issuer_url.clone());
        }

        if let Some(ref client_id) = command.oidc_client_id {
            idp.oidc_client_id = Some(client_id.clone());
        }

        if let Some(ref secret_ref) = command.oidc_client_secret_ref {
            idp.oidc_client_secret_ref = Some(secret_ref.clone());
        }

        idp.updated_at = chrono::Utc::now();

        // Create domain event
        let event = IdentityProviderUpdated::new(
            &ctx,
            &idp.id,
            updated_name,
        );

        // Update via repo
        if let Err(e) = self.idp_repo.update(&idp).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to update identity provider: {}", e
            )));
        }

        self.unit_of_work.emit_event(event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateIdentityProviderCommand {
            idp_id: "idp-123".to_string(),
            name: Some("Updated Name".to_string()),
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret_ref: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("idpId"));
        assert!(json.contains("Updated Name"));
    }
}

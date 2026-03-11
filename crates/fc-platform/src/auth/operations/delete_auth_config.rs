//! Delete ClientAuthConfig Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::auth::config_repository::ClientAuthConfigRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::AuthConfigDeleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAuthConfigCommand {
    pub auth_config_id: String,
}

pub struct DeleteAuthConfigUseCase<U: UnitOfWork> {
    auth_config_repo: Arc<ClientAuthConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteAuthConfigUseCase<U> {
    pub fn new(auth_config_repo: Arc<ClientAuthConfigRepository>, unit_of_work: Arc<U>) -> Self {
        Self { auth_config_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: DeleteAuthConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AuthConfigDeleted> {
        if command.auth_config_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "ID_REQUIRED",
                "Auth config ID is required",
            ));
        }

        let config = match self.auth_config_repo.find_by_id(&command.auth_config_id).await {
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

        let event = AuthConfigDeleted::new(&ctx, &config.id, &config.email_domain);

        self.unit_of_work.commit_delete(&config, event, &command).await
    }
}

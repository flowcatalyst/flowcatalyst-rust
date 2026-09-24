//! Delete ClientAuthConfig Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::AuthConfigDeleted;
use crate::auth::config_entity::ClientAuthConfig;
use crate::auth::config_repository::ClientAuthConfigRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAuthConfigCommand {
    pub auth_config_id: String,
}

impl crate::usecase::AuditMasked for DeleteAuthConfigCommand {}

pub struct DeleteAuthConfigUseCase<U: UnitOfWork> {
    auth_config_repo: Arc<ClientAuthConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteAuthConfigUseCase<U> {
    pub fn new(auth_config_repo: Arc<ClientAuthConfigRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            auth_config_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteAuthConfigUseCase<U> {
    type Command = DeleteAuthConfigCommand;
    type Event = AuthConfigDeleted;

    async fn validate(&self, command: &DeleteAuthConfigCommand) -> Result<(), UseCaseError> {
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
        _command: &DeleteAuthConfigCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteAuthConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AuthConfigDeleted> {
        let (config, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&config, &*self.auth_config_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteAuthConfigUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteAuthConfigCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ClientAuthConfig, AuthConfigDeleted), UseCaseError> {
        let config = self
            .auth_config_repo
            .find_by_id(&command.auth_config_id)
            .await
            .or_not_found(
                "AUTH_CONFIG_NOT_FOUND",
                format!("Auth config '{}' not found", command.auth_config_id),
            )?;

        let event = AuthConfigDeleted::new(ctx, &config.id, &config.email_domain);
        Ok((config, event))
    }
}

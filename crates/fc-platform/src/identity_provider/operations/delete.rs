//! Delete Identity Provider Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::IdentityProviderDeleted;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::IdentityProviderRepository;

/// Command for deleting an identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteIdentityProviderCommand {
    pub idp_id: String,
}

/// Use case for deleting an identity provider.
pub struct DeleteIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteIdentityProviderUseCase<U> {
    pub fn new(idp_repo: Arc<IdentityProviderRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            idp_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteIdentityProviderUseCase<U> {
    type Command = DeleteIdentityProviderCommand;
    type Event = IdentityProviderDeleted;

    async fn validate(&self, _command: &DeleteIdentityProviderCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeleteIdentityProviderCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityProviderDeleted> {
        let event = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work.emit_event(event, &command).await
    }
}

impl<U: UnitOfWork> DeleteIdentityProviderUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteIdentityProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<IdentityProviderDeleted, UseCaseError> {
        // Fetch existing identity provider
        let idp = self
            .idp_repo
            .find_by_id(&command.idp_id)
            .await
            .or_not_found(
                "NOT_FOUND",
                format!("Identity provider with ID '{}' not found", command.idp_id),
            )?;

        // Create domain event before delete
        let event = IdentityProviderDeleted::new(ctx, &idp.id, &idp.code);

        // Delete via repo
        if let Err(e) = self.idp_repo.delete(&idp.id).await {
            return Err(UseCaseError::commit(format!(
                "Failed to delete identity provider: {}",
                e
            )));
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteIdentityProviderCommand {
            idp_id: "idp-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("idpId"));
    }
}

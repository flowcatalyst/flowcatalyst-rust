//! Delete IdpRoleMapping Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::config_repository::IdpRoleMappingRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::IdpRoleMappingDeleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteIdpRoleMappingCommand {
    pub mapping_id: String,
}

pub struct DeleteIdpRoleMappingUseCase<U: UnitOfWork> {
    idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteIdpRoleMappingUseCase<U> {
    pub fn new(
        idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { idp_role_mapping_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteIdpRoleMappingUseCase<U> {
    type Command = DeleteIdpRoleMappingCommand;
    type Event = IdpRoleMappingDeleted;

    async fn validate(&self, command: &DeleteIdpRoleMappingCommand) -> Result<(), UseCaseError> {
        if command.mapping_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ID_REQUIRED", "Mapping ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &DeleteIdpRoleMappingCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteIdpRoleMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdpRoleMappingDeleted> {
        let mapping = match self.idp_role_mapping_repo.find_by_id(&command.mapping_id).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "MAPPING_NOT_FOUND",
                    format!("Mapping '{}' not found", command.mapping_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch mapping: {}", e,
                )));
            }
        };

        let event = IdpRoleMappingDeleted::new(&ctx, &mapping.id);

        self.unit_of_work
            .commit_delete(&mapping, &*self.idp_role_mapping_repo, event, &command)
            .await
    }
}

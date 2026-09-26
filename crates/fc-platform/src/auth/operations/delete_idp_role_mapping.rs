//! Delete IdpRoleMapping Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::IdpRoleMappingDeleted;
use crate::auth::config_repository::IdpRoleMappingRepository;
use crate::usecase::{Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteIdpRoleMappingCommand {
    pub mapping_id: String,
}

impl crate::usecase::AuditMasked for DeleteIdpRoleMappingCommand {}

pub struct DeleteIdpRoleMappingUseCase<U: UnitOfWork> {
    idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteIdpRoleMappingUseCase<U> {
    pub fn new(idp_role_mapping_repo: Arc<IdpRoleMappingRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            idp_role_mapping_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteIdpRoleMappingUseCase<U> {
    type Command = DeleteIdpRoleMappingCommand;
    type Event = IdpRoleMappingDeleted;

    async fn validate(&self, command: &DeleteIdpRoleMappingCommand) -> Result<(), UseCaseError> {
        if command.mapping_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ID_REQUIRED",
                "Mapping ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeleteIdpRoleMappingCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteIdpRoleMappingCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<IdpRoleMappingDeleted>, UseCaseError> {
        let mapping = self
            .idp_role_mapping_repo
            .find_by_id(&command.mapping_id)
            .await
            .or_not_found(
                "MAPPING_NOT_FOUND",
                format!("Mapping '{}' not found", command.mapping_id),
            )?;

        let event = IdpRoleMappingDeleted::new(&ctx, &mapping.id, &mapping.idp_role_name);

        self.unit_of_work
            .commit_delete(&mapping, &*self.idp_role_mapping_repo, event, &command)
            .await
    }
}

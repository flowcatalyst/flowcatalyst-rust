//! Create IdpRoleMapping Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::config_entity::IdpRoleMapping;
use crate::auth::config_repository::IdpRoleMappingRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::IdpRoleMappingCreated;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdpRoleMappingCommand {
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
}

pub struct CreateIdpRoleMappingUseCase<U: UnitOfWork> {
    idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateIdpRoleMappingUseCase<U> {
    pub fn new(
        idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { idp_role_mapping_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateIdpRoleMappingUseCase<U> {
    type Command = CreateIdpRoleMappingCommand;
    type Event = IdpRoleMappingCreated;

    async fn validate(&self, command: &CreateIdpRoleMappingCommand) -> Result<(), UseCaseError> {
        if command.idp_role_name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "IDP_ROLE_REQUIRED", "IdP role name is required",
            ));
        }
        if command.platform_role_name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PLATFORM_ROLE_REQUIRED", "Platform role name is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &CreateIdpRoleMappingCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateIdpRoleMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdpRoleMappingCreated> {
        if let Ok(Some(_)) = self.idp_role_mapping_repo
            .find_by_idp_role(&command.idp_type, &command.idp_role_name).await
        {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "MAPPING_EXISTS",
                format!("Mapping for '{}:{}' already exists", command.idp_type, command.idp_role_name),
            ));
        }

        let mapping = IdpRoleMapping::new(
            &command.idp_type,
            &command.idp_role_name,
            &command.platform_role_name,
        );
        let idp_role = format!("{}:{}", mapping.idp_type, mapping.idp_role_name);
        let event = IdpRoleMappingCreated::new(&ctx, &mapping.id, &idp_role, &mapping.platform_role_name);

        self.unit_of_work
            .commit(&mapping, &*self.idp_role_mapping_repo, event, &command)
            .await
    }
}

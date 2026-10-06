//! Delete IdpRoleMapping Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::IdpRoleMappingId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::IdpRoleMappingDeleted;
use crate::auth::config_repository::IdpRoleMappingRepository;
use crate::role::ceiling;
use crate::role::repository::RoleRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};
use std::slice;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteIdpRoleMappingCommand {
    pub mapping_id: IdpRoleMappingId,
}

impl AuditMasked for DeleteIdpRoleMappingCommand {}

pub struct DeleteIdpRoleMappingUseCase<U: UnitOfWork> {
    idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    /// The mapped role's ceiling (owner ruling 14).
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteIdpRoleMappingUseCase<U> {
    pub fn new(
        idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
        role_repo: Arc<RoleRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            idp_role_mapping_repo,
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteIdpRoleMappingUseCase<U> {
    type Command = DeleteIdpRoleMappingCommand;
    type Event = IdpRoleMappingDeleted;

    async fn validate(&self, command: &DeleteIdpRoleMappingCommand) -> Result<(), UseCaseError> {
        if command.mapping_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "ID_REQUIRED",
                "Mapping ID is required",
            ));
        }
        Ok(())
    }

    /// Anchors only, as on create. Removing a mapping withdraws its role, so the
    /// role is bounded by the caller's role ceiling (owner ruling 14); a missing
    /// mapping is left to `execute`'s 404.
    async fn authorize(
        &self,
        command: &DeleteIdpRoleMappingCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        checks::require_anchor_scope(ctx.caller())?;
        if let Some(mapping) = self
            .idp_role_mapping_repo
            .find_by_id(&command.mapping_id)
            .await?
        {
            ceiling::require_role_change(
                ctx.caller(),
                &self.role_repo,
                slice::from_ref(&mapping.platform_role_name),
                &[],
            )
            .await?;
        }
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

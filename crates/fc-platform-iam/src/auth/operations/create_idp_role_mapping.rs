//! Create IdpRoleMapping Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::IdpRoleMappingCreated;
use crate::auth::config_entity::IdpRoleMapping;
use crate::auth::config_repository::IdpRoleMappingRepository;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdpRoleMappingCommand {
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
}

impl fc_platform_core::usecase::AuditMasked for CreateIdpRoleMappingCommand {}

pub struct CreateIdpRoleMappingUseCase<U: UnitOfWork> {
    idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    /// The mapped role's ceiling (owner ruling 14).
    role_repo: Arc<crate::role::repository::RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateIdpRoleMappingUseCase<U> {
    pub fn new(
        idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
        role_repo: Arc<crate::role::repository::RoleRepository>,
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
impl<U: UnitOfWork> UseCase for CreateIdpRoleMappingUseCase<U> {
    type Command = CreateIdpRoleMappingCommand;
    type Event = IdpRoleMappingCreated;

    async fn validate(&self, command: &CreateIdpRoleMappingCommand) -> Result<(), UseCaseError> {
        if command.idp_role_name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "IDP_ROLE_REQUIRED",
                "IdP role name is required",
            ));
        }
        if command.platform_role_name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PLATFORM_ROLE_REQUIRED",
                "Platform role name is required",
            ));
        }
        Ok(())
    }

    /// IdP role mappings are platform-owner data, written by anchors only (the
    /// handler's `can_update_identity_providers` gate checks it, with the
    /// permission, before the body is read). A mapping hands its role out at
    /// login, so the role is bounded by the caller's role ceiling (owner ruling
    /// 14): 403 `ROLE_ABOVE_CALLER`.
    async fn authorize(
        &self,
        command: &CreateIdpRoleMappingCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        fc_platform_core::shared::authorization_service::checks::require_anchor_scope(
            ctx.caller(),
        )?;
        crate::role::ceiling::require_role_change(
            ctx.caller(),
            &self.role_repo,
            &[],
            std::slice::from_ref(&command.platform_role_name),
        )
        .await
    }

    async fn execute(
        &self,
        command: CreateIdpRoleMappingCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<IdpRoleMappingCreated>, UseCaseError> {
        let existing = self
            .idp_role_mapping_repo
            .find_by_idp_role(&command.idp_type, &command.idp_role_name)
            .await?;
        if existing.is_some() {
            return Err(UseCaseError::business_rule(
                "MAPPING_EXISTS",
                format!(
                    "Mapping for '{}:{}' already exists",
                    command.idp_type, command.idp_role_name
                ),
            ));
        }

        let mapping = IdpRoleMapping::new(
            &command.idp_type,
            &command.idp_role_name,
            &command.platform_role_name,
        );
        let event = IdpRoleMappingCreated::new(
            &ctx,
            &mapping.id,
            &mapping.idp_type,
            &mapping.idp_role_name,
            &mapping.platform_role_name,
        );

        self.unit_of_work
            .commit(&mapping, &*self.idp_role_mapping_repo, event, &command)
            .await
    }
}

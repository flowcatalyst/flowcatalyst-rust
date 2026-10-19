//! Create IdpRoleMapping Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;

use super::events::IdpRoleMappingCreated;
use crate::auth::config_entity::IdpRoleMapping;
use crate::auth::config_repository::IdpRoleMappingRepository;
use crate::role::ceiling;
use crate::role::name;
use crate::role::repository::RoleRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};
use std::slice;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdpRoleMappingCommand {
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
}

impl AuditMasked for CreateIdpRoleMappingCommand {}

pub struct CreateIdpRoleMappingUseCase<U: UnitOfWork> {
    idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    /// The mapped role's ceiling (owner ruling 14).
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
    /// Refuse a mapping whose platform role does not exist (Go F33); see
    /// [`require_mapped_role_from_env`].
    require_role: bool,
}

/// Whether a mapping's platform role must exist when the mapping is written
/// (`FC_IDP_MAPPING_REQUIRE_ROLE`, default false). Go refuses an unknown role
/// with `400 ROLE_NOT_FOUND` (F33). It is off by default here because a caller
/// may create a mapping before the role it names exists (an application's
/// roles appear when it first syncs); either way the login skips a mapping
/// whose role is missing, with a warning, and applies the rest.
pub fn require_mapped_role_from_env() -> bool {
    env::var("FC_IDP_MAPPING_REQUIRE_ROLE").is_ok_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

impl<U: UnitOfWork> CreateIdpRoleMappingUseCase<U> {
    pub fn new(
        idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
        role_repo: Arc<RoleRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            idp_role_mapping_repo,
            role_repo,
            unit_of_work,
            require_role: require_mapped_role_from_env(),
        }
    }

    /// Turn the F33 existence check on or off (the default is
    /// `FC_IDP_MAPPING_REQUIRE_ROLE`, off).
    #[must_use]
    pub fn with_required_role(mut self, require: bool) -> Self {
        self.require_role = require;
        self
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
        // Go F18: the platform role is a canonical role name; a malformed one
        // is refused, never trimmed. Existence is checked at write (Go F33,
        // in `execute`); a mapping whose role is deleted later is skipped at
        // login with a warning.
        name::validate_name(&command.platform_role_name)?;
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
        checks::require_anchor_scope(ctx.caller())?;
        ceiling::require_role_change(
            ctx.caller(),
            &self.role_repo,
            &[],
            slice::from_ref(&command.platform_role_name),
        )
        .await
    }

    async fn execute(
        &self,
        command: CreateIdpRoleMappingCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<IdpRoleMappingCreated>, UseCaseError> {
        // Go F33 (when switched on): the mapped platform role must exist when
        // the mapping is written, so a typo is refused here rather than
        // silently skipped at every login.
        if self.require_role
            && self
                .role_repo
                .find_by_name(&command.platform_role_name)
                .await?
                .is_none()
        {
            return Err(UseCaseError::validation(
                "ROLE_NOT_FOUND",
                format!("Role not found: {}", command.platform_role_name),
            ));
        }
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

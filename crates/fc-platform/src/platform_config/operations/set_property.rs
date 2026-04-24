//! Set Platform Config Property Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::platform_config::entity::{ConfigScope, ConfigValueType, PlatformConfig};
use crate::platform_config::repository::PlatformConfigRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::PlatformConfigPropertySet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPlatformConfigPropertyCommand {
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub value: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub struct SetPlatformConfigPropertyUseCase<U: UnitOfWork> {
    config_repo: Arc<PlatformConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SetPlatformConfigPropertyUseCase<U> {
    pub fn new(config_repo: Arc<PlatformConfigRepository>, unit_of_work: Arc<U>) -> Self {
        Self { config_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SetPlatformConfigPropertyUseCase<U> {
    type Command = SetPlatformConfigPropertyCommand;
    type Event = PlatformConfigPropertySet;

    async fn validate(&self, command: &SetPlatformConfigPropertyCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation("APP_CODE_REQUIRED", "Application code is required"));
        }
        if command.section.trim().is_empty() {
            return Err(UseCaseError::validation("SECTION_REQUIRED", "Section is required"));
        }
        if command.property.trim().is_empty() {
            return Err(UseCaseError::validation("PROPERTY_REQUIRED", "Property is required"));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &SetPlatformConfigPropertyCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SetPlatformConfigPropertyCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PlatformConfigPropertySet> {
        // Upsert by natural key: (app_code, section, property, scope, client_id).
        let existing = match self.config_repo
            .find_by_key(
                &command.application_code,
                &command.section,
                &command.property,
                &command.scope,
                command.client_id.as_deref(),
            )
            .await
        {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(UseCaseError::commit(format!(
                "fetch config: {}", e,
            ))),
        };

        let (mut config, was_created) = match existing {
            Some(cfg) => (cfg, false),
            None => (
                PlatformConfig::new(
                    &command.application_code,
                    &command.section,
                    &command.property,
                    &command.value,
                ),
                true,
            ),
        };

        // Apply the patch. On create, also set scope/client_id/value_type.
        config.value = command.value.clone();
        if was_created {
            config.scope = ConfigScope::from_str(&command.scope);
            config.client_id = command.client_id.clone();
        }
        if let Some(ref vt) = command.value_type {
            config.value_type = ConfigValueType::from_str(vt);
        }
        if let Some(ref desc) = command.description {
            config.description = Some(desc.clone());
        }
        config.updated_at = chrono::Utc::now();

        let event = PlatformConfigPropertySet::new(
            &ctx,
            &config.id,
            &config.application_code,
            &config.section,
            &config.property,
            config.scope.as_str(),
            config.client_id.as_deref(),
            config.value_type.as_str(),
            was_created,
        );

        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &command)
            .await
    }
}

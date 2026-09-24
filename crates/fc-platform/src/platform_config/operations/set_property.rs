//! Set Platform Config Property Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PlatformConfigPropertySet;
use crate::platform_config::entity::{ConfigScope, ConfigValueType, PlatformConfig, SECRET_MASK};
use crate::platform_config::repository::PlatformConfigRepository;
use crate::shared::encryption_service::{require_configured, EncryptionService};
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPlatformConfigPropertyCommand {
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub value: String,
    pub scope: ConfigScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<ConfigValueType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl SetPlatformConfigPropertyCommand {
    /// The command as the audit log records it. When the stored property is
    /// a SECRET, the value is masked: the unit of work serialises the command
    /// into `aud_logs.operation_json`, and the plaintext must not land there.
    /// It stays the same type (not a `Cow`) because the audit row's
    /// `operation` is the command's type name.
    pub(crate) fn audit_view(&self, stored_type: ConfigValueType) -> Self {
        match stored_type {
            ConfigValueType::Secret => Self {
                value: SECRET_MASK.to_string(),
                ..self.clone()
            },
            ConfigValueType::Plain => self.clone(),
        }
    }
}

pub struct SetPlatformConfigPropertyUseCase<U: UnitOfWork> {
    config_repo: Arc<PlatformConfigRepository>,
    unit_of_work: Arc<U>,
    /// Encrypts SECRET values before they are stored. `None` when no key is
    /// configured; setting a SECRET then fails rather than store plaintext.
    encryption: Option<Arc<EncryptionService>>,
}

impl<U: UnitOfWork> SetPlatformConfigPropertyUseCase<U> {
    pub fn new(
        config_repo: Arc<PlatformConfigRepository>,
        unit_of_work: Arc<U>,
        encryption: Option<Arc<EncryptionService>>,
    ) -> Self {
        Self {
            config_repo,
            unit_of_work,
            encryption,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SetPlatformConfigPropertyUseCase<U> {
    type Command = SetPlatformConfigPropertyCommand;
    type Event = PlatformConfigPropertySet;

    async fn validate(
        &self,
        command: &SetPlatformConfigPropertyCommand,
    ) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APP_CODE_REQUIRED",
                "Application code is required",
            ));
        }
        if command.section.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SECTION_REQUIRED",
                "Section is required",
            ));
        }
        if command.property.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PROPERTY_REQUIRED",
                "Property is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &SetPlatformConfigPropertyCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SetPlatformConfigPropertyCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PlatformConfigPropertySet> {
        let (config, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        let audited = command.audit_view(config.value_type);
        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &audited)
            .await
    }
}

impl<U: UnitOfWork> SetPlatformConfigPropertyUseCase<U> {
    async fn prepare(
        &self,
        command: &SetPlatformConfigPropertyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PlatformConfig, PlatformConfigPropertySet), UseCaseError> {
        // Upsert by natural key: (app_code, section, property, scope, client_id).
        let existing = self
            .config_repo
            .find_by_key(
                &command.application_code,
                &command.section,
                &command.property,
                command.scope.as_str(),
                command.client_id.as_deref(),
            )
            .await?;

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
        if was_created {
            config.scope = command.scope;
            config.client_id = command.client_id.clone();
        }
        if let Some(vt) = command.value_type {
            config.value_type = vt;
        }
        // The value's type is the patched one: an update that omits
        // `value_type` keeps a SECRET a SECRET.
        config.value = match config.value_type {
            ConfigValueType::Secret => {
                require_configured(self.encryption.as_deref())?.encrypt_ref(&command.value)?
            }
            ConfigValueType::Plain => command.value.clone(),
        };
        if let Some(ref desc) = command.description {
            config.description = Some(desc.clone());
        }
        config.updated_at = chrono::Utc::now();

        let event = PlatformConfigPropertySet {
            metadata: PlatformConfigPropertySet::metadata_for(ctx, &config.id),
            config_id: config.id.clone(),
            application_code: config.application_code.clone(),
            section: config.section.clone(),
            property: config.property.clone(),
            scope: config.scope.as_str().to_string(),
            client_id: config.client_id.clone(),
            value_type: config.value_type.as_str().to_string(),
            was_created,
        };
        Ok((config, event))
    }
}

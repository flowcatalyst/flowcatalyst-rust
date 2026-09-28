//! Set Platform Config Property Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PlatformConfigPropertySet;
use crate::platform_config::entity::{ConfigScope, ConfigValueType, PlatformConfig};
use crate::platform_config::repository::PlatformConfigRepository;
use crate::shared::encryption_service::{require_configured, EncryptionService};
use crate::usecase::{AuditMasked, Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

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
    /// The audit-masked fields of a set-property command whose `valueType`
    /// serialises as `value_type` (owner spec `docs/spec/audit-redaction.md`,
    /// Java repo): `value` unless the type is exactly `PLAIN`. An omitted type
    /// keeps the property's current type, which may be `SECRET`, so it masks
    /// too. `value` matches no secret-key name, so without this declaration a
    /// SECRET value would reach `aud_logs` in the clear.
    ///
    /// Shared by the command's own [`AuditMasked`] impl and the temporary
    /// sweep of stored audit rows, which has only the row's JSON.
    pub fn audit_masked_fields_for(value_type: Option<&str>) -> &'static [&'static str] {
        if value_type == Some(ConfigValueType::Plain.as_str()) {
            &[]
        } else {
            &["value"]
        }
    }
}

impl AuditMasked for SetPlatformConfigPropertyCommand {
    fn audit_masked_fields(&self) -> &'static [&'static str] {
        Self::audit_masked_fields_for(self.value_type.map(|t| t.as_str()))
    }
}

pub struct SetPlatformConfigPropertyUseCase<U: UnitOfWork> {
    config_repo: Arc<PlatformConfigRepository>,
    unit_of_work: Arc<U>,
    /// Encrypts SECRET values before they are stored. `None` when no key is
    /// configured; setting a SECRET then fails rather than store plaintext.
    encryption: Option<Arc<EncryptionService>>,
    /// Role-based access grants, for a non-anchor caller's write access.
    access_repo: Arc<crate::PlatformConfigAccessRepository>,
}

impl<U: UnitOfWork> SetPlatformConfigPropertyUseCase<U> {
    pub fn new(
        config_repo: Arc<PlatformConfigRepository>,
        unit_of_work: Arc<U>,
        encryption: Option<Arc<EncryptionService>>,
        access_repo: Arc<crate::PlatformConfigAccessRepository>,
    ) -> Self {
        Self {
            config_repo,
            unit_of_work,
            encryption,
            access_repo,
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

    /// Write access to the application's config (Go
    /// `operations/set_property.go:60-73`): an anchor, or a write grant on one
    /// of the caller's roles, else 403. The handlers check it first too, where
    /// Go's controller does (before the path's application scope and the
    /// body's 400s).
    async fn authorize(
        &self,
        command: &SetPlatformConfigPropertyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        let roles: &[String] = ctx.caller().auth().map_or(&[], |a| a.roles.as_slice());
        Ok(crate::platform_config::access::require_config_access(
            &self.access_repo,
            ctx.caller().is_anchor(),
            roles,
            &command.application_code,
            true,
        )
        .await?)
    }

    async fn execute(
        &self,
        command: SetPlatformConfigPropertyCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<PlatformConfigPropertySet>, UseCaseError> {
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
        // Go set_property.go: the description is the command's, so a set
        // without one clears it.
        config.description = command.description.clone();
        config.updated_at = chrono::Utc::now();

        let event = PlatformConfigPropertySet::new(&ctx, &config, was_created);

        // The audit row masks `value` through the command's AuditMasked
        // declaration; the unit of work applies it.
        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &command)
            .await
    }
}

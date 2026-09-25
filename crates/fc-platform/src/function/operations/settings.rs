//! A function's config and secrets (Java `function/operations/SetFunctionConfig.java`,
//! `SetFunctionSecret.java`, `DeleteFunctionSecret.java`; spec
//! `function-context.md` §1).
//!
//! Config is a full replacement of the function's map. A secret is set,
//! replaced or deleted one key at a time, and its value reaches neither the
//! event nor the audit row: [`SetSecretCommand`]'s value is a
//! [`SecretValue`], which serialises as `***`, and the command declares the
//! field masked as well. The handlers answer `503 ENCRYPTION_UNCONFIGURED`
//! before any of this runs when there is no app key.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use super::access::{function_by_address, resource_not_found, Caller};
use super::events::{ConfigUpdated, SecretDeleted, SecretSet};
use crate::function::entity::{FunctionConfig, FunctionSecret, SecretValue};
use crate::function::repository::FunctionRepository;
use crate::function::settings_repository::FunctionSettingsRepository;
use crate::function::{FunctionAddress, SettingKey};
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// At most this many config keys (`SETTING_TOO_LARGE`).
pub const MAX_CONFIG_KEYS: usize = 100;
/// At most this many bytes per config or secret value (`SETTING_TOO_LARGE`).
pub const MAX_VALUE_BYTES: usize = 8192;

// ── SetFunctionConfig ───────────────────────────────────────────────────────

/// `PUT /api/functions/{address}/config`: the whole map.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetConfigCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    pub values: BTreeMap<String, String>,
}

impl AuditMasked for SetConfigCommand {}

pub struct SetFunctionConfigUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) settings: Arc<FunctionSettingsRepository>,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SetFunctionConfigUseCase<U> {
    type Command = SetConfigCommand;
    type Event = ConfigUpdated;

    async fn validate(&self, command: &SetConfigCommand) -> Result<(), UseCaseError> {
        if command.values.len() > MAX_CONFIG_KEYS {
            return Err(UseCaseError::validation(
                "SETTING_TOO_LARGE",
                format!(
                    "config carries {} keys, which exceeds the limit of {MAX_CONFIG_KEYS}",
                    command.values.len()
                ),
            ));
        }
        for (key, value) in &command.values {
            SettingKey::parse(key)?;
            if value.len() > MAX_VALUE_BYTES {
                return Err(UseCaseError::validation(
                    "SETTING_TOO_LARGE",
                    format!(
                        "config['{key}'] is {} bytes, which exceeds the limit of {MAX_VALUE_BYTES}",
                        value.len()
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &SetConfigCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SetConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConfigUpdated> {
        let function =
            match function_by_address(&self.functions, &command.address, &self.caller).await {
                Ok(f) => f,
                Err(e) => return UseCaseResult::failure(e),
            };
        let config = FunctionConfig {
            function_id: function.id.clone(),
            values: command.values.clone(),
            updated_by: ctx.principal_id.clone(),
            updated_at: Utc::now(),
        };
        let event = ConfigUpdated::new(&ctx, &function, command.values.keys().cloned().collect());
        self.unit_of_work
            .commit(&config, &*self.settings, event, &command)
            .await
    }
}

// ── SetFunctionSecret ───────────────────────────────────────────────────────

/// `PUT /api/functions/{address}/secrets/{key}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSecretCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    pub key: String,
    /// Serialises as `***`, and is declared masked too.
    pub value: SecretValue,
}

impl AuditMasked for SetSecretCommand {
    fn audit_masked_fields(&self) -> &'static [&'static str] {
        &["value"]
    }
}

pub struct SetFunctionSecretUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) settings: Arc<FunctionSettingsRepository>,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SetFunctionSecretUseCase<U> {
    type Command = SetSecretCommand;
    type Event = SecretSet;

    async fn validate(&self, command: &SetSecretCommand) -> Result<(), UseCaseError> {
        SettingKey::parse(&command.key)?;
        let value = command.value.expose();
        if value.is_empty() {
            return Err(UseCaseError::validation(
                "SETTING_VALUE_REQUIRED",
                "value is required",
            ));
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(UseCaseError::validation(
                "SETTING_TOO_LARGE",
                format!(
                    "value is {} bytes, which exceeds the limit of {MAX_VALUE_BYTES}",
                    value.len()
                ),
            ));
        }
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &SetSecretCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SetSecretCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SecretSet> {
        let function =
            match function_by_address(&self.functions, &command.address, &self.caller).await {
                Ok(f) => f,
                Err(e) => return UseCaseResult::failure(e),
            };
        let secret = FunctionSecret {
            function_id: function.id.clone(),
            key: command.key.clone(),
            value: command.value.clone(),
            updated_by: ctx.principal_id.clone(),
            updated_at: Utc::now(),
        };
        let event = SecretSet::new(&ctx, &function, &command.key);
        self.unit_of_work
            .commit(&secret, &*self.settings, event, &command)
            .await
    }
}

// ── DeleteFunctionSecret ────────────────────────────────────────────────────

/// `DELETE /api/functions/{address}/secrets/{key}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSecretCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    pub key: String,
}

impl AuditMasked for DeleteSecretCommand {}

pub struct DeleteFunctionSecretUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) settings: Arc<FunctionSettingsRepository>,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteFunctionSecretUseCase<U> {
    type Command = DeleteSecretCommand;
    type Event = SecretDeleted;

    async fn validate(&self, command: &DeleteSecretCommand) -> Result<(), UseCaseError> {
        SettingKey::parse(&command.key)?;
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &DeleteSecretCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteSecretCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SecretDeleted> {
        let (secret, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit_delete(&secret, &*self.settings, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteFunctionSecretUseCase<U> {
    /// 404 `FUNCTION_SECRET_NOT_FOUND` when the key was never set.
    async fn prepare(
        &self,
        command: &DeleteSecretCommand,
        ctx: &ExecutionContext,
    ) -> Result<(FunctionSecret, SecretDeleted), UseCaseError> {
        let function = function_by_address(&self.functions, &command.address, &self.caller).await?;
        if !self.settings.has_secret(&function.id, &command.key).await? {
            return Err(resource_not_found("FunctionSecret", &command.key));
        }
        let secret = FunctionSecret {
            function_id: function.id.clone(),
            key: command.key.clone(),
            value: SecretValue::new(String::new()),
            updated_by: ctx.principal_id.clone(),
            updated_at: Utc::now(),
        };
        let event = SecretDeleted::new(ctx, &function, &command.key);
        Ok((secret, event))
    }
}

//! Java `function/operations/PromoteVersion.java` and `RemoveAlias.java`
//! (spec `function-zones-and-aliases.md` §2).
//!
//! **Promote**, in Java's order: the alias name (`400 ALIAS_INVALID`, before
//! anything is read), load and reach (`404`), the version (`404
//! FUNCTION_VERSION_NOT_FOUND`), `409 VERSION_NOT_READY` for a version no
//! host has verified yet (a retired one falls through to
//! `VERSION_RETIRED`), `409 SETTINGS_MISSING`, the promote plan, then the
//! function's own `VERSION_RETIRED`, `FUNCTION_DISABLED` and
//! `ALIAS_UNCHANGED`, then the plan's conflicts. Then one commit of the
//! alias change (`platform:function:alias:changed`), with the public routes
//! when promoting `live` changes them, and the rest of the wiring through
//! [`TriggerSync::apply`], each object with its own event.
//!
//! **An optional precondition** (owner decision 5, beyond Java):
//! `expectedVersion` (or `If-Match`) names the version the caller believes
//! the alias points at, `0` for "no version yet". Checked right after the
//! function is loaded and before anything else about the version, it
//! answers `412 ALIAS_VERSION_CONFLICT` (details `alias`, `expectedVersion`,
//! `currentVersion`) when another promote got there first. Absent, nothing
//! changes.
//!
//! **Wiring is `live`-only**: a named alias is HTTP-only by ruling and
//! never reaches the wiring, since its version's manifest must not be
//! materialised as if it were live.
//!
//! Like Java's `TxOperation`, promote runs on a transaction-scoped unit of
//! work (`PgUnitOfWork::run`): a wiring write that cannot be honoured rolls
//! the alias change back too.
//!
//! **Remove alias** deletes a named pointer: `409 ALIAS_PROTECTED` for
//! `live`, `404 ALIAS_NOT_FOUND` for one the function lacks. HTTP-only, no
//! wiring.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use super::access::{function_by_address, resource_not_found, Caller};
use super::events::{AliasChanged, AliasRemoved};
use super::trigger_sync::{missing_settings, TriggerSync};
use crate::function::entity::{require_valid_alias_name, FunctionVersion, VersionState};
use crate::function::repository::FunctionRepository;
use crate::function::route_repository::{
    FunctionRouteRepository, PromotedFunction, PromotedFunctionRepository,
};
use crate::function::settings_repository::FunctionSettingsRepository;
use crate::function::version_repository::FunctionVersionRepository;
use crate::function::{FunctionAddress, LIVE_ALIAS};
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// `PUT /api/functions/{address}/aliases/{alias}`: `alias` from the path,
/// `version` from the body.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    pub alias: String,
    pub version: i32,
    /// The version the caller expects the alias to point at now (`0`: none
    /// yet); `None` checks nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<i32>,
}

impl AuditMasked for PromoteCommand {}

pub struct PromoteVersionUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) versions: Arc<FunctionVersionRepository>,
    pub(crate) settings: Arc<FunctionSettingsRepository>,
    pub(crate) routes: Arc<FunctionRouteRepository>,
    pub(crate) trigger_sync: TriggerSync,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

/// Everything decided before the commit.
struct Prepared {
    promoted: PromotedFunction,
    version: FunctionVersion,
    plan: super::promote_plan::PromotePlan,
    event: AliasChanged,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for PromoteVersionUseCase<U> {
    type Command = PromoteCommand;
    type Event = AliasChanged;

    /// The alias name, before the function or its version is read, so a
    /// bad name on an unready version is `ALIAS_INVALID`, not
    /// `VERSION_NOT_READY`.
    async fn validate(&self, command: &PromoteCommand) -> Result<(), UseCaseError> {
        require_valid_alias_name(&command.alias)
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &PromoteCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: PromoteCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AliasChanged> {
        let prepared = match self.prepare(&command, &ctx).await {
            Ok(p) => p,
            Err(e) => return UseCaseResult::failure(e),
        };
        let promotions = PromotedFunctionRepository {
            functions: &self.functions,
            routes: &self.routes,
        };
        let result = self
            .unit_of_work
            .commit(&prepared.promoted, &promotions, prepared.event, &command)
            .await;
        if result.as_result().is_err() || command.alias != LIVE_ALIAS {
            return result;
        }
        match self
            .trigger_sync
            .apply(
                &*self.unit_of_work,
                &prepared.promoted.function,
                &prepared.version,
                &ctx,
                &prepared.plan,
            )
            .await
        {
            Ok(()) => result,
            Err(e) => UseCaseResult::failure(e),
        }
    }
}

impl<U: UnitOfWork> PromoteVersionUseCase<U> {
    async fn prepare(
        &self,
        command: &PromoteCommand,
        ctx: &ExecutionContext,
    ) -> Result<Prepared, UseCaseError> {
        let mut function =
            function_by_address(&self.functions, &command.address, &self.caller).await?;
        if let Some(expected) = command.expected_version {
            let current = match function.version_id_of(&command.alias) {
                Some(id) => self.versions.find_by_id(id).await?.map_or(0, |v| v.version),
                None => 0,
            };
            if current != expected {
                return Err(UseCaseError::precondition_failed(
                    "ALIAS_VERSION_CONFLICT",
                    format!(
                        "alias '{}' {}, not the expected version {expected}",
                        command.alias,
                        match current {
                            0 => "points at no version".to_string(),
                            n => format!("points at version {n}"),
                        }
                    ),
                    crate::details! {
                        "alias" => command.alias,
                        "expectedVersion" => expected,
                        "currentVersion" => current,
                    },
                ));
            }
        }
        let version = self
            .versions
            .find_by_function_and_version(&function.id, command.version)
            .await?
            .ok_or_else(|| {
                resource_not_found(
                    "FunctionVersion",
                    &format!("{}#{}", function.address.render(), command.version),
                )
            })?;

        // Only "not yet ready" is this operation's own guard: a retired
        // version is left to the function's own VERSION_RETIRED.
        if version.state == VersionState::Published {
            return Err(UseCaseError::business_rule(
                "VERSION_NOT_READY",
                format!(
                    "version {} has not been verified by any host in pool '{}' yet",
                    version.version,
                    version.manifest.pool.value()
                ),
            ));
        }

        let missing = missing_settings(&self.settings, &function.id, &version.manifest).await?;
        if !missing.is_empty() {
            return Err(UseCaseError::business_rule(
                "SETTINGS_MISSING",
                format!(
                    "the following config/secret keys have no value set: {}",
                    missing.join(", ")
                ),
            ));
        }

        // The wiring diff, read before the promote: `function` still carries
        // the alias's current target.
        let plan = self
            .trigger_sync
            .plan(
                &function,
                &version.manifest,
                version.version,
                &command.alias,
                &self.caller,
            )
            .await?;

        let now = Utc::now();
        let previous = function.promote(&command.alias, &version, &ctx.principal_id, now)?;
        if let Some(conflict) = plan.conflicts.first() {
            return Err(conflict.to_error());
        }
        let event = AliasChanged::new(ctx, &function, &command.alias, &version, previous);
        let routes = if command.alias == LIVE_ALIAS {
            self.trigger_sync
                .routes_for(&function, &version.manifest, &plan, now)
        } else {
            None
        };
        Ok(Prepared {
            promoted: PromotedFunction { function, routes },
            version,
            plan,
            event,
        })
    }
}

/// `DELETE /api/functions/{address}/aliases/{alias}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveAliasCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    pub alias: String,
}

impl AuditMasked for RemoveAliasCommand {}

pub struct RemoveAliasUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) versions: Arc<FunctionVersionRepository>,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RemoveAliasUseCase<U> {
    type Command = RemoveAliasCommand;
    type Event = AliasRemoved;

    async fn validate(&self, _command: &RemoveAliasCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &RemoveAliasCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RemoveAliasCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AliasRemoved> {
        let mut function =
            match function_by_address(&self.functions, &command.address, &self.caller).await {
                Ok(f) => f,
                Err(e) => return UseCaseResult::failure(e),
            };
        let version_id = match function.remove_alias(&command.alias, Utc::now()) {
            Ok(id) => id,
            Err(e) => return UseCaseResult::failure(e),
        };
        // An alias never outlives its version (the FK cascades), so the
        // version it named still exists.
        let version = match self.versions.find_by_id(&version_id).await {
            Ok(Some(v)) => v,
            Ok(None) => {
                return UseCaseResult::failure(resource_not_found("FunctionVersion", &version_id))
            }
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        let event = AliasRemoved::new(&ctx, &function, &command.alias, &version);
        self.unit_of_work
            .commit(&function, &*self.functions, event, &command)
            .await
    }
}

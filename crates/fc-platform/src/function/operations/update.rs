//! Java `function/operations/UpdateFunction.java`: `description` and
//! `status` (absent means untouched), one `function:updated` event. The
//! immutable fields are not on the command at all: rejecting them in the raw
//! body (`FUNCTION_IMMUTABLE_FIELD`) is the handler's job.
//!
//! A real status transition then pauses or resumes the function's linked
//! subscriptions and jobs ([`TriggerSync::on_status_change`]) after the
//! function's own commit, on the same unit of work: a transaction-scoped one
//! (`PgUnitOfWork::run`), as Java's `TxOperation`, so the two land together.
//!
//! **A no-op is not a write** (owner decision 5, beyond Java): a `status`
//! the function already has is skipped, and when nothing changes at all the
//! use case stops before its commit with [`UseCaseError::unchanged`] (no
//! event, no audit row), which the handler answers like a success. The code
//! is `FUNCTION_ALREADY_ACTIVE` / `FUNCTION_ALREADY_DISABLED` for a status
//! alone (Java's 409 for it), `FUNCTION_UNCHANGED` otherwise.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use super::access::{function_by_address, Caller};
use super::events::FunctionUpdated;
use super::trigger_sync::TriggerSync;
use crate::function::entity::Function;
use crate::function::repository::FunctionRepository;
use crate::function::FunctionAddress;
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// `PUT /api/functions/{address}`. `status`, when given, is `ACTIVE` or
/// `DISABLED`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl AuditMasked for UpdateCommand {}

pub struct UpdateFunctionUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) trigger_sync: TriggerSync,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateFunctionUseCase<U> {
    type Command = UpdateCommand;
    type Event = FunctionUpdated;

    async fn validate(&self, _command: &UpdateCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &UpdateCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<FunctionUpdated> {
        let (function, status_changed) = match self.prepare(&command).await {
            Ok(prepared) => prepared,
            Err(e) => return UseCaseResult::failure(e),
        };
        let event = FunctionUpdated::new(&ctx, &function);
        let result = self
            .unit_of_work
            .commit(&function, &*self.functions, event, &command)
            .await;
        if result.as_result().is_err() || !status_changed {
            return result;
        }
        match self
            .trigger_sync
            .on_status_change(&*self.unit_of_work, &function, &ctx)
            .await
        {
            Ok(()) => result,
            Err(e) => UseCaseResult::failure(e),
        }
    }
}

impl<U: UnitOfWork> UpdateFunctionUseCase<U> {
    /// The updated function and whether its status changed; a request
    /// that changes nothing is [`UseCaseError::unchanged`].
    async fn prepare(&self, command: &UpdateCommand) -> Result<(Function, bool), UseCaseError> {
        let mut function =
            function_by_address(&self.functions, &command.address, &self.caller).await?;
        let now = Utc::now();
        let before = function.description.clone();
        if let Some(description) = &command.description {
            function.describe(description.clone(), now);
        }
        let description_changed = function.description != before;
        let mut status_noop = None;
        if let Some(status) = &command.status {
            let transition = match status.as_str() {
                "ACTIVE" => function.enable(now),
                "DISABLED" => function.disable(now),
                _ => {
                    return Err(UseCaseError::validation(
                        "STATUS_INVALID",
                        "status must be ACTIVE or DISABLED",
                    ))
                }
            };
            match transition {
                Ok(()) => {}
                Err(e) if e.is_unchanged() => status_noop = Some(e),
                Err(e) => return Err(e),
            }
        }
        let status_changed = command.status.is_some() && status_noop.is_none();
        if !description_changed && !status_changed {
            return Err(match status_noop {
                Some(noop) if command.description.is_none() => noop,
                _ => UseCaseError::unchanged(
                    "FUNCTION_UNCHANGED",
                    "the function already has this description and status",
                    Default::default(),
                ),
            });
        }
        Ok((function, status_changed))
    }
}

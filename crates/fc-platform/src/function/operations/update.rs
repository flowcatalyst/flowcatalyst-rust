//! Java `function/operations/UpdateFunction.java`: `description` and
//! `status` (absent means untouched), one `function:updated` event. The
//! immutable fields are not on the command at all: rejecting them in the raw
//! body (`FUNCTION_IMMUTABLE_FIELD`) is the handler's job.

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
        let function = match self.prepare(&command, &ctx).await {
            Ok(f) => f,
            Err(e) => return UseCaseResult::failure(e),
        };
        let event = FunctionUpdated::new(&ctx, &function);
        self.unit_of_work
            .commit(&function, &*self.functions, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateFunctionUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateCommand,
        ctx: &ExecutionContext,
    ) -> Result<Function, UseCaseError> {
        let mut function =
            function_by_address(&self.functions, &command.address, &self.caller).await?;
        let now = Utc::now();
        if let Some(description) = &command.description {
            function.describe(description.clone(), now);
        }
        if let Some(status) = &command.status {
            match status.as_str() {
                "ACTIVE" => function.enable(now)?,
                "DISABLED" => function.disable(now)?,
                _ => {
                    return Err(UseCaseError::validation(
                        "STATUS_INVALID",
                        "status must be ACTIVE or DISABLED",
                    ))
                }
            }
            // enable/disable refuse a no-op, so this is a real transition.
            self.trigger_sync.on_status_change(&function, ctx).await?;
        }
        Ok(function)
    }
}

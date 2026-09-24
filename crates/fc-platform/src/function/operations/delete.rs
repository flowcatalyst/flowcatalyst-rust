//! Java `function/operations/DeleteFunction.java`. The database cascades to
//! the function's versions, aliases, routes, config, secrets and
//! trigger-object links; the objects those links name are [`TriggerSync`]'s.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

use super::access::{function_by_address, Caller};
use super::events::FunctionDeleted;
use super::trigger_sync::TriggerSync;
use crate::function::entity::Function;
use crate::function::repository::FunctionRepository;
use crate::function::FunctionAddress;
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// `DELETE /api/functions/{address}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
}

impl AuditMasked for DeleteCommand {}

pub struct DeleteFunctionUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) trigger_sync: TriggerSync,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteFunctionUseCase<U> {
    type Command = DeleteCommand;
    type Event = FunctionDeleted;

    async fn validate(&self, _command: &DeleteCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &DeleteCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<FunctionDeleted> {
        let function = match self.prepare(&command, &ctx).await {
            Ok(f) => f,
            Err(e) => return UseCaseResult::failure(e),
        };
        let event = FunctionDeleted::new(&ctx, &function);
        self.unit_of_work
            .commit_delete(&function, &*self.functions, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteFunctionUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteCommand,
        ctx: &ExecutionContext,
    ) -> Result<Function, UseCaseError> {
        let function = function_by_address(&self.functions, &command.address, &self.caller).await?;
        self.trigger_sync.on_delete(&function, ctx).await?;
        Ok(function)
    }
}

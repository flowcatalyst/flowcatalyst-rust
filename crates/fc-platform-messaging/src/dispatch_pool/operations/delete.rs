//! Delete Dispatch Pool Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::DispatchPoolId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::DispatchPoolDeleted;
use crate::dispatch_pool::repository::DispatchPoolRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::id::OptionIdExt;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deleting a dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDispatchPoolCommand {
    /// Dispatch pool ID
    pub id: DispatchPoolId,
}

impl AuditMasked for DeleteDispatchPoolCommand {}

/// Use case for deleting a dispatch pool.
pub struct DeleteDispatchPoolUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteDispatchPoolUseCase<U> {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteDispatchPoolUseCase<U> {
    type Command = DeleteDispatchPoolCommand;
    type Event = DispatchPoolDeleted;

    async fn validate(&self, _command: &DeleteDispatchPoolCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored pool (Go checks it post-load): a
    /// client's pool needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing pool is `execute`'s 404.
    async fn authorize(
        &self,
        command: &DeleteDispatchPoolCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(target) = self.dispatch_pool_repo.find_by_id(&command.id).await? {
            caller_reach::check_scope_access(ctx.caller(), target.client_id.as_id_str())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<DispatchPoolDeleted>, UseCaseError> {
        // Find the dispatch pool
        let pool = self
            .dispatch_pool_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "DISPATCH_POOL_NOT_FOUND",
                format!("Dispatch pool with ID '{}' not found", command.id),
            )?;

        // Create domain event
        let event = DispatchPoolDeleted::new(&ctx, &pool.id, &pool.code);

        // Atomic commit with delete
        self.unit_of_work
            .commit_delete(&pool, &*self.dispatch_pool_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteDispatchPoolCommand {
            id: DispatchPoolId::from_wire("dp-123"),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("dp-123"));
    }
}

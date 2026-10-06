//! Archive Dispatch Pool Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::DispatchPoolId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::DispatchPoolArchived;
use crate::dispatch_pool::entity::DispatchPoolStatus;
use crate::dispatch_pool::repository::DispatchPoolRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for archiving a dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveDispatchPoolCommand {
    /// Dispatch pool ID
    pub id: DispatchPoolId,
}

impl AuditMasked for ArchiveDispatchPoolCommand {}

/// Use case for archiving a dispatch pool.
pub struct ArchiveDispatchPoolUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ArchiveDispatchPoolUseCase<U> {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ArchiveDispatchPoolUseCase<U> {
    type Command = ArchiveDispatchPoolCommand;
    type Event = DispatchPoolArchived;

    async fn validate(&self, _command: &ArchiveDispatchPoolCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored pool (Go checks it post-load): a
    /// client's pool needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing pool is `execute`'s 404.
    async fn authorize(
        &self,
        command: &ArchiveDispatchPoolCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(target) = self.dispatch_pool_repo.find_by_id(&command.id).await? {
            caller_reach::check_scope_access(ctx.caller(), target.client_id.as_ref())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: ArchiveDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<DispatchPoolArchived>, UseCaseError> {
        // Find the dispatch pool
        let mut pool = self
            .dispatch_pool_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "DISPATCH_POOL_NOT_FOUND",
                format!("Dispatch pool with ID '{}' not found", command.id),
            )?;

        // Business rule: must be active to archive
        if pool.status == DispatchPoolStatus::Archived {
            return Err(UseCaseError::business_rule(
                "DISPATCH_POOL_ALREADY_ARCHIVED",
                "Dispatch pool is already archived",
            ));
        }

        // Archive the dispatch pool
        pool.archive();

        // Create domain event
        let event = DispatchPoolArchived::new(&ctx, &pool.id, &pool.code);

        // Atomic commit
        self.unit_of_work
            .commit(&pool, &*self.dispatch_pool_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = ArchiveDispatchPoolCommand {
            id: DispatchPoolId::from_wire("dp-123"),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("dp-123"));
    }
}

//! Archive Dispatch Pool Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::DispatchPoolArchived;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::DispatchPool;
use crate::DispatchPoolRepository;
use crate::DispatchPoolStatus;

/// Command for archiving a dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveDispatchPoolCommand {
    /// Dispatch pool ID
    pub id: String,
}

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

    async fn authorize(
        &self,
        _command: &ArchiveDispatchPoolCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ArchiveDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolArchived> {
        let (pool, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&pool, &*self.dispatch_pool_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> ArchiveDispatchPoolUseCase<U> {
    async fn prepare(
        &self,
        command: &ArchiveDispatchPoolCommand,
        ctx: &ExecutionContext,
    ) -> Result<(DispatchPool, DispatchPoolArchived), UseCaseError> {
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
        let event = DispatchPoolArchived::new(ctx, &pool.id, &pool.code);
        Ok((pool, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = ArchiveDispatchPoolCommand {
            id: "dp-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("dp-123"));
    }
}

//! Suspend / Activate Dispatch Pool Use Cases
//!
//! Go's `SuspendDispatchPool` / `ActivateDispatchPool`
//! (`dispatchpool/operations/suspend.go`): flip the pool's status to
//! SUSPENDED or ACTIVE and emit `platform:admin:dispatch-pool:suspended` /
//! `:activated`. Neither checks the current status, as Go. The caller's
//! reach over the pool is checked by the handler, which loads it first.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{DispatchPoolActivated, DispatchPoolSuspended};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::DispatchPoolRepository;

/// Command for suspending a dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspendDispatchPoolCommand {
    pub id: String,
}

impl crate::usecase::AuditMasked for SuspendDispatchPoolCommand {}

/// Command for activating a suspended dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateDispatchPoolCommand {
    pub id: String,
}

impl crate::usecase::AuditMasked for ActivateDispatchPoolCommand {}

fn require_id(id: &str) -> Result<(), UseCaseError> {
    if id.trim().is_empty() {
        return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
    }
    Ok(())
}

/// Use case for suspending a dispatch pool.
pub struct SuspendDispatchPoolUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SuspendDispatchPoolUseCase<U> {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SuspendDispatchPoolUseCase<U> {
    type Command = SuspendDispatchPoolCommand;
    type Event = DispatchPoolSuspended;

    async fn validate(&self, command: &SuspendDispatchPoolCommand) -> Result<(), UseCaseError> {
        require_id(&command.id)
    }

    async fn authorize(
        &self,
        _command: &SuspendDispatchPoolCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SuspendDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolSuspended> {
        let mut pool = match self
            .dispatch_pool_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "DISPATCH_POOL_NOT_FOUND",
                format!("Dispatch pool with ID '{}' not found", command.id),
            ) {
            Ok(p) => p,
            Err(e) => return UseCaseResult::failure(e),
        };
        pool.suspend();
        let event = DispatchPoolSuspended::new(&ctx, &pool.id, &pool.code);
        self.unit_of_work
            .commit(&pool, &*self.dispatch_pool_repo, event, &command)
            .await
    }
}

/// Use case for activating a dispatch pool.
pub struct ActivateDispatchPoolUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ActivateDispatchPoolUseCase<U> {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ActivateDispatchPoolUseCase<U> {
    type Command = ActivateDispatchPoolCommand;
    type Event = DispatchPoolActivated;

    async fn validate(&self, command: &ActivateDispatchPoolCommand) -> Result<(), UseCaseError> {
        require_id(&command.id)
    }

    async fn authorize(
        &self,
        _command: &ActivateDispatchPoolCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ActivateDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolActivated> {
        let mut pool = match self
            .dispatch_pool_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "DISPATCH_POOL_NOT_FOUND",
                format!("Dispatch pool with ID '{}' not found", command.id),
            ) {
            Ok(p) => p,
            Err(e) => return UseCaseResult::failure(e),
        };
        pool.activate();
        let event = DispatchPoolActivated::new(&ctx, &pool.id, &pool.code);
        self.unit_of_work
            .commit(&pool, &*self.dispatch_pool_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let s = DispatchPoolSuspended::new(&ctx, "dpl_1", "bulk");
        assert_eq!(
            s.metadata.event_type,
            "platform:admin:dispatch-pool:suspended"
        );
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"poolId": "dpl_1", "code": "bulk"})
        );
        let a = DispatchPoolActivated::new(&ctx, "dpl_1", "bulk");
        assert_eq!(
            a.metadata.event_type,
            "platform:admin:dispatch-pool:activated"
        );
    }
}

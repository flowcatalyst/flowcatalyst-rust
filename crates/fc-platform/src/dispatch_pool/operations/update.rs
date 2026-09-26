//! Update Dispatch Pool Use Case

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::DispatchPoolUpdated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::DispatchPool;
use crate::DispatchPoolRepository;

/// Command for updating a dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDispatchPoolCommand {
    /// Dispatch pool ID
    pub id: String,

    /// Updated name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Updated description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Updated rate limit (messages per minute)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<i32>,

    /// Updated max concurrent dispatches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<i32>,

    /// Who is updating it, for Go's scope check on the loaded pool (never
    /// serialised). `None` is a platform-authored update.
    #[serde(skip)]
    pub caller: Option<crate::shared::authorization_service::AuthContext>,
}

impl crate::usecase::AuditMasked for UpdateDispatchPoolCommand {}

/// Use case for updating a dispatch pool.
pub struct UpdateDispatchPoolUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateDispatchPoolUseCase<U> {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateDispatchPoolUseCase<U> {
    type Command = UpdateDispatchPoolCommand;
    type Event = DispatchPoolUpdated;

    /// Go `UpdateDispatchPool.Validate`.
    async fn validate(&self, command: &UpdateDispatchPoolCommand) -> Result<(), UseCaseError> {
        if command.id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        if command.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name cannot be empty",
            ));
        }
        super::create::validate_counts(command.rate_limit, command.concurrency)
    }

    async fn authorize(
        &self,
        _command: &UpdateDispatchPoolCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolUpdated> {
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

impl<U: UnitOfWork> UpdateDispatchPoolUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateDispatchPoolCommand,
        ctx: &ExecutionContext,
    ) -> Result<(DispatchPool, DispatchPoolUpdated), UseCaseError> {
        // Find the dispatch pool
        let mut pool = self
            .dispatch_pool_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "DISPATCH_POOL_NOT_FOUND",
                format!("Dispatch pool with ID '{}' not found", command.id),
            )?;
        // Go: per-resource scope on the loaded pool.
        if let Some(ref caller) = command.caller {
            crate::shared::caller_reach::check_scope_access(caller, pool.client_id.as_deref())?;
        }

        // Apply name update
        if let Some(ref name) = command.name {
            let name = name.trim();
            if pool.name != name {
                pool.name = name.to_string();
            }
        }

        // Apply description update
        if let Some(ref description) = command.description {
            pool.description = Some(description.clone());
        }

        // Apply rate limit update (Some sets/changes; clearing requires a separate flag)
        if let Some(rate) = command.rate_limit {
            pool.rate_limit = Some(rate);
        }

        // Apply concurrency update
        if let Some(conc) = command.concurrency {
            pool.concurrency = conc;
        }

        pool.updated_at = Utc::now();

        // Create domain event
        let event = DispatchPoolUpdated::new(ctx, &pool.id, &pool.name);
        Ok((pool, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateDispatchPoolCommand {
            id: "dp-123".to_string(),
            name: Some("Updated Name".to_string()),
            description: None,
            rate_limit: Some(2000),
            concurrency: Some(20),
            caller: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("dp-123"));
    }
}

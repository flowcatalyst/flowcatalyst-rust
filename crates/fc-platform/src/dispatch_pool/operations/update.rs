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
    pub rate_limit: Option<u32>,

    /// Updated max concurrent dispatches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
}

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

    async fn validate(&self, _command: &UpdateDispatchPoolCommand) -> Result<(), UseCaseError> {
        Ok(())
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

        // Track changes for event
        let mut updated_name: Option<String> = None;

        // Apply name update
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name.is_empty() {
                return Err(UseCaseError::validation(
                    "INVALID_NAME",
                    "Name cannot be empty",
                ));
            }
            if pool.name != name {
                pool.name = name.to_string();
                updated_name = Some(name.to_string());
            }
        }

        // Apply description update
        if let Some(ref description) = command.description {
            pool.description = Some(description.clone());
        }

        // Apply rate limit update (Some sets/changes; clearing requires a separate flag)
        if let Some(rate) = command.rate_limit {
            pool.rate_limit = Some(rate as i32);
        }

        // Apply concurrency update
        if let Some(conc) = command.concurrency {
            pool.concurrency = conc as i32;
        }

        pool.updated_at = Utc::now();

        // Create domain event
        let event = DispatchPoolUpdated::new(
            ctx,
            &pool.id,
            updated_name.as_deref(),
            command.rate_limit,
            command.concurrency,
        );
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
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("dp-123"));
    }
}

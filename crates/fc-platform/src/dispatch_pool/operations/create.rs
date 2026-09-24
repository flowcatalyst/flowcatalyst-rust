//! Create Dispatch Pool Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::DispatchPoolCreated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::DispatchPool;
use crate::DispatchPoolRepository;

/// Command for creating a new dispatch pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDispatchPoolCommand {
    /// Unique code (URL-safe)
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Client ID (null for anchor-level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Rate limit (messages per minute)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,

    /// Max concurrent dispatches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
}

impl crate::usecase::AuditMasked for CreateDispatchPoolCommand {}

/// Use case for creating a new dispatch pool.
pub struct CreateDispatchPoolUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateDispatchPoolUseCase<U> {
    pub fn new(dispatch_pool_repo: Arc<DispatchPoolRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateDispatchPoolUseCase<U> {
    type Command = CreateDispatchPoolCommand;
    type Event = DispatchPoolCreated;

    async fn validate(&self, command: &CreateDispatchPoolCommand) -> Result<(), UseCaseError> {
        let code = command.code.trim();
        if code.is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Dispatch pool code is required",
            ));
        }

        let name = command.name.trim();
        if name.is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Dispatch pool name is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateDispatchPoolCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolCreated> {
        let code = command.code.trim();
        let name = command.name.trim();

        // Business rule: code must be unique
        let existing = match self
            .dispatch_pool_repo
            .find_by_code(code, command.client_id.as_deref())
            .await
        {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "DISPATCH_POOL_CODE_EXISTS",
                format!("A dispatch pool with code '{}' already exists", code),
            ));
        }

        // Create the dispatch pool entity
        let mut pool = DispatchPool::new(code, name);

        if let Some(ref desc) = command.description {
            pool = pool.with_description(desc);
        }

        if let Some(ref client_id) = command.client_id {
            pool = pool.with_client_id(client_id);
        }

        pool = pool.with_rate_limit(command.rate_limit);

        if let Some(conc) = command.concurrency {
            pool = pool.with_concurrency(conc);
        }

        // Create domain event
        let event = DispatchPoolCreated::new(
            &ctx,
            &pool.id,
            &pool.code,
            &pool.name,
            pool.client_id.as_deref(),
        );

        // Atomic commit
        self.unit_of_work
            .commit(&pool, &*self.dispatch_pool_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateDispatchPoolCommand {
            code: "main-pool".to_string(),
            name: "Main Pool".to_string(),
            description: Some("Primary dispatch pool".to_string()),
            client_id: Some("client-123".to_string()),
            rate_limit: Some(1000),
            concurrency: Some(10),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("main-pool"));
    }

    #[test]
    fn test_dispatch_pool_has_id() {
        let pool = DispatchPool::new("test", "Test");
        assert!(!pool.id().is_empty());
    }
}

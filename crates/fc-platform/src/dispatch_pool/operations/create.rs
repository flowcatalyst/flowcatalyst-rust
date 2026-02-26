//! Create Dispatch Pool Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::DispatchPool;
use crate::DispatchPoolRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
    unit_of_work::HasId,
};
use super::events::DispatchPoolCreated;

impl HasId for DispatchPool {
    fn id(&self) -> &str {
        &self.id
    }

    fn collection_name() -> &'static str {
        "dispatch_pools"
    }
}

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

/// Use case for creating a new dispatch pool.
pub struct CreateDispatchPoolUseCase<U: UnitOfWork> {
    dispatch_pool_repo: Arc<DispatchPoolRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateDispatchPoolUseCase<U> {
    pub fn new(
        dispatch_pool_repo: Arc<DispatchPoolRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            dispatch_pool_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: CreateDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolCreated> {
        // Validation: code is required
        let code = command.code.trim();
        if code.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CODE_REQUIRED",
                "Dispatch pool code is required",
            ));
        }

        // Validation: name is required
        let name = command.name.trim();
        if name.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NAME_REQUIRED",
                "Dispatch pool name is required",
            ));
        }

        // Business rule: code must be unique
        let existing = self.dispatch_pool_repo.find_by_code(code).await;
        if let Ok(Some(_)) = existing {
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

        if let Some(rate) = command.rate_limit {
            pool = pool.with_rate_limit(rate);
        }

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
        self.unit_of_work.commit(&pool, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(DispatchPool::collection_name(), "dispatch_pools");
    }
}

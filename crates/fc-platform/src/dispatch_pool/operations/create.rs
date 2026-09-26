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
    pub rate_limit: Option<i32>,

    /// Max concurrent dispatches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<i32>,

    /// Who is creating it, for Go's scope check (never serialised). `None`
    /// is a platform-authored pool.
    #[serde(skip)]
    pub caller: Option<crate::shared::authorization_service::AuthContext>,
}

/// Go `validate.CodeUnderscorePattern`, the pool-code rule: a lowercase
/// letter, then lowercase alphanumerics, hyphens and underscores.
fn is_pool_code(code: &str) -> bool {
    let mut chars = code.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Go's pool number rules (`dispatchpool/operations/create.go`, `update.go`).
pub(crate) fn validate_counts(
    rate_limit: Option<i32>,
    concurrency: Option<i32>,
) -> Result<(), UseCaseError> {
    if concurrency.is_some_and(|c| c < 1) {
        return Err(UseCaseError::validation(
            "INVALID_CONCURRENCY",
            "concurrency must be >= 1",
        ));
    }
    if rate_limit.is_some_and(|r| r < 0) {
        return Err(UseCaseError::validation(
            "INVALID_RATE_LIMIT",
            "rateLimit cannot be negative",
        ));
    }
    Ok(())
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

    /// Go `CreateDispatchPool.Validate`, its codes and messages.
    async fn validate(&self, command: &CreateDispatchPoolCommand) -> Result<(), UseCaseError> {
        let code = command.code.trim().to_lowercase();
        if code.is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "code is required",
            ));
        }
        if !is_pool_code(&code) {
            return Err(UseCaseError::validation(
                "INVALID_CODE_FORMAT",
                "code must start with a lowercase letter and contain only lowercase alphanumeric, hyphens, underscores",
            ));
        }
        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name is required",
            ));
        }
        validate_counts(command.rate_limit, command.concurrency)
    }

    /// Go `CheckScopeAccess` on the requested client: a client's pool
    /// needs that client, a platform pool anchor scope.
    async fn authorize(
        &self,
        command: &CreateDispatchPoolCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        match command.caller {
            Some(ref caller) => crate::shared::caller_reach::check_scope_access(
                caller,
                command.client_id.as_deref(),
            ),
            None => Ok(()),
        }
    }

    async fn execute(
        &self,
        command: CreateDispatchPoolCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DispatchPoolCreated> {
        let code = command.code.trim().to_lowercase();
        let code = code.as_str();
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
                "CODE_EXISTS",
                format!("Dispatch pool with code '{}' already exists", code),
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

        pool.rate_limit = command.rate_limit;
        if let Some(conc) = command.concurrency {
            pool.concurrency = conc;
        }

        // Create domain event
        let event = DispatchPoolCreated::new(&ctx, &pool.id, &pool.code, &pool.name);

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
            caller: None,
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

#[cfg(test)]
mod go_rules {
    use super::*;

    #[test]
    fn pool_codes_and_counts_follow_go() {
        assert!(is_pool_code("logistics_portal"));
        assert!(is_pool_code("a-1"));
        assert!(!is_pool_code("Bad Code"));
        assert!(!is_pool_code("1pool"));
        assert_eq!(
            validate_counts(None, Some(0)).unwrap_err().code(),
            "INVALID_CONCURRENCY"
        );
        assert_eq!(
            validate_counts(Some(-1), None).unwrap_err().code(),
            "INVALID_RATE_LIMIT"
        );
        assert!(validate_counts(Some(0), Some(1)).is_ok());
    }
}

//! Add CORS Origin Use Case

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::CorsOriginAdded;
use crate::cors::entity::CorsAllowedOrigin;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::CorsOriginRepository;

fn origin_pattern() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^https?://[a-zA-Z0-9*]([a-zA-Z0-9*.-]*[a-zA-Z0-9*])?(:\d+)?$").unwrap()
    })
}

/// Command for adding a new CORS allowed origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCorsOriginCommand {
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl crate::usecase::AuditMasked for AddCorsOriginCommand {}

pub struct AddCorsOriginUseCase<U: UnitOfWork> {
    cors_repo: Arc<CorsOriginRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AddCorsOriginUseCase<U> {
    pub fn new(cors_repo: Arc<CorsOriginRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            cors_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AddCorsOriginUseCase<U> {
    type Command = AddCorsOriginCommand;
    type Event = CorsOriginAdded;

    async fn validate(&self, command: &AddCorsOriginCommand) -> Result<(), UseCaseError> {
        let origin = command.origin.trim();
        if origin.is_empty() {
            return Err(UseCaseError::validation(
                "ORIGIN_REQUIRED",
                "Origin is required",
            ));
        }

        if !origin_pattern().is_match(origin) {
            return Err(UseCaseError::validation(
                "INVALID_ORIGIN_FORMAT",
                "Origin must be a valid URL (e.g. https://example.com or http://localhost:3000)",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &AddCorsOriginCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: AddCorsOriginCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<CorsOriginAdded> {
        let (entity, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&entity, &*self.cors_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> AddCorsOriginUseCase<U> {
    async fn prepare(
        &self,
        command: &AddCorsOriginCommand,
        ctx: &ExecutionContext,
    ) -> Result<(CorsAllowedOrigin, CorsOriginAdded), UseCaseError> {
        let origin = command.origin.trim();

        // Check for duplicate origin
        if self.cors_repo.find_by_origin(origin).await?.is_some() {
            return Err(UseCaseError::validation(
                "ORIGIN_ALREADY_EXISTS",
                format!("CORS origin '{}' already exists", origin),
            ));
        }

        let entity = CorsAllowedOrigin::new(
            origin,
            command.description.clone(),
            Some(ctx.principal_id.clone()),
        );

        let event = CorsOriginAdded::new(ctx, &entity.id, &entity.origin);
        Ok((entity, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = AddCorsOriginCommand {
            origin: "https://example.com".to_string(),
            description: Some("Example origin".to_string()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("https://example.com"));
    }

    #[test]
    fn test_origin_pattern() {
        let pattern = origin_pattern();
        assert!(pattern.is_match("https://example.com"));
        assert!(pattern.is_match("http://localhost:3000"));
        assert!(pattern.is_match("https://*.example.com"));
        assert!(pattern.is_match("https://example.com:8080"));
        assert!(!pattern.is_match("ftp://example.com"));
        assert!(!pattern.is_match("https://"));
        assert!(!pattern.is_match("not-a-url"));
    }
}

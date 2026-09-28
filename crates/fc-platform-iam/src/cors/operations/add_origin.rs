//! Add CORS Origin Use Case

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::CorsOriginAdded;
use crate::cors::entity::CorsAllowedOrigin;
use crate::cors::repository::CorsOriginRepository;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

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

impl fc_platform_core::usecase::AuditMasked for AddCorsOriginCommand {}

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

    /// CORS origins are platform-owner data, written by anchors only (Go's
    /// `Can*CorsOrigins` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &AddCorsOriginCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(
            fc_platform_core::shared::authorization_service::checks::require_anchor_scope(
                ctx.caller(),
            )?,
        )
    }

    async fn execute(
        &self,
        command: AddCorsOriginCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<CorsOriginAdded>, UseCaseError> {
        let origin = command.origin.trim();

        // Check for duplicate origin: 409, as Go's `usecase.Conflict`.
        if self.cors_repo.find_by_origin(origin).await?.is_some() {
            return Err(UseCaseError::business_rule(
                "ORIGIN_ALREADY_EXISTS",
                format!("CORS origin '{}' already exists", origin),
            ));
        }

        let entity = CorsAllowedOrigin::new(
            origin,
            command.description.clone(),
            Some(ctx.principal_id.clone()),
        );

        let event = CorsOriginAdded::new(&ctx, &entity.id, &entity.origin);

        self.unit_of_work
            .commit(&entity, &*self.cors_repo, event, &command)
            .await
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

//! Add CORS Origin Use Case

use std::sync::Arc;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::CorsOriginRepository;
use crate::cors::entity::CorsAllowedOrigin;
use crate::usecase::{ExecutionContext, UseCaseError, UseCaseResult};
use super::events::CorsOriginAdded;

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

pub struct AddCorsOriginUseCase {
    cors_repo: Arc<CorsOriginRepository>,
}

impl AddCorsOriginUseCase {
    pub fn new(cors_repo: Arc<CorsOriginRepository>) -> Self {
        Self { cors_repo }
    }

    pub async fn execute(
        &self,
        command: AddCorsOriginCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<CorsOriginAdded> {
        let origin = command.origin.trim();
        if origin.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "ORIGIN_REQUIRED", "Origin is required",
            ));
        }

        if !origin_pattern().is_match(origin) {
            return UseCaseResult::failure(UseCaseError::validation(
                "INVALID_ORIGIN_FORMAT",
                "Origin must be a valid URL (e.g. https://example.com or http://localhost:3000)",
            ));
        }

        // Check for duplicate origin
        match self.cors_repo.find_by_origin(origin).await {
            Ok(Some(_)) => {
                return UseCaseResult::failure(UseCaseError::validation(
                    "ORIGIN_ALREADY_EXISTS",
                    format!("CORS origin '{}' already exists", origin),
                ));
            }
            Ok(None) => {}
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to check for existing origin: {}", e
                )));
            }
        }

        let entity = CorsAllowedOrigin::new(
            origin,
            command.description.clone(),
            Some(ctx.principal_id.clone()),
        );

        if let Err(e) = self.cors_repo.insert(&entity).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to insert CORS origin: {}", e
            )));
        }

        let event = CorsOriginAdded::new(&ctx, &entity.id, &entity.origin);

        UseCaseResult::success(event)
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

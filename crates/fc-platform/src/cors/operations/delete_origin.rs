//! Delete CORS Origin Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::CorsOriginRepository;
use crate::usecase::{ExecutionContext, UseCaseError, UseCaseResult};
use super::events::CorsOriginDeleted;

/// Command for deleting a CORS allowed origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCorsOriginCommand {
    pub origin_id: String,
}

pub struct DeleteCorsOriginUseCase {
    cors_repo: Arc<CorsOriginRepository>,
}

impl DeleteCorsOriginUseCase {
    pub fn new(cors_repo: Arc<CorsOriginRepository>) -> Self {
        Self { cors_repo }
    }

    pub async fn execute(
        &self,
        command: DeleteCorsOriginCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<CorsOriginDeleted> {
        let origin = match self.cors_repo.find_by_id(&command.origin_id).await {
            Ok(Some(o)) => o,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "NOT_FOUND",
                    format!("CORS origin with ID '{}' not found", command.origin_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch CORS origin: {}", e
                )));
            }
        };

        if let Err(e) = self.cors_repo.delete(&origin.id).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to delete CORS origin: {}", e
            )));
        }

        let event = CorsOriginDeleted::new(&ctx, &origin.id, &origin.origin);

        UseCaseResult::success(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteCorsOriginCommand {
            origin_id: "cors-123".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("originId"));
    }
}

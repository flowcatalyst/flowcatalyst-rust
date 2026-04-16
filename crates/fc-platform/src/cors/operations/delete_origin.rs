//! Delete CORS Origin Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::CorsOriginRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use super::events::CorsOriginDeleted;

/// Command for deleting a CORS allowed origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCorsOriginCommand {
    pub origin_id: String,
}

pub struct DeleteCorsOriginUseCase<U: UnitOfWork> {
    cors_repo: Arc<CorsOriginRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteCorsOriginUseCase<U> {
    pub fn new(cors_repo: Arc<CorsOriginRepository>, unit_of_work: Arc<U>) -> Self {
        Self { cors_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteCorsOriginUseCase<U> {
    type Command = DeleteCorsOriginCommand;
    type Event = CorsOriginDeleted;

    async fn validate(&self, _command: &DeleteCorsOriginCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(&self, _command: &DeleteCorsOriginCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
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

        let event = CorsOriginDeleted::new(&ctx, &origin.id, &origin.origin);

        self.unit_of_work.commit_delete(&origin, event, &command).await
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

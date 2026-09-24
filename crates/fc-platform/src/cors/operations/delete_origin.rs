//! Delete CORS Origin Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::CorsOriginDeleted;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::CorsOriginRepository;

/// Command for deleting a CORS allowed origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCorsOriginCommand {
    pub origin_id: String,
}

impl crate::usecase::AuditMasked for DeleteCorsOriginCommand {}

pub struct DeleteCorsOriginUseCase<U: UnitOfWork> {
    cors_repo: Arc<CorsOriginRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteCorsOriginUseCase<U> {
    pub fn new(cors_repo: Arc<CorsOriginRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            cors_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteCorsOriginUseCase<U> {
    type Command = DeleteCorsOriginCommand;
    type Event = CorsOriginDeleted;

    async fn validate(&self, _command: &DeleteCorsOriginCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeleteCorsOriginCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteCorsOriginCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<CorsOriginDeleted> {
        let (origin, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&origin, &*self.cors_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteCorsOriginUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteCorsOriginCommand,
        ctx: &ExecutionContext,
    ) -> Result<(crate::CorsAllowedOrigin, CorsOriginDeleted), UseCaseError> {
        let origin = self
            .cors_repo
            .find_by_id(&command.origin_id)
            .await
            .or_not_found(
                "NOT_FOUND",
                format!("CORS origin with ID '{}' not found", command.origin_id),
            )?;

        let event = CorsOriginDeleted::new(ctx, &origin.id, &origin.origin);
        Ok((origin, event))
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

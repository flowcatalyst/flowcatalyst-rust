//! Delete CORS Origin Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::CorsOriginId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::CorsOriginDeleted;
use crate::cors::repository::CorsOriginRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deleting a CORS allowed origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCorsOriginCommand {
    pub origin_id: CorsOriginId,
}

impl AuditMasked for DeleteCorsOriginCommand {}

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

    /// CORS origins are platform-owner data, written by anchors only (Go's
    /// `Can*CorsOrigins` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeleteCorsOriginCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeleteCorsOriginCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<CorsOriginDeleted>, UseCaseError> {
        let origin = self
            .cors_repo
            .find_by_id(&command.origin_id)
            .await
            .or_not_found(
                "NOT_FOUND",
                format!("CORS origin with ID '{}' not found", command.origin_id),
            )?;

        let event = CorsOriginDeleted::new(&ctx, &origin.id, &origin.origin);

        self.unit_of_work
            .commit_delete(&origin, &*self.cors_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteCorsOriginCommand {
            origin_id: CorsOriginId::from_wire("cors-123"),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("originId"));
    }
}

//! Delete OAuth Client Use Case.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::OAuthClientDeleted;
use crate::usecase::{Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError};
use crate::OAuthClientRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOAuthClientCommand {
    pub oauth_client_id: String,
}

impl crate::usecase::AuditMasked for DeleteOAuthClientCommand {}

pub struct DeleteOAuthClientUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteOAuthClientUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            oauth_client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteOAuthClientUseCase<U> {
    type Command = DeleteOAuthClientCommand;
    type Event = OAuthClientDeleted;

    async fn validate(&self, command: &DeleteOAuthClientCommand) -> Result<(), UseCaseError> {
        if command.oauth_client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "OAUTH_CLIENT_ID_REQUIRED",
                "OAuth client id is required",
            ));
        }
        Ok(())
    }

    /// OAuth clients are platform-owner data, written by anchors only (Go's
    /// `Can*OAuthClients` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeleteOAuthClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(crate::checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeleteOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<OAuthClientDeleted>, UseCaseError> {
        let client = self
            .oauth_client_repo
            .find_by_id(&command.oauth_client_id)
            .await
            .or_not_found(
                "OAUTH_CLIENT_NOT_FOUND",
                format!("OAuth client '{}' not found", command.oauth_client_id),
            )?;

        let event = OAuthClientDeleted::new(&ctx, &client.id, &client.client_id);

        self.unit_of_work
            .commit_delete(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

//! Deactivate OAuth Client Use Case.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::OAuthClientDeactivated;
use crate::usecase::{Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError};
use crate::OAuthClientRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateOAuthClientCommand {
    pub oauth_client_id: String,
}

impl crate::usecase::AuditMasked for DeactivateOAuthClientCommand {}

pub struct DeactivateOAuthClientUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeactivateOAuthClientUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            oauth_client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeactivateOAuthClientUseCase<U> {
    type Command = DeactivateOAuthClientCommand;
    type Event = OAuthClientDeactivated;

    async fn validate(&self, command: &DeactivateOAuthClientCommand) -> Result<(), UseCaseError> {
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
        _command: &DeactivateOAuthClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(crate::checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeactivateOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<OAuthClientDeactivated>, UseCaseError> {
        let mut client = self
            .oauth_client_repo
            .find_by_id(&command.oauth_client_id)
            .await
            .or_not_found(
                "OAUTH_CLIENT_NOT_FOUND",
                format!("OAuth client '{}' not found", command.oauth_client_id),
            )?;

        client.active = false;
        client.updated_at = chrono::Utc::now();

        let event = OAuthClientDeactivated::new(&ctx, &client.id);

        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

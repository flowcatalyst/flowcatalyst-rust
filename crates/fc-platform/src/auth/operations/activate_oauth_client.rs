//! Activate OAuth Client Use Case.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::OAuthClientActivated;
use crate::usecase::{Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError};
use crate::OAuthClientRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateOAuthClientCommand {
    pub oauth_client_id: String,
}

impl crate::usecase::AuditMasked for ActivateOAuthClientCommand {}

pub struct ActivateOAuthClientUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ActivateOAuthClientUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            oauth_client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ActivateOAuthClientUseCase<U> {
    type Command = ActivateOAuthClientCommand;
    type Event = OAuthClientActivated;

    async fn validate(&self, command: &ActivateOAuthClientCommand) -> Result<(), UseCaseError> {
        if command.oauth_client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "OAUTH_CLIENT_ID_REQUIRED",
                "OAuth client id is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &ActivateOAuthClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ActivateOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<OAuthClientActivated>, UseCaseError> {
        let mut client = self
            .oauth_client_repo
            .find_by_id(&command.oauth_client_id)
            .await
            .or_not_found(
                "OAUTH_CLIENT_NOT_FOUND",
                format!("OAuth client '{}' not found", command.oauth_client_id),
            )?;

        client.active = true;
        client.updated_at = chrono::Utc::now();

        let event = OAuthClientActivated::new(&ctx, &client.id);

        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

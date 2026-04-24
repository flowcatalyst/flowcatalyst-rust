//! Activate OAuth Client Use Case.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::OAuthClientRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::OAuthClientActivated;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateOAuthClientCommand {
    pub oauth_client_id: String,
}

pub struct ActivateOAuthClientUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ActivateOAuthClientUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self { oauth_client_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ActivateOAuthClientUseCase<U> {
    type Command = ActivateOAuthClientCommand;
    type Event = OAuthClientActivated;

    async fn validate(&self, command: &ActivateOAuthClientCommand) -> Result<(), UseCaseError> {
        if command.oauth_client_id.trim().is_empty() {
            return Err(UseCaseError::validation("OAUTH_CLIENT_ID_REQUIRED", "OAuth client id is required"));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &ActivateOAuthClientCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ActivateOAuthClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<OAuthClientActivated> {
        let mut client = match self.oauth_client_repo.find_by_id(&command.oauth_client_id).await {
            Ok(Some(c)) => c,
            Ok(None) => return UseCaseResult::failure(UseCaseError::not_found(
                "OAUTH_CLIENT_NOT_FOUND",
                format!("OAuth client '{}' not found", command.oauth_client_id),
            )),
            Err(e) => return UseCaseResult::failure(UseCaseError::commit(format!(
                "fetch oauth client: {}", e,
            ))),
        };

        client.active = true;
        client.updated_at = chrono::Utc::now();

        let event = OAuthClientActivated::new(&ctx, &client.id, &client.client_id);

        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

//! Revoke OAuth Client Previous Secret Use Case.
//!
//! Ends a secret-rotation overlap now, so the superseded secret stops
//! authenticating before its window would have lapsed (Go's
//! `RevokeOAuthClientPreviousSecret`, auth/operations/oauth_client.go:
//! 411-454). Idempotent: with no overlap in flight it succeeds and changes
//! nothing.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::OAuthClientPreviousSecretRevoked;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::OAuthClientRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeOAuthClientPreviousSecretCommand {
    pub oauth_client_id: String,
}

pub struct RevokeOAuthClientPreviousSecretUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RevokeOAuthClientPreviousSecretUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            oauth_client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokeOAuthClientPreviousSecretUseCase<U> {
    type Command = RevokeOAuthClientPreviousSecretCommand;
    type Event = OAuthClientPreviousSecretRevoked;

    async fn validate(
        &self,
        command: &RevokeOAuthClientPreviousSecretCommand,
    ) -> Result<(), UseCaseError> {
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
        _command: &RevokeOAuthClientPreviousSecretCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RevokeOAuthClientPreviousSecretCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<OAuthClientPreviousSecretRevoked> {
        let mut client = match self
            .oauth_client_repo
            .find_by_id(&command.oauth_client_id)
            .await
            .or_not_found(
                "OAUTH_CLIENT_NOT_FOUND",
                format!("OAuth client '{}' not found", command.oauth_client_id),
            ) {
            Ok(c) => c,
            Err(e) => return UseCaseResult::failure(e),
        };
        client.revoke_previous_secret();

        let event = OAuthClientPreviousSecretRevoked::new(&ctx, &client.id, &client.client_id);
        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

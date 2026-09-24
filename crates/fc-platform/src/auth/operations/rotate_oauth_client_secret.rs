//! Rotate OAuth Client Secret Use Case.
//!
//! Persists a new (already-encrypted) `client_secret_ref` on an existing
//! OAuth client. Secret generation + encryption stays in the handler so
//! the domain layer never touches plaintext secrets.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::OAuthClientSecretRotated;
use crate::auth::oauth_entity::OAuthClient;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::OAuthClientRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateOAuthClientSecretCommand {
    pub oauth_client_id: String,
    /// The already-encrypted secret reference (e.g. `encrypted:…`). The use
    /// case treats this as opaque — encryption happens at the edge so the
    /// plaintext can be returned to the caller without ever crossing the
    /// domain boundary.
    pub new_client_secret_ref: String,
}

pub struct RotateOAuthClientSecretUseCase<U: UnitOfWork> {
    oauth_client_repo: Arc<OAuthClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RotateOAuthClientSecretUseCase<U> {
    pub fn new(oauth_client_repo: Arc<OAuthClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            oauth_client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RotateOAuthClientSecretUseCase<U> {
    type Command = RotateOAuthClientSecretCommand;
    type Event = OAuthClientSecretRotated;

    async fn validate(&self, command: &RotateOAuthClientSecretCommand) -> Result<(), UseCaseError> {
        if command.oauth_client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "OAUTH_CLIENT_ID_REQUIRED",
                "OAuth client id is required",
            ));
        }
        if command.new_client_secret_ref.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SECRET_REF_REQUIRED",
                "New client secret ref is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &RotateOAuthClientSecretCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RotateOAuthClientSecretCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<OAuthClientSecretRotated> {
        let (client, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&client, &*self.oauth_client_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> RotateOAuthClientSecretUseCase<U> {
    async fn prepare(
        &self,
        command: &RotateOAuthClientSecretCommand,
        ctx: &ExecutionContext,
    ) -> Result<(OAuthClient, OAuthClientSecretRotated), UseCaseError> {
        let mut client = self
            .oauth_client_repo
            .find_by_id(&command.oauth_client_id)
            .await
            .or_not_found(
                "OAUTH_CLIENT_NOT_FOUND",
                format!("OAuth client '{}' not found", command.oauth_client_id),
            )?;

        client.client_secret_ref = Some(command.new_client_secret_ref.clone());
        client.updated_at = chrono::Utc::now();

        let event = OAuthClientSecretRotated::new(ctx, &client.id, &client.client_id);
        Ok((client, event))
    }
}

//! Rotate OAuth Client Secret Use Case.
//!
//! Persists a new (already-hashed) `client_secret_ref` on an existing
//! OAuth client and keeps the outgoing secret acceptable for a grace window,
//! as Go's `RotateOAuthClientSecret` does
//! (auth/operations/oauth_client.go:344-408). Secret generation + hashing
//! stays in the handler so the domain layer never touches plaintext secrets.

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
    /// The stored secret reference (`hashed:v1:…`). The use case treats
    /// this as opaque — hashing happens at the edge so the
    /// plaintext can be returned to the caller without ever crossing the
    /// domain boundary.
    pub new_client_secret_ref: String,
    /// How long the outgoing secret stays acceptable, in seconds. `None`
    /// takes [`DEFAULT_SECRET_GRACE_SECONDS`]; 0 is an immediate cutover,
    /// for a secret believed compromised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_seconds: Option<i64>,
}

impl crate::usecase::AuditMasked for RotateOAuthClientSecretCommand {}

/// How long the outgoing secret keeps working after a rotation unless the
/// caller says otherwise (Go's `DefaultSecretGrace`, 24h).
pub const DEFAULT_SECRET_GRACE_SECONDS: i64 = 24 * 60 * 60;

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
        if command.grace_seconds.is_some_and(|g| g < 0) {
            return Err(UseCaseError::validation(
                "GRACE_INVALID",
                "graceSeconds must not be negative",
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

        if !client.is_confidential() {
            return Err(UseCaseError::business_rule(
                "NOT_CONFIDENTIAL",
                "Only CONFIDENTIAL clients have rotatable secrets",
            ));
        }

        let grace = chrono::Duration::seconds(
            command
                .grace_seconds
                .unwrap_or(DEFAULT_SECRET_GRACE_SECONDS),
        );
        let previous_expires_at =
            client.rotate_secret_ref(command.new_client_secret_ref.clone(), grace);

        let mut event = OAuthClientSecretRotated::new(ctx, &client.id);
        event.previous_secret_expires_at = previous_expires_at;
        Ok((client, event))
    }
}

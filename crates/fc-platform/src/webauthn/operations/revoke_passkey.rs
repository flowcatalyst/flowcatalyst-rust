//! Revoke Passkey Use Case
//!
//! Removes a single registered passkey. The caller must be the owning principal.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PasskeyRevoked;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::webauthn::entity::WebauthnCredential;
use crate::webauthn::repository::WebauthnCredentialRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokePasskeyCommand {
    pub credential_id: String,
}

pub struct RevokePasskeyUseCase<U: UnitOfWork> {
    credential_repo: Arc<WebauthnCredentialRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RevokePasskeyUseCase<U> {
    pub fn new(credential_repo: Arc<WebauthnCredentialRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            credential_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokePasskeyUseCase<U> {
    type Command = RevokePasskeyCommand;
    type Event = PasskeyRevoked;

    async fn validate(&self, command: &RevokePasskeyCommand) -> Result<(), UseCaseError> {
        if command.credential_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CREDENTIAL_ID_REQUIRED",
                "credentialId is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &RevokePasskeyCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Owner check happens in execute() because we need to load the row first.
        Ok(())
    }

    async fn execute(
        &self,
        command: RevokePasskeyCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PasskeyRevoked> {
        let (credential, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&credential, &*self.credential_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> RevokePasskeyUseCase<U> {
    async fn prepare(
        &self,
        command: &RevokePasskeyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(WebauthnCredential, PasskeyRevoked), UseCaseError> {
        let credential = self
            .credential_repo
            .find_by_id(&command.credential_id)
            .await
            .or_not_found(
                "CREDENTIAL_NOT_FOUND",
                format!("passkey '{}' not found", command.credential_id),
            )?;

        if ctx.principal_id != credential.principal_id {
            return Err(UseCaseError::business_rule(
                "PRINCIPAL_MISMATCH",
                "you may only revoke your own passkeys",
            ));
        }

        let event = PasskeyRevoked::new(ctx, &credential.id, &credential.principal_id);
        Ok((credential, event))
    }
}

//! Revoke Passkey Use Case
//!
//! Removes a single registered passkey. The caller must be the owning principal.

use async_trait::async_trait;
use fc_platform_core::shared::id::WebauthnCredentialId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PasskeyRevoked;
use crate::webauthn::repository::WebauthnCredentialRepository;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokePasskeyCommand {
    pub credential_id: WebauthnCredentialId,
}

impl AuditMasked for RevokePasskeyCommand {}

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
        if command.credential_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "CREDENTIAL_ID_REQUIRED",
                "credentialId is required",
            ));
        }
        Ok(())
    }

    /// Self-service: only your own passkey (409 `PRINCIPAL_MISMATCH` otherwise).
    /// A missing credential is `execute`'s 404. The handler requires the
    /// signed-in session (401) first.
    async fn authorize(
        &self,
        command: &RevokePasskeyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        let Some(credential) = self
            .credential_repo
            .find_by_id(command.credential_id.as_str())
            .await?
        else {
            return Ok(());
        };
        if ctx.principal_id != credential.principal_id.as_str() {
            return Err(UseCaseError::business_rule(
                "PRINCIPAL_MISMATCH",
                "you may only revoke your own passkeys",
            ));
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: RevokePasskeyCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<PasskeyRevoked>, UseCaseError> {
        let credential = self
            .credential_repo
            .find_by_id(command.credential_id.as_str())
            .await
            .or_not_found(
                "CREDENTIAL_NOT_FOUND",
                format!("passkey '{}' not found", command.credential_id),
            )?;

        let event = PasskeyRevoked::new(&ctx, &credential.id, &credential.principal_id);

        self.unit_of_work
            .commit_delete(&credential, &*self.credential_repo, event, &command)
            .await
    }
}

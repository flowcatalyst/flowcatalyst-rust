//! Register Passkey Use Case
//!
//! Caller has already completed the WebAuthn registration ceremony in the
//! browser; the handler has consumed the matching ceremony state via
//! `WebauthnCeremonyRepository::consume_registration` and now passes the
//! recovered `PasskeyRegistration` plus the browser's
//! `RegisterPublicKeyCredential` response into this use case.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use webauthn_rs::prelude::{PasskeyRegistration, RegisterPublicKeyCredential};

use super::events::PasskeyRegistered;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::webauthn::entity::WebauthnCredential;
use crate::webauthn::repository::WebauthnCredentialRepository;
use crate::webauthn::webauthn_service::WebauthnService;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPasskeyCommand {
    pub principal_id: String,
    pub name: Option<String>,
    pub registration_response: RegisterPublicKeyCredential,
    #[serde(skip)]
    pub registration_state: Option<PasskeyRegistration>,
}

pub struct RegisterPasskeyUseCase<U: UnitOfWork> {
    credential_repo: Arc<WebauthnCredentialRepository>,
    webauthn_service: Arc<WebauthnService>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RegisterPasskeyUseCase<U> {
    pub fn new(
        credential_repo: Arc<WebauthnCredentialRepository>,
        webauthn_service: Arc<WebauthnService>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            credential_repo,
            webauthn_service,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RegisterPasskeyUseCase<U> {
    type Command = RegisterPasskeyCommand;
    type Event = PasskeyRegistered;

    async fn validate(&self, command: &RegisterPasskeyCommand) -> Result<(), UseCaseError> {
        if command.principal_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPAL_REQUIRED",
                "principalId is required",
            ));
        }
        if let Some(name) = &command.name {
            if name.trim().is_empty() {
                return Err(UseCaseError::validation(
                    "NAME_EMPTY",
                    "name must not be blank",
                ));
            }
            if name.chars().count() > 120 {
                return Err(UseCaseError::validation(
                    "NAME_TOO_LONG",
                    "name must be ≤120 characters",
                ));
            }
        }
        if command.registration_state.is_none() {
            return Err(UseCaseError::validation(
                "STATE_MISSING",
                "registration ceremony state was not provided (expired or already used?)",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        command: &RegisterPasskeyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if ctx.principal_id == command.principal_id {
            Ok(())
        } else {
            Err(UseCaseError::business_rule(
                "PRINCIPAL_MISMATCH",
                "you may only register passkeys for your own principal",
            ))
        }
    }

    async fn execute(
        &self,
        command: RegisterPasskeyCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PasskeyRegistered> {
        let (credential, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&credential, &*self.credential_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> RegisterPasskeyUseCase<U> {
    async fn prepare(
        &self,
        command: &RegisterPasskeyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(WebauthnCredential, PasskeyRegistered), UseCaseError> {
        let RegisterPasskeyCommand {
            principal_id,
            name,
            registration_response,
            registration_state,
        } = command.clone();
        let state = registration_state.ok_or_else(|| {
            UseCaseError::business_rule("STATE_MISSING", "registration ceremony state missing")
        })?;

        let passkey = self
            .webauthn_service
            .finish_registration(&registration_response, &state)
            .map_err(|e| UseCaseError::business_rule("REGISTRATION_FAILED", e.to_string()))?;

        // Reject if the credential is somehow already registered (rare — webauthn-rs
        // de-dupes via exclude_credentials at challenge time, but defence in depth).
        if self
            .credential_repo
            .find_by_credential_id(passkey.cred_id().as_ref())
            .await?
            .is_some()
        {
            return Err(UseCaseError::business_rule(
                "CREDENTIAL_EXISTS",
                "this credential is already registered",
            ));
        }

        let credential = WebauthnCredential::new(&principal_id, passkey, name.clone());
        let event = PasskeyRegistered::new(ctx, &credential.id, &principal_id, name);
        Ok((credential, event))
    }
}

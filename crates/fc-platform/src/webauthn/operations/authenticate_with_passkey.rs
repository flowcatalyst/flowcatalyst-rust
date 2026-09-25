//! Authenticate With Passkey Use Case
//!
//! The handler has consumed the matching authentication ceremony state from
//! `oauth_oidc_payloads` and resolved the recovered `PasskeyAuthentication`.
//! This use case completes the assertion check, applies counter / backup-state
//! updates, enforces the hard-cutover domain gate (federated principals can
//! never authenticate with a passkey, even if a stale row exists), and emits
//! `UserLoggedInWithPasskey`.
//!
//! Counter regression: handled inside `webauthn-rs` — its
//! `require_valid_counter_value` defaults to `true`, so the library returns
//! `CredentialPossibleCompromise` when stored > 0 and received ≤ stored.
//! That bubbles out as a `business_rule` failure.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use webauthn_rs::prelude::{PasskeyAuthentication, PublicKeyCredential};

use super::events::UserLoggedInWithPasskey;
use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::webauthn::entity::WebauthnCredential;
use crate::webauthn::repository::WebauthnCredentialRepository;
use crate::webauthn::webauthn_service::WebauthnService;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatePasskeyCommand {
    pub authentication_response: PublicKeyCredential,
    #[serde(skip)]
    pub authentication_state: Option<PasskeyAuthentication>,
}

impl crate::usecase::AuditMasked for AuthenticatePasskeyCommand {}

pub struct AuthenticationOutcome {
    pub principal_id: String,
    pub credential_id: String,
}

pub struct AuthenticatePasskeyUseCase<U: UnitOfWork> {
    credential_repo: Arc<WebauthnCredentialRepository>,
    principal_repo: Arc<PrincipalRepository>,
    email_domain_mapping_repo: Arc<EmailDomainMappingRepository>,
    webauthn_service: Arc<WebauthnService>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AuthenticatePasskeyUseCase<U> {
    pub fn new(
        credential_repo: Arc<WebauthnCredentialRepository>,
        principal_repo: Arc<PrincipalRepository>,
        email_domain_mapping_repo: Arc<EmailDomainMappingRepository>,
        webauthn_service: Arc<WebauthnService>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            credential_repo,
            principal_repo,
            email_domain_mapping_repo,
            webauthn_service,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AuthenticatePasskeyUseCase<U> {
    type Command = AuthenticatePasskeyCommand;
    type Event = UserLoggedInWithPasskey;

    async fn validate(&self, command: &AuthenticatePasskeyCommand) -> Result<(), UseCaseError> {
        if command.authentication_state.is_none() {
            return Err(UseCaseError::validation(
                "STATE_MISSING",
                "authentication ceremony state was not provided (expired or already used?)",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &AuthenticatePasskeyCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Pre-login flow: no caller identity yet. Domain gate is enforced in execute().
        Ok(())
    }

    async fn execute(
        &self,
        command: AuthenticatePasskeyCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserLoggedInWithPasskey> {
        let (credential, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&credential, &*self.credential_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> AuthenticatePasskeyUseCase<U> {
    async fn prepare(
        &self,
        command: &AuthenticatePasskeyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(WebauthnCredential, UserLoggedInWithPasskey), UseCaseError> {
        let state = command.authentication_state.clone().ok_or_else(|| {
            UseCaseError::business_rule("STATE_MISSING", "authentication ceremony state missing")
        })?;

        // 1. Verify the assertion. Rejects on signature mismatch, origin/RP-ID
        //    mismatch, or counter regression (CredentialPossibleCompromise).
        let result = self
            .webauthn_service
            .finish_authentication(&command.authentication_response, &state)
            .map_err(|e| UseCaseError::business_rule("ASSERTION_FAILED", e.to_string()))?;

        // 2. Load the credential row that was just asserted.
        let mut credential = self
            .credential_repo
            .find_by_credential_id(result.cred_id().as_ref())
            .await
            .or_not_found(
                "CREDENTIAL_NOT_FOUND",
                "no stored credential matches the asserted credential id",
            )?;

        // 3. Load the principal.
        let principal = self
            .principal_repo
            .find_by_id(&credential.principal_id)
            .await
            .or_not_found(
                "PRINCIPAL_NOT_FOUND",
                "the credential's principal no longer exists",
            )?;

        if !principal.active {
            return Err(UseCaseError::business_rule(
                "PRINCIPAL_INACTIVE",
                "this account is not active",
            ));
        }

        // 4. Hard-cutover domain gate: if the principal's domain has been
        //    mapped to an OIDC IdP since the passkey was registered, the
        //    passkey is no longer usable — the IdP owns identity.
        let email = principal.email().ok_or_else(|| {
            UseCaseError::business_rule(
                "PRINCIPAL_NO_EMAIL",
                "passkey login requires a principal with an email address",
            )
        })?;
        let domain = email.split('@').nth(1).unwrap_or("").to_lowercase();
        if self
            .email_domain_mapping_repo
            .is_federated_domain(&domain)
            .await?
        {
            return Err(UseCaseError::business_rule(
                "DOMAIN_FEDERATED",
                "this account's domain is federated; sign in via your identity provider",
            ));
        }

        // 5. Apply counter / backup-state updates to the stored Passkey.
        credential.record_authentication(&result);

        // 6. Commit credential update + login event.
        let event = UserLoggedInWithPasskey::new(ctx, &credential.id, &credential.principal_id);
        Ok((credential, event))
    }
}

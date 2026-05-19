//! Authenticate With Passkey Use Case
//!
//! The handler has consumed the matching authentication ceremony state from
//! `oauth_oidc_payloads` and resolved the recovered `PasskeyAuthentication`.
//! This use case completes the assertion check, applies counter / backup-state
//! updates, enforces the hard-cutover per-principal gate (federated principals
//! and password-less principals can never authenticate with a passkey, even
//! if a stale row exists), and emits `UserLoggedInWithPasskey`.
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
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::webauthn::repository::WebauthnCredentialRepository;
use crate::webauthn::webauthn_service::WebauthnService;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatePasskeyCommand {
    pub authentication_response: PublicKeyCredential,
    #[serde(skip)]
    pub authentication_state: Option<PasskeyAuthentication>,
}

pub struct AuthenticationOutcome {
    pub principal_id: String,
    pub credential_id: String,
}

pub struct AuthenticatePasskeyUseCase<U: UnitOfWork> {
    credential_repo: Arc<WebauthnCredentialRepository>,
    principal_repo: Arc<PrincipalRepository>,
    webauthn_service: Arc<WebauthnService>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AuthenticatePasskeyUseCase<U> {
    pub fn new(
        credential_repo: Arc<WebauthnCredentialRepository>,
        principal_repo: Arc<PrincipalRepository>,
        webauthn_service: Arc<WebauthnService>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            credential_repo,
            principal_repo,
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
        let state = match command.authentication_state.clone() {
            Some(s) => s,
            None => {
                return UseCaseResult::failure(UseCaseError::business_rule(
                    "STATE_MISSING",
                    "authentication ceremony state missing",
                ))
            }
        };

        // 1. Verify the assertion. Rejects on signature mismatch, origin/RP-ID
        //    mismatch, or counter regression (CredentialPossibleCompromise).
        let result = match self
            .webauthn_service
            .finish_authentication(&command.authentication_response, &state)
        {
            Ok(r) => r,
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::business_rule(
                    "ASSERTION_FAILED",
                    e.to_string(),
                ))
            }
        };

        // 2. Load the credential row that was just asserted.
        let mut credential = match self
            .credential_repo
            .find_by_credential_id(result.cred_id().as_ref())
            .await
        {
            Ok(Some(c)) => c,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "CREDENTIAL_NOT_FOUND",
                    "no stored credential matches the asserted credential id",
                ))
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to load credential: {}",
                    e,
                )))
            }
        };

        // 3. Load the principal.
        let principal = match self
            .principal_repo
            .find_by_id(&credential.principal_id)
            .await
        {
            Ok(Some(p)) => p,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "PRINCIPAL_NOT_FOUND",
                    "the credential's principal no longer exists",
                ))
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to load principal: {}",
                    e,
                )))
            }
        };

        if !principal.active {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "PRINCIPAL_INACTIVE",
                "this account is not active",
            ));
        }

        // 4. Hard-cutover per-principal gate: if the principal has been
        //    converted to federated (got an external_id) OR has had their
        //    password removed since the passkey was registered, the passkey
        //    is no longer usable — the IdP owns identity, and a password-
        //    less account doesn't qualify for internal-auth-only flows.
        let identity = match principal.user_identity.as_ref() {
            Some(i) => i,
            None => {
                return UseCaseResult::failure(UseCaseError::business_rule(
                    "PRINCIPAL_NOT_USER",
                    "passkey login requires a user principal",
                ))
            }
        };
        if identity.external_id.is_some() || identity.password_hash.is_none() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ACCOUNT_NOT_INTERNAL",
                "this account is managed by an external identity provider; sign in there instead",
            ));
        }

        // 5. Apply counter / backup-state updates to the stored Passkey.
        credential.record_authentication(&result);

        // 6. Commit credential update + login event.
        let event = UserLoggedInWithPasskey::new(&ctx, &credential.id, &credential.principal_id);
        self.unit_of_work
            .commit(&credential, &*self.credential_repo, event, &command)
            .await
    }
}

//! Create Service Account Use Case

use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::client_reach::{dedupe_client_ids, require_clients_exist, resolve_client_reach};
use super::events::ServiceAccountCreated;
use crate::principal::entity::UserScope;
use crate::shared::encryption_service::{require_configured, EncryptionService};
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::{ClientRepository, ServiceAccountRepository};
use crate::{ServiceAccount, WebhookCredentials};

/// Generate a bearer token with fc_ prefix
fn generate_auth_token() -> String {
    let random_part: String = (0..32)
        .map(|_| {
            let idx = rand::rng().random_range(0..36);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect();
    format!("fc_{}", random_part)
}

/// Generate a signing secret (URL-safe base64)
fn generate_signing_secret() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Command for creating a new service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceAccountCommand {
    /// Unique code (1-50 chars)
    pub code: String,

    /// Human-readable name (1-100 chars)
    pub name: String,

    /// Optional description (max 500 chars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Client tier. Absent, it follows `client_ids` (none → ANCHOR, one →
    /// CLIENT, several → PARTNER), as in Go; present, `client_ids` must agree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<UserScope>,

    /// Client IDs this account can access
    #[serde(default)]
    pub client_ids: Vec<String>,

    /// Application ID, set only by application provisioning. The account is
    /// bound to it and granted it; otherwise it starts with no application
    /// access at all (owner ruling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
}

/// Result returned from create service account use case.
/// Contains the event plus one-time secrets that need to be returned to caller.
/// The secrets are never serialized, so this serializes exactly as the event.
#[derive(Serialize)]
pub struct CreateServiceAccountResult {
    #[serde(flatten)]
    pub event: ServiceAccountCreated,
    #[serde(skip_serializing)]
    pub auth_token: String,
    #[serde(skip_serializing)]
    pub signing_secret: String,
}

crate::impl_domain_event!(CreateServiceAccountResult => event);

/// Use case for creating a new service account.
pub struct CreateServiceAccountUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
    /// Encrypts the generated credential before it is stored. `None` when no
    /// key is configured; the use case then fails rather than store plaintext.
    encryption: Option<Arc<EncryptionService>>,
}

impl<U: UnitOfWork> CreateServiceAccountUseCase<U> {
    pub fn new(
        service_account_repo: Arc<ServiceAccountRepository>,
        client_repo: Arc<ClientRepository>,
        unit_of_work: Arc<U>,
        encryption: Option<Arc<EncryptionService>>,
    ) -> Self {
        Self {
            service_account_repo,
            client_repo,
            unit_of_work,
            encryption,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateServiceAccountUseCase<U> {
    type Command = CreateServiceAccountCommand;
    type Event = CreateServiceAccountResult;

    async fn validate(&self, command: &CreateServiceAccountCommand) -> Result<(), UseCaseError> {
        let code = command.code.trim();
        if code.is_empty() || code.len() > 50 {
            return Err(UseCaseError::validation(
                "INVALID_CODE",
                "Code must be 1-50 characters",
            ));
        }

        let name = command.name.trim();
        if name.is_empty() || name.len() > 100 {
            return Err(UseCaseError::validation(
                "INVALID_NAME",
                "Name must be 1-100 characters",
            ));
        }

        if let Some(ref desc) = command.description {
            if desc.len() > 500 {
                return Err(UseCaseError::validation(
                    "INVALID_DESCRIPTION",
                    "Description must be max 500 characters",
                ));
            }
        }

        resolve_client_reach(
            command.scope,
            &dedupe_client_ids(command.client_ids.clone()),
        )?;

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateServiceAccountCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateServiceAccountCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<CreateServiceAccountResult> {
        let code = command.code.trim();
        let name = command.name.trim();

        // Business rule: code must be unique
        let existing = match self.service_account_repo.find_by_code(code).await {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "SERVICE_ACCOUNT_CODE_EXISTS",
                format!("A service account with code '{}' already exists", code),
            ));
        }

        // The account's reach lands on its principal, so every token built
        // from it carries the chosen scope, not ANCHOR.
        let client_ids = dedupe_client_ids(command.client_ids.clone());
        let scope = match resolve_client_reach(command.scope, &client_ids) {
            Ok(scope) => scope,
            Err(e) => return UseCaseResult::failure(e),
        };
        if let Err(e) = require_clients_exist(&self.client_repo, &client_ids).await {
            return UseCaseResult::failure(e);
        }

        // Generate credentials. The caller gets the plaintext once in the
        // result; only the `encrypted:` form is stored.
        let auth_token = generate_auth_token();
        let signing_secret = generate_signing_secret();
        let sealed = require_configured(self.encryption.as_deref()).and_then(|enc| {
            Ok((
                enc.encrypt_ref(&auth_token)?,
                enc.encrypt_ref(&signing_secret)?,
            ))
        });
        let (auth_token_ref, signing_secret_ref) = match sealed {
            Ok(refs) => refs,
            Err(e) => return UseCaseResult::failure(e.into()),
        };

        // Create the service account entity
        let mut service_account = ServiceAccount::new(code, name, scope);
        service_account.description = command.description.clone();
        service_account.client_ids = client_ids;
        service_account.application_id = command.application_id.clone();
        // No application access unless the account is made for one
        // application, which it then reaches alone (Go: all_applications
        // false plus a single access row).
        service_account.all_applications = false;
        service_account.accessible_application_ids =
            command.application_id.clone().into_iter().collect();
        service_account.webhook_credentials = WebhookCredentials::bearer_token(&auth_token_ref);
        service_account.webhook_credentials.signing_secret = Some(signing_secret_ref);

        // Create domain event
        let event = ServiceAccountCreated::new(
            &ctx,
            &service_account.id,
            &service_account.code,
            &service_account.name,
            service_account.application_id.as_deref(),
            service_account.client_ids.clone(),
        );

        // Create result with one-time secrets
        let result = CreateServiceAccountResult {
            event: event.clone(),
            auth_token,
            signing_secret,
        };

        // Atomic commit through UnitOfWork. `.map()` is defined inside the
        // usecase module, so it can translate the committed event into our
        // wrapper result (which carries the one-time secrets) without
        // bypassing the seal.
        self.unit_of_work
            .commit(
                &service_account,
                &*self.service_account_repo,
                event,
                &command,
            )
            .await
            .map(|_| result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateServiceAccountCommand {
            code: "my-service".to_string(),
            name: "My Service Account".to_string(),
            description: Some("Handles order processing".to_string()),
            scope: Some(UserScope::Client),
            client_ids: vec!["client-123".to_string()],
            application_id: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("my-service"));
        assert!(json.contains("My Service Account"));
        assert!(json.contains(r#""scope":"CLIENT""#));
    }

    #[test]
    fn test_service_account_has_id() {
        let sa = ServiceAccount::new("test", "Test", UserScope::Client);
        assert!(!sa.id().is_empty());
    }

    #[test]
    fn test_generate_auth_token() {
        let token = generate_auth_token();
        assert!(token.starts_with("fc_"));
        assert_eq!(token.len(), 35); // "fc_" + 32 chars
    }

    #[test]
    fn test_generate_signing_secret() {
        let secret = generate_signing_secret();
        assert!(!secret.is_empty());
        // URL-safe base64 of 32 bytes should be ~43 chars
        assert!(secret.len() > 40);
    }
}

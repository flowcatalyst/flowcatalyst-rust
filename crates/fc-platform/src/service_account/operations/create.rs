//! Create Service Account Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use rand::Rng;

use crate::{ServiceAccount, WebhookCredentials};
use crate::ServiceAccountRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ServiceAccountCreated;

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
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
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

    /// Client IDs this account can access
    #[serde(default)]
    pub client_ids: Vec<String>,

    /// Application ID (if created for an application)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
}

/// Result returned from create service account use case.
/// Contains the event plus one-time secrets that need to be returned to caller.
#[derive(Serialize)]
pub struct CreateServiceAccountResult {
    #[serde(flatten)]
    pub event: ServiceAccountCreated,
    pub auth_token: String,
    pub signing_secret: String,
}

impl crate::usecase::DomainEvent for CreateServiceAccountResult {
    fn event_id(&self) -> &str { self.event.event_id() }
    fn event_type(&self) -> &str { self.event.event_type() }
    fn spec_version(&self) -> &str { self.event.spec_version() }
    fn source(&self) -> &str { self.event.source() }
    fn subject(&self) -> &str { self.event.subject() }
    fn time(&self) -> chrono::DateTime<chrono::Utc> { self.event.time() }
    fn execution_id(&self) -> &str { self.event.execution_id() }
    fn correlation_id(&self) -> &str { self.event.correlation_id() }
    fn causation_id(&self) -> Option<&str> { self.event.causation_id() }
    fn principal_id(&self) -> &str { self.event.principal_id() }
    fn message_group(&self) -> &str { self.event.message_group() }
    fn to_data_json(&self) -> String { self.event.to_data_json() }
}

/// Use case for creating a new service account.
pub struct CreateServiceAccountUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateServiceAccountUseCase<U> {
    pub fn new(
        service_account_repo: Arc<ServiceAccountRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            service_account_repo,
            unit_of_work,
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

        Ok(())
    }

    async fn authorize(&self, _command: &CreateServiceAccountCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
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
        let existing = self.service_account_repo.find_by_code(code).await;
        if let Ok(Some(_)) = existing {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "SERVICE_ACCOUNT_CODE_EXISTS",
                format!("A service account with code '{}' already exists", code),
            ));
        }

        // Generate credentials
        let auth_token = generate_auth_token();
        let signing_secret = generate_signing_secret();

        // Create the service account entity
        let mut service_account = ServiceAccount::new(code, name);
        service_account.description = command.description.clone();
        service_account.client_ids = command.client_ids.clone();
        service_account.application_id = command.application_id.clone();
        service_account.webhook_credentials = WebhookCredentials::bearer_token(&auth_token);
        service_account.webhook_credentials.signing_secret = Some(signing_secret.clone());

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
            .commit(&service_account, event, &command)
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
            client_ids: vec!["client-123".to_string()],
            application_id: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("my-service"));
        assert!(json.contains("My Service Account"));
    }

    #[test]
    fn test_service_account_has_id() {
        let sa = ServiceAccount::new("test", "Test");
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

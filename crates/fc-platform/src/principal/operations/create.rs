//! Create User Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use regex::Regex;

use crate::auth::password_service::PasswordService;
use crate::principal::entity::{Principal, UserScope};
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use crate::details;
use super::events::UserCreated;

/// Email validation pattern
fn email_pattern() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap()
    })
}

/// Command for creating a new user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserCommand {
    /// User's email address (required, must be valid format)
    pub email: String,

    /// Display name (optional, derived from email if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// User scope (ANCHOR, PARTNER, CLIENT). The admin handler resolves this
    /// from the email domain (anchor domain / email-domain-mapping) before
    /// calling the use case.
    pub scope: UserScope,

    /// Home client ID. For CLIENT scope, typically the user's single client.
    /// For PARTNER, the primary client the grants attach to (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Client IDs to grant access to. Persisted as `iam_client_access_grants`
    /// rows atomically with the principal insert (via `pg_persist`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub granted_client_ids: Vec<String>,

    /// Initial password (optional, for embedded auth). Hashed by the use case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// When `false`, skip the platform's password complexity rules (uppercase/
    /// lowercase/digit/special) and enforce only a 2-character minimum. Used
    /// by SDK callers that apply their own policy. Defaults to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_password_complexity: Option<bool>,
}


/// Use case for creating a new user.
pub struct CreateUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    password_service: Arc<PasswordService>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateUserUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        password_service: Arc<PasswordService>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            principal_repo,
            password_service,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateUserUseCase<U> {
    type Command = CreateUserCommand;
    type Event = UserCreated;

    async fn validate(&self, command: &CreateUserCommand) -> Result<(), UseCaseError> {
        let email = command.email.trim().to_lowercase();
        if email.is_empty() {
            return Err(UseCaseError::validation(
                "EMAIL_REQUIRED",
                "Email address is required",
            ));
        }
        if !email_pattern().is_match(&email) {
            return Err(UseCaseError::validation_with_details(
                "INVALID_EMAIL_FORMAT",
                "Invalid email address format",
                details! { "email" => &command.email },
            ));
        }

        if command.scope == UserScope::Client && command.client_id.is_none() {
            return Err(UseCaseError::validation_with_details(
                "CLIENT_ID_REQUIRED",
                "Client ID is required for CLIENT scope users",
                details! { "scope" => "CLIENT" },
            ));
        }

        Ok(())
    }

    async fn authorize(&self, _command: &CreateUserCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserCreated> {
        let email = command.email.trim().to_lowercase();

        // Business rule: email must be unique
        if let Ok(Some(_)) = self.principal_repo.find_by_email(&email).await {
            return UseCaseResult::failure(UseCaseError::business_rule_with_details(
                "EMAIL_EXISTS",
                format!("A user with email '{}' already exists", email),
                details! { "email" => &email },
            ));
        }

        // Create the principal entity
        let mut principal = Principal::new_user(&email, command.scope);

        // Set name if provided
        if let Some(ref name) = command.name {
            let name = name.trim();
            if !name.is_empty() {
                principal.name = name.to_string();
            }
        }

        // Set home client_id if provided
        if let Some(ref client_id) = command.client_id {
            principal = principal.with_client_id(client_id);
        }

        // Grants — persisted atomically via `pg_persist` (syncs
        // `iam_client_access_grants` in the same transaction as the principal
        // insert).
        for cid in &command.granted_client_ids {
            principal.grant_client_access(cid.clone());
        }

        // Hash + set password if provided (internal-auth users).
        if let Some(ref password) = command.password {
            let enforce = command.enforce_password_complexity.unwrap_or(true);
            let hash = match self
                .password_service
                .hash_password_with_complexity(password, enforce)
            {
                Ok(h) => h,
                Err(e) => {
                    return UseCaseResult::failure(UseCaseError::validation(
                        "INVALID_PASSWORD",
                        e.to_string(),
                    ));
                }
            };
            if let Some(identity) = principal.user_identity.as_mut() {
                identity.password_hash = Some(hash);
            }
        }

        let is_anchor_user = command.scope == UserScope::Anchor;

        // Create domain event using builder pattern
        let event = UserCreated::builder()
            .from(&ctx)
            .principal_id(&principal.id)
            .email(&email)
            .name(&principal.name)
            .scope(command.scope)
            .client_id(principal.client_id.as_deref())
            .is_anchor_user(is_anchor_user)
            .build();

        // Atomic commit — principal + event + audit log, in one transaction.
        self.unit_of_work.commit(&principal, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateUserCommand {
            email: "user@example.com".to_string(),
            name: Some("Test User".to_string()),
            scope: UserScope::Client,
            client_id: Some("client-123".to_string()),
            granted_client_ids: vec![],
            password: None,
            enforce_password_complexity: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("user@example.com"));
        assert!(json.contains("CLIENT"));
    }

    #[test]
    fn test_email_pattern() {
        assert!(email_pattern().is_match("user@example.com"));
        assert!(email_pattern().is_match("user.name@example.co.uk"));
        assert!(email_pattern().is_match("user+tag@example.com"));
        assert!(!email_pattern().is_match("invalid"));
        assert!(!email_pattern().is_match("@example.com"));
        assert!(!email_pattern().is_match("user@"));
    }

    #[test]
    fn test_principal_has_id() {
        let principal = Principal::new_user("test@example.com", UserScope::Anchor);
        assert!(!principal.id().is_empty());
    }
}

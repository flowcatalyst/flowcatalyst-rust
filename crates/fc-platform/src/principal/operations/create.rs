//! Create User Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use regex::Regex;

use crate::principal::entity::{Principal, UserScope};
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
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

    /// User scope (ANCHOR, PARTNER, CLIENT)
    pub scope: UserScope,

    /// Client ID (required for CLIENT scope users)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Initial password (optional, for embedded auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}


/// Use case for creating a new user.
pub struct CreateUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateUserUseCase<U> {
    pub fn new(principal_repo: Arc<PrincipalRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            principal_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: CreateUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserCreated> {
        // Validation: email is required and must be valid
        let email = command.email.trim().to_lowercase();
        if email.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "EMAIL_REQUIRED",
                "Email address is required",
            ));
        }
        if !email_pattern().is_match(&email) {
            return UseCaseResult::failure(UseCaseError::validation_with_details(
                "INVALID_EMAIL_FORMAT",
                "Invalid email address format",
                details! { "email" => &command.email },
            ));
        }

        // Validation: CLIENT scope requires client_id
        if command.scope == UserScope::Client && command.client_id.is_none() {
            return UseCaseResult::failure(UseCaseError::validation_with_details(
                "CLIENT_ID_REQUIRED",
                "Client ID is required for CLIENT scope users",
                details! { "scope" => "CLIENT" },
            ));
        }

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

        // Set client_id if provided
        if let Some(ref client_id) = command.client_id {
            principal = principal.with_client_id(client_id);
        }

        // Set password if provided
        if let Some(ref password) = command.password {
            if let Some(ref mut identity) = principal.user_identity {
                // In production, this would be hashed
                identity.password_hash = Some(password.clone());
            }
        }

        // Create domain event using builder pattern
        let event = UserCreated::builder()
            .from(&ctx)
            .principal_id(&principal.id)
            .email(&email)
            .name(&principal.name)
            .scope(command.scope)
            .client_id(command.client_id.as_deref())
            .is_anchor_user(false)
            .build();

        // Atomic commit
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
            password: None,
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

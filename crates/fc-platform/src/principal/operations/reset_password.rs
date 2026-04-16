//! Reset Password Use Case
//!
//! Admin-initiated password reset for internal-auth users. Used from the user
//! detail page when a user needs a new password and email-based reset isn't an
//! option. Hashes the new password with the configured complexity policy (or a
//! relaxed policy when the caller opts out) and commits atomically through
//! `UnitOfWork` so events and audit logs are emitted.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::password_service::PasswordService;
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::PasswordResetCompleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordCommand {
    pub principal_id: String,
    pub new_password: String,
    /// When `false`, skip the platform's complexity rules (uppercase/lowercase/
    /// digit/special) and enforce only a 2-character minimum. Defaults to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_password_complexity: Option<bool>,
}

pub struct ResetPasswordUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    password_service: Arc<PasswordService>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ResetPasswordUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        password_service: Arc<PasswordService>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { principal_repo, password_service, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ResetPasswordUseCase<U> {
    type Command = ResetPasswordCommand;
    type Event = PasswordResetCompleted;

    async fn validate(&self, command: &ResetPasswordCommand) -> Result<(), UseCaseError> {
        if command.principal_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPAL_ID_REQUIRED",
                "Principal ID is required",
            ));
        }
        if command.new_password.is_empty() {
            return Err(UseCaseError::validation(
                "NEW_PASSWORD_REQUIRED",
                "New password is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &ResetPasswordCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // The handler gates this with `require_anchor`. No additional
        // resource-level check is needed here.
        Ok(())
    }

    async fn execute(
        &self,
        command: ResetPasswordCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PasswordResetCompleted> {
        // Load the principal.
        let mut principal = match self.principal_repo.find_by_id(&command.principal_id).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "PRINCIPAL_NOT_FOUND",
                    format!("Principal with ID '{}' not found", command.principal_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch principal: {}", e
                )));
            }
        };

        if !principal.is_user() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "NOT_A_USER",
                "Password reset only applies to user principals",
            ));
        }

        // OIDC-backed users don't have a local password to reset.
        if principal.external_identity.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "OIDC_USER",
                "Cannot reset password for OIDC-authenticated users",
            ));
        }

        // Hash the new password, honouring the complexity flag.
        let enforce = command.enforce_password_complexity.unwrap_or(true);
        let hash = match self
            .password_service
            .hash_password_with_complexity(&command.new_password, enforce)
        {
            Ok(h) => h,
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_PASSWORD",
                    e.to_string(),
                ));
            }
        };

        // Capture the email for the event before we mutate the identity.
        let email = principal
            .user_identity
            .as_ref()
            .map(|i| i.email.clone())
            .unwrap_or_default();

        if let Some(identity) = principal.user_identity.as_mut() {
            identity.password_hash = Some(hash);
        }
        principal.updated_at = chrono::Utc::now();

        let event = PasswordResetCompleted::from_ctx(&ctx, &principal.id, &email);

        self.unit_of_work.commit(&principal, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serialization() {
        let cmd = ResetPasswordCommand {
            principal_id: "user-1".to_string(),
            new_password: "hunter22!".to_string(),
            enforce_password_complexity: Some(false),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
        assert!(json.contains("newPassword"));
        assert!(json.contains("enforcePasswordComplexity"));
    }
}

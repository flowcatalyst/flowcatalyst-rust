//! Reset Password Use Case
//!
//! Admin-initiated password reset for internal-auth users. Used from the user
//! detail page when a user needs a new password and email-based reset isn't an
//! option. Hashes the new password with the configured complexity policy (or a
//! relaxed policy when the caller opts out) and commits atomically through
//! `UnitOfWork` so events and audit logs are emitted.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PasswordResetCompleted;
use crate::auth::password_service::PasswordService;
use crate::principal::entity::Principal;
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordCommand {
    pub principal_id: String,
    /// Never serialised: the UnitOfWork persists the command into
    /// `aud_logs.operation_json`, and a plaintext password must not land there.
    #[serde(skip_serializing)]
    pub new_password: String,
    /// When `false`, skip the platform's complexity rules (uppercase/lowercase/
    /// digit/special) and enforce only a 2-character minimum. Defaults to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_password_complexity: Option<bool>,
}

impl crate::usecase::AuditMasked for ResetPasswordCommand {}

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
        Self {
            principal_repo,
            password_service,
            unit_of_work,
        }
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
        let (principal, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> ResetPasswordUseCase<U> {
    async fn prepare(
        &self,
        command: &ResetPasswordCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Principal, PasswordResetCompleted), UseCaseError> {
        // Load the principal.
        let mut principal = self
            .principal_repo
            .find_by_id(&command.principal_id)
            .await
            .or_not_found(
                "PRINCIPAL_NOT_FOUND",
                format!("Principal with ID '{}' not found", command.principal_id),
            )?;

        if !principal.is_user() {
            return Err(UseCaseError::business_rule(
                "NOT_A_USER",
                "Password reset only applies to user principals",
            ));
        }

        // OIDC-backed users don't have a local password to reset.
        if principal.external_identity.is_some() {
            return Err(UseCaseError::business_rule(
                "OIDC_USER",
                "Cannot reset password for OIDC-authenticated users",
            ));
        }

        // Hash the new password, honouring the complexity flag.
        let enforce = command.enforce_password_complexity.unwrap_or(true);
        let hash = self
            .password_service
            .hash_password_with_complexity(&command.new_password, enforce)
            .map_err(|e| UseCaseError::validation("INVALID_PASSWORD", e.to_string()))?;

        if let Some(identity) = principal.user_identity.as_mut() {
            identity.password_hash = Some(hash);
        }
        principal.updated_at = chrono::Utc::now();

        let event = PasswordResetCompleted::from_ctx(ctx, &principal.id);
        Ok((principal, event))
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
        assert!(json.contains("enforcePasswordComplexity"));
        // The command is persisted into aud_logs.operation_json by the UoW.
        assert!(!json.contains("newPassword"), "password leaked: {json}");
        assert!(!json.contains("hunter22!"), "password leaked: {json}");
    }

    #[test]
    fn command_still_deserializes_the_password() {
        let cmd: ResetPasswordCommand =
            serde_json::from_str(r#"{"principalId":"user-1","newPassword":"hunter22!"}"#).unwrap();
        assert_eq!(cmd.new_password, "hunter22!");
    }
}

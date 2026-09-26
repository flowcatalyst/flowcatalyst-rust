//! Update User Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::UserUpdated;
use crate::principal::entity::{Principal, UserScope};
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for updating an existing user / principal.
///
/// Covers every mutable field the API layer exposes. Fields that weren't
/// sent stay `None`; only `Some(_)` values are applied. As Go's `UpdateUser`
/// (principal/operations/update.go), an update that changes nothing still
/// saves and records `UserUpdated`: the SPA sends the name on every save,
/// then changes the tier or client through `/client-association`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserCommand {
    /// Principal ID to update
    pub principal_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,

    /// `ANCHOR` / `PARTNER` / `CLIENT`. Requires caller be anchor — the
    /// handler checks this before building the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<UserScope>,

    /// Home client ID. Required when scope becomes `CLIENT`; ignored for
    /// other scopes (the principal's `client_id` is nulled out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Asserted against the stored email, never applied: a different value
    /// is refused with `EMAIL_IMMUTABLE` (Go `UpdateCommand.Email`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl crate::usecase::AuditMasked for UpdateUserCommand {}

/// Use case for updating an existing user.
pub struct UpdateUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateUserUseCase<U> {
    pub fn new(principal_repo: Arc<PrincipalRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            principal_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateUserUseCase<U> {
    type Command = UpdateUserCommand;
    type Event = UserUpdated;

    async fn validate(&self, command: &UpdateUserCommand) -> Result<(), UseCaseError> {
        if command.principal_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPAL_ID_REQUIRED",
                "Principal ID is required",
            ));
        }

        // Go `UpdateUser.Validate`: a name, when sent, is not blank. An
        // empty body is not refused; it saves and records the event.
        if command.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name cannot be empty",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateUserCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserUpdated> {
        let (principal, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateUserUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateUserCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Principal, UserUpdated), UseCaseError> {
        // Fetch existing principal
        let mut principal = self
            .principal_repo
            .find_by_id(&command.principal_id)
            .await
            .or_not_found(
                "PRINCIPAL_NOT_FOUND",
                format!("User with ID '{}' not found", command.principal_id),
            )?;

        // Email is the principal's identity: accepted so a caller can PUT a
        // whole object, but only as an assertion (Go `UpdateUser`).
        if let Some(ref email) = command.email {
            let got = email.trim().to_lowercase();
            let current = principal
                .user_identity
                .as_ref()
                .map(|i| i.email.trim().to_lowercase())
                .unwrap_or_default();
            if !got.is_empty() && got != current {
                return Err(UseCaseError::validation(
                    "EMAIL_IMMUTABLE",
                    "email cannot be changed here; it is the principal's identity",
                ));
            }
        }

        // Apply what was sent. Nothing changing is not an error: Go saves
        // and records `UserUpdated` all the same.
        if let Some(ref name) = command.name {
            principal.name = name.trim().to_string();
        }

        if let Some(active) = command.active {
            if active != principal.active {
                if active {
                    principal.activate();
                } else {
                    principal.deactivate();
                }
            }
        }

        // Scope change (+ consequent client_id rules).
        let new_scope = command.scope;

        if let Some(scope) = new_scope {
            principal.scope = scope;
        }

        if command.client_id.is_some() || new_scope.is_some() {
            match principal.scope {
                UserScope::Client => {
                    let cid = command
                        .client_id
                        .clone()
                        .or_else(|| principal.client_id.clone())
                        .ok_or_else(|| {
                            UseCaseError::validation(
                                "CLIENT_ID_REQUIRED",
                                "client_id is required when scope is CLIENT",
                            )
                        })?;
                    if cid.trim().is_empty() {
                        return Err(UseCaseError::validation(
                            "CLIENT_ID_REQUIRED",
                            "client_id cannot be empty when scope is CLIENT",
                        ));
                    }
                    principal.client_id = Some(cid);
                }
                _ => {
                    principal.client_id = None;
                }
            }
        }

        // first_name / last_name only apply to USER-type principals.
        if principal.is_user() {
            if let Some(ref mut identity) = principal.user_identity {
                if let Some(first) = command.first_name.clone() {
                    identity.first_name = Some(first);
                }
                if let Some(last) = command.last_name.clone() {
                    identity.last_name = Some(last);
                }
            }
        }

        principal.updated_at = chrono::Utc::now();

        let event = UserUpdated::new(ctx, &principal.id, &principal.name);
        Ok((principal, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateUserCommand {
            principal_id: "user-123".to_string(),
            name: Some("New Name".to_string()),
            first_name: None,
            last_name: None,
            active: None,
            scope: None,
            client_id: None,
            email: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
        assert!(json.contains("New Name"));
    }
}

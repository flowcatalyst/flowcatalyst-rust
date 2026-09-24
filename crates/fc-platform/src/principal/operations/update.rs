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
/// sent stay `None`; only `Some(_)` values are applied.
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

        let has_any = command.name.is_some()
            || command.first_name.is_some()
            || command.last_name.is_some()
            || command.active.is_some()
            || command.scope.is_some()
            || command.client_id.is_some();
        if !has_any {
            return Err(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
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
                "USER_NOT_FOUND",
                format!("User with ID '{}' not found", command.principal_id),
            )?;

        // Apply updates — track whether anything actually changed.
        let mut changed = false;
        let mut new_name: Option<String> = None;

        if let Some(ref name) = command.name {
            let trimmed = name.trim().to_string();
            if trimmed != principal.name {
                principal.name = trimmed.clone();
                new_name = Some(trimmed);
                changed = true;
            }
        }

        if let Some(active) = command.active {
            if active != principal.active {
                if active {
                    principal.activate();
                } else {
                    principal.deactivate();
                }
                changed = true;
            }
        }

        // Scope change (+ consequent client_id rules).
        let new_scope = command.scope;

        if let Some(scope) = new_scope {
            if scope != principal.scope {
                principal.scope = scope;
                changed = true;
            }
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
                    if principal.client_id.as_deref() != Some(cid.as_str()) {
                        principal.client_id = Some(cid);
                        changed = true;
                    }
                }
                _ => {
                    if principal.client_id.is_some() {
                        principal.client_id = None;
                        changed = true;
                    }
                }
            }
        }

        // first_name / last_name only apply to USER-type principals.
        if principal.is_user() {
            if let Some(ref mut identity) = principal.user_identity {
                if let Some(first) = command.first_name.clone() {
                    if identity.first_name.as_deref() != Some(first.as_str()) {
                        identity.first_name = Some(first);
                        changed = true;
                    }
                }
                if let Some(last) = command.last_name.clone() {
                    if identity.last_name.as_deref() != Some(last.as_str()) {
                        identity.last_name = Some(last);
                        changed = true;
                    }
                }
            }
        }

        if !changed {
            return Err(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        principal.updated_at = chrono::Utc::now();

        let event = UserUpdated::new(ctx, &principal.id, new_name.as_deref(), None);
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
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
        assert!(json.contains("New Name"));
    }
}

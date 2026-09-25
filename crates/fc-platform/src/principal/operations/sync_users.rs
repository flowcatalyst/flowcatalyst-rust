//! Sync Users Use Case — the platform-level `POST /api/principals/sync`.
//!
//! Go's `SyncPrincipals` with no application
//! (flowcatalyst-go internal/platform/principal/operations/sync_principals.go:
//! 68-246, called from principal/api/sync.go:43-76): a declarative upsert of
//! users by email, carrying a migrated password hash verbatim.
//!
//! For each entry, the email lower-cased:
//! - an existing user keeps its non-`SDK_SYNC` roles, takes the entry's roles
//!   as its `SDK_SYNC` set, and takes the entry's name and active flag. Its
//!   stored password hash is never touched: a `passwordHash` on the entry is
//!   ignored, whoever the caller (owner decision #22, ruling 4; Java
//!   f4cd1bc2), logged by principal id and reported by the route
//!   ([`password_hashes_ignored`]). One `platform:iam:user:updated` event.
//! - a new user is created CLIENT-tier with no home client, with the entry's
//!   name, active flag, roles and hash. One `platform:iam:user:created` event.
//!
//! Roles are lower-cased and not prefixed. The role ceiling (owner ruling
//! 14) bounds them: the caller may add or remove only roles whose every
//! platform permission it holds, else 403 `ROLE_ABOVE_CALLER` and nothing is
//! written. Nothing is removed for unlisted users. A `platform:iam:principals:synced` rollup
//! closes the sync. Run inside [`crate::usecase::PgUnitOfWork::run`], the
//! rows, every event and every audit entry commit in one transaction, as
//! Go's `CommitSync` does.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::events::{PrincipalsSynced, UserCreated, UserUpdated};
use crate::principal::entity::{Principal, PrincipalSyncBatch, UserScope};
use crate::role::ceiling;
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use crate::shared::authorization_service::AuthContext;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::{PrincipalRepository, RoleRepository};

/// One user in a platform-level sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncUserInput {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub roles: Vec<String>,
    /// Defaults to active (Go sync.go:51-54).
    #[serde(default = "default_active")]
    pub active: bool,
    /// A password hash to store verbatim (e.g. Laravel's bcrypt `$2y$…`);
    /// login verifies it and re-encodes it to argon2id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

fn default_active() -> bool {
    true
}

/// Command for a platform-level user sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncUsersCommand {
    pub principals: Vec<SyncUserInput>,
}

/// The hashes ride `principals[].passwordHash`, which the audit name rule
/// masks (`passwordhash` suffix, fc-common audit_redaction.rs); there is no
/// top-level field to declare.
impl crate::usecase::AuditMasked for SyncUsersCommand {}

pub struct SyncUsersUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    role_repo: Arc<RoleRepository>,
    /// Who runs the sync, for the role ceiling.
    caller: AuthContext,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncUsersUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        role_repo: Arc<RoleRepository>,
        caller: AuthContext,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            principal_repo,
            role_repo,
            caller,
            unit_of_work,
        }
    }
}

/// What one sync entry did to one user.
enum RowEvent {
    Created(UserCreated),
    Updated(UserUpdated),
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncUsersUseCase<U> {
    type Command = SyncUsersCommand;
    type Event = PrincipalsSynced;

    async fn validate(&self, command: &SyncUsersCommand) -> Result<(), UseCaseError> {
        if command.principals.is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPALS_REQUIRED",
                "At least one principal must be provided",
            ));
        }
        Ok(())
    }

    /// Go's authorize applies only to an unlisted-user sweep, which this
    /// sync never does; the handler's `can_sync_principals` is the gate.
    async fn authorize(
        &self,
        _command: &SyncUsersCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncUsersCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PrincipalsSynced> {
        let (batch, row_events, rollup) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        for event in row_events {
            let emitted = match event {
                RowEvent::Created(e) => self.unit_of_work.emit_event(e, &command).await.map(|_| ()),
                RowEvent::Updated(e) => self.unit_of_work.emit_event(e, &command).await.map(|_| ()),
            };
            if let Err(e) = emitted.into_result() {
                return UseCaseResult::failure(e);
            }
        }

        self.unit_of_work
            .commit(&batch, &*self.principal_repo, rollup, &command)
            .await
    }
}

impl<U: UnitOfWork> SyncUsersUseCase<U> {
    async fn prepare(
        &self,
        command: &SyncUsersCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PrincipalSyncBatch, Vec<RowEvent>, PrincipalsSynced), UseCaseError> {
        let now = Utc::now();
        let emails: Vec<String> = command
            .principals
            .iter()
            .map(|p| p.email.to_lowercase())
            .collect();

        // One lookup for every listed email.
        let mut existing: HashMap<String, Principal> = self
            .principal_repo
            .find_users_by_emails(&emails)
            .await
            .map_err(|e| UseCaseError::commit(format!("Failed to load users: {e}")))?
            .into_iter()
            .map(|p| (p.email().unwrap_or_default().to_lowercase(), p))
            .collect();

        // What each listed user holds now, for the role ceiling.
        let stored_roles: HashMap<String, Vec<String>> = existing
            .iter()
            .map(|(email, p)| {
                (
                    email.clone(),
                    p.roles.iter().map(|r| r.role.clone()).collect(),
                )
            })
            .collect();

        // Keyed by email so a repeated entry updates the user the earlier one
        // created or loaded, in order.
        let mut order: Vec<String> = Vec::new();
        let mut saved: HashMap<String, Principal> = HashMap::new();
        let mut row_events = Vec::with_capacity(command.principals.len());
        let (mut created, mut updated) = (0u32, 0u32);

        for (input, email) in command.principals.iter().zip(&emails) {
            let roles: Vec<RoleAssignment> = input
                .roles
                .iter()
                .map(|r| RoleAssignment {
                    role: r.to_lowercase(),
                    client_id: None,
                    assignment_source: Some(AssignmentSource::SdkSync),
                    assigned_at: now,
                    assigned_by: None,
                })
                .collect();
            let hash = input.password_hash.as_deref().filter(|h| !h.is_empty());

            let current = saved.remove(email).or_else(|| existing.remove(email));
            let principal = match current {
                Some(mut p) => {
                    p.roles
                        .retain(|ra| ra.assignment_source != Some(AssignmentSource::SdkSync));
                    p.roles.extend(roles);
                    p.name = input.name.clone();
                    p.active = input.active;
                    p.updated_at = now;
                    if hash.is_some() {
                        // Decision #22: a hash is used only to create. Password
                        // changes go through the explicit, audited routes.
                        tracing::info!(
                            principal_id = %p.id,
                            "principal sync: passwordHash ignored for an existing principal"
                        );
                    }
                    row_events.push(RowEvent::Updated(UserUpdated::new(
                        ctx,
                        &p.id,
                        Some(&p.name),
                        None,
                    )));
                    updated += 1;
                    p
                }
                None => {
                    let mut p = Principal::new_user(email.as_str(), UserScope::Client);
                    p.name = input.name.clone();
                    p.active = input.active;
                    p.roles = roles;
                    if let (Some(hash), Some(identity)) = (hash, p.user_identity.as_mut()) {
                        identity.password_hash = Some(hash.to_string());
                    }
                    row_events.push(RowEvent::Created(UserCreated::new(
                        ctx, &p.id, email, &p.name, p.scope, None,
                    )));
                    created += 1;
                    p
                }
            };
            if !order.contains(email) {
                order.push(email.clone());
            }
            saved.insert(email.clone(), principal);
        }

        // Owner ruling 14: every role the sync adds or removes, on any user,
        // must be within the caller's own permissions.
        let mut changed: Vec<String> = Vec::new();
        for email in &order {
            let before = stored_roles.get(email).cloned().unwrap_or_default();
            let after: Vec<String> = saved[email].roles.iter().map(|r| r.role.clone()).collect();
            for role in ceiling::changed(&before, &after) {
                if !changed.contains(&role) {
                    changed.push(role);
                }
            }
        }
        let definitions = ceiling::definitions(&self.role_repo, &changed).await?;
        ceiling::require_roles(Some(&self.caller), &changed, &definitions)?;

        let batch = PrincipalSyncBatch {
            principals: order.iter().filter_map(|e| saved.remove(e)).collect(),
        };
        let rollup = PrincipalsSynced {
            metadata: PrincipalsSynced::metadata_for_platform(ctx),
            application_code: String::new(),
            created,
            updated,
            deactivated: 0,
            synced_emails: emails,
        };
        Ok((batch, row_events, rollup))
    }
}

/// The emails, lower-cased and without repeats, whose entry carries a
/// `passwordHash` for a user that already exists, so the sync will not apply
/// it (decision #22). Read before the sync runs, for the caller's response
/// (Java `SyncPrincipals.passwordHashesIgnored`). One query.
pub async fn password_hashes_ignored<'a>(
    principal_repo: &PrincipalRepository,
    entries: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> crate::shared::error::Result<Vec<String>> {
    let mut with_hash: Vec<String> = Vec::new();
    for (email, hash) in entries {
        let email = email.to_lowercase();
        if hash.is_some_and(|h| !h.is_empty()) && !with_hash.contains(&email) {
            with_hash.push(email);
        }
    }
    if with_hash.is_empty() {
        return Ok(with_hash);
    }
    let existing: Vec<String> = principal_repo
        .find_users_by_emails(&with_hash)
        .await?
        .iter()
        .filter_map(|p| p.email().map(str::to_lowercase))
        .collect();
    with_hash.retain(|e| existing.contains(e));
    Ok(with_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// integral `SyncUsersToFlowCatalystCommand.php:589-599` through the
    /// Laravel SDK's `SyncPrincipalEntry::toArray()`: `active` omitted,
    /// `roles` empty, the hash verbatim.
    #[test]
    fn integrals_entry_deserializes_with_go_defaults() {
        let req: SyncUsersCommand = serde_json::from_value(serde_json::json!({
            "principals": [{
                "email": "Jo@Inhance.test",
                "name": "Jo",
                "roles": [],
                "passwordHash": "$2y$10$abcdefghijklmnopqrstuuJ0nZlqSxW1lQxXcI0oJ9Q1Hc2r8m3a6"
            }]
        }))
        .unwrap();
        let p = &req.principals[0];
        assert!(p.active);
        assert!(p.password_hash.as_deref().unwrap().starts_with("$2y$"));
    }

    #[test]
    fn the_audit_copy_masks_the_hashes() {
        let command = SyncUsersCommand {
            principals: vec![SyncUserInput {
                email: "jo@inhance.test".to_string(),
                name: "Jo".to_string(),
                roles: vec![],
                active: true,
                password_hash: Some("$2y$10$secret".to_string()),
            }],
        };
        let json = fc_common::audit_redaction::redacted_command_json(&command).unwrap();
        assert_eq!(json["principals"][0]["passwordHash"], "***");
        assert_eq!(json["principals"][0]["email"], "jo@inhance.test");
    }
}

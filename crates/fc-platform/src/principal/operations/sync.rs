//! Sync Principals Use Case — the application-scoped
//! `POST /api/applications/{appCode}/principals/sync`.
//!
//! Go's `SyncPrincipals` with an application
//! (flowcatalyst-go internal/platform/principal/operations/sync_principals.go:
//! 68-246, called from sdksync/api.go:500-540), with the owner's rulings of
//! 2026-09-25 on top. For each entry, the email lower-cased:
//! - an existing user keeps every role except this application's `SDK_SYNC`
//!   ones (`{appCode}:` prefixed), which the entry's roles replace; another
//!   application's `SDK_SYNC` roles survive (Java 037e6f56). Name and active
//!   flag are the entry's; a `passwordHash` is never applied (decision #22).
//!   One `platform:iam:user:updated` event.
//! - a new user is created CLIENT-tier with no home client, with the entry's
//!   name, active flag, roles and password hash. One
//!   `platform:iam:user:created` event.
//!
//! `removeUnlisted` never deletes: it strips this application's `SDK_SYNC`
//! roles from every user absent from the payload (counted as "deactivated").
//!
//! The caller touches only principals within its reach (Java S1.3,
//! decision #23): a listed user it could not administer refuses the whole
//! sync with 403 `SYNC_TARGET_FORBIDDEN`; the sweep skips such users. Role
//! names prefixed `platform:` or with another application's code are
//! refused with 403 `ROLE_APP_FORBIDDEN` (decision #23); other names are
//! accepted as before. A `platform:iam:principals:synced` rollup closes the
//! sync. Run inside [`crate::usecase::PgUnitOfWork::run`], the rows, every
//! event and every audit entry commit in one transaction.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::events::{PrincipalsSynced, UserCreated, UserUpdated};
use crate::principal::entity::{Principal, PrincipalSyncBatch, UserScope};
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use crate::shared::authorization_service::AuthContext;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationRepository;
use crate::PrincipalRepository;

/// A single principal definition in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrincipalInput {
    /// User's email address (unique identifier for matching)
    pub email: String,
    /// Display name
    pub name: String,
    /// Role names to assign (the SDK prefixes them with the applicationCode)
    #[serde(default)]
    pub roles: Vec<String>,
    /// Whether the user is active (default: true)
    #[serde(default = "default_active")]
    pub active: bool,
    /// A password hash (e.g. Laravel's bcrypt `$2y$`) stored verbatim on a
    /// user this sync creates; login verifies it and re-encodes it. Never
    /// applied to an existing user (decision #22). Masked in the audit row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

fn default_active() -> bool {
    true
}

/// Command for syncing principals from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrincipalsCommand {
    pub application_code: String,
    pub principals: Vec<SyncPrincipalInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
}

/// The hashes ride `principals[].passwordHash`, which the audit name rule
/// masks.
impl crate::usecase::AuditMasked for SyncPrincipalsCommand {}

/// Whether `caller` may administer `target` (Java `Access.administers`,
/// Go `blockNonClientTarget` + `CanAccessScope`): an anchor reaches every
/// principal; anyone else only CLIENT-tier principals of a client it can
/// access, and a client-less one only as a super-admin.
pub fn administers(caller: &AuthContext, target: &Principal) -> bool {
    if caller.is_anchor() {
        return true;
    }
    if target.scope != UserScope::Client {
        return false;
    }
    match target.client_id.as_deref() {
        Some(client_id) => caller.can_access_client(client_id),
        None => caller.has_permission(crate::permissions::ADMIN_ALL),
    }
}

/// 403 `SYNC_TARGET_FORBIDDEN` for a listed user out of the caller's reach.
pub(crate) fn sync_target_forbidden(email: &str) -> UseCaseError {
    UseCaseError::forbidden(
        "SYNC_TARGET_FORBIDDEN",
        format!("Not authorised to manage principal '{email}'"),
    )
}

/// Whether `role` is one of this application's own SDK-synced roles, the
/// set a sync replaces and a sweep strips: `SDK_SYNC` and `{appCode}:`
/// prefixed (Go X-02(c), Java 037e6f56).
fn is_own_sdk_role(role: &RoleAssignment, application_code: &str) -> bool {
    role.has_source(AssignmentSource::SdkSync)
        && role
            .role
            .strip_prefix(application_code)
            .is_some_and(|rest| rest.starts_with(':'))
}

pub struct SyncPrincipalsUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    application_repo: Arc<ApplicationRepository>,
    /// Who runs the sync, for its reach.
    caller: AuthContext,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncPrincipalsUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        application_repo: Arc<ApplicationRepository>,
        caller: AuthContext,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            principal_repo,
            application_repo,
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
impl<U: UnitOfWork> UseCase for SyncPrincipalsUseCase<U> {
    type Command = SyncPrincipalsCommand;
    type Event = PrincipalsSynced;

    async fn validate(&self, command: &SyncPrincipalsCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }

        if command.principals.is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPALS_REQUIRED",
                "At least one principal must be provided",
            ));
        }

        Ok(())
    }

    /// The per-principal reach and the role-name rule need the stored rows,
    /// so they are checked in `execute` before anything is written.
    async fn authorize(
        &self,
        _command: &SyncPrincipalsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncPrincipalsCommand,
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

impl<U: UnitOfWork> SyncPrincipalsUseCase<U> {
    /// Decision #23: refuse role names prefixed `platform:` or with another
    /// application's code. One query for the whole payload.
    async fn require_syncable_roles(
        &self,
        application_code: &str,
        role_names: &[String],
    ) -> Result<(), UseCaseError> {
        let prefixes: Vec<String> = role_names
            .iter()
            .filter_map(|r| r.split_once(':').map(|(p, _)| p.to_string()))
            .filter(|p| p != application_code)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if prefixes.is_empty() {
            return Ok(());
        }
        let other_apps = self
            .application_repo
            .find_ids_by_codes(&prefixes)
            .await
            .map_err(|e| UseCaseError::commit(format!("Failed to load applications: {e}")))?;
        for name in role_names {
            let Some((prefix, _)) = name.split_once(':') else {
                continue;
            };
            if prefix != application_code
                && (prefix == "platform" || other_apps.contains_key(prefix))
            {
                return Err(UseCaseError::forbidden(
                    "ROLE_APP_FORBIDDEN",
                    format!("role '{name}' does not belong to application '{application_code}'"),
                ));
            }
        }
        Ok(())
    }

    async fn prepare(
        &self,
        command: &SyncPrincipalsCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PrincipalSyncBatch, Vec<RowEvent>, PrincipalsSynced), UseCaseError> {
        let app_code = command.application_code.as_str();
        self.application_repo
            .find_by_code(app_code)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application not found: {}", app_code),
            )?;

        let all_roles: Vec<String> = command
            .principals
            .iter()
            .flat_map(|p| p.roles.iter().map(|r| r.to_lowercase()))
            .collect();
        self.require_syncable_roles(app_code, &all_roles).await?;

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

        // Java S1.3: a listed user out of reach refuses the whole sync.
        for (email, p) in &existing {
            if !administers(&self.caller, p) {
                return Err(sync_target_forbidden(email));
            }
        }

        // Keyed by email so a repeated entry updates the user the earlier one
        // created or loaded, in order.
        let mut order: Vec<String> = Vec::new();
        let mut saved: HashMap<String, Principal> = HashMap::new();
        let mut row_events = Vec::with_capacity(command.principals.len());
        let (mut created, mut updated, mut deactivated) = (0u32, 0u32, 0u32);

        for (input, email) in command.principals.iter().zip(&emails) {
            let mut roles: Vec<RoleAssignment> = Vec::with_capacity(input.roles.len());
            for r in &input.roles {
                let name = r.to_lowercase();
                if !roles.iter().any(|a| a.role == name) {
                    roles.push(RoleAssignment::with_source(name, AssignmentSource::SdkSync));
                }
            }
            let hash = input.password_hash.as_deref().filter(|h| !h.is_empty());

            let current = saved.remove(email).or_else(|| existing.remove(email));
            let principal = match current {
                Some(mut p) => {
                    // Only this application's SDK roles are replaced.
                    p.roles.retain(|ra| !is_own_sdk_role(ra, app_code));
                    for ra in roles {
                        if !p.roles.iter().any(|kept| kept.role == ra.role) {
                            p.roles.push(ra);
                        }
                    }
                    p.name = input.name.clone();
                    p.active = input.active;
                    p.updated_at = now;
                    if hash.is_some() {
                        // Decision #22: a hash is used only to create.
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
                    // Go sdksync/api.go:527: carry a migrated credential
                    // verbatim so the user keeps their password.
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

        let mut principals: Vec<Principal> = order.iter().filter_map(|e| saved.remove(e)).collect();

        // Strip this application's SDK roles from unlisted users within reach.
        if command.remove_unlisted {
            let listed: HashSet<&str> = emails.iter().map(String::as_str).collect();
            let all = self
                .principal_repo
                .find_all()
                .await
                .map_err(|e| UseCaseError::commit(format!("Failed to load principals: {e}")))?;
            for mut p in all {
                if !p.is_user() {
                    continue;
                }
                let Some(email) = p.email().map(str::to_lowercase) else {
                    continue;
                };
                if listed.contains(email.as_str()) || !administers(&self.caller, &p) {
                    continue;
                }
                if !p.roles.iter().any(|ra| is_own_sdk_role(ra, app_code)) {
                    continue;
                }
                p.roles.retain(|ra| !is_own_sdk_role(ra, app_code));
                p.updated_at = now;
                row_events.push(RowEvent::Updated(UserUpdated::new(
                    ctx,
                    &p.id,
                    Some(&p.name),
                    None,
                )));
                deactivated += 1;
                principals.push(p);
            }
        }

        let rollup = PrincipalsSynced {
            metadata: PrincipalsSynced::metadata_for(ctx, app_code),
            application_code: command.application_code.clone(),
            created,
            updated,
            deactivated,
            synced_emails: emails,
        };
        Ok((PrincipalSyncBatch { principals }, row_events, rollup))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::role::ceiling::test_caller as ctx;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncPrincipalsCommand {
            application_code: "orders".to_string(),
            principals: vec![],
            remove_unlisted: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }

    #[test]
    fn reach_follows_tier_and_client() {
        let anchor = ctx(UserScope::Anchor, &["*"], &[]);
        let client = ctx(UserScope::Client, &["clt_a"], &[]);
        let own = Principal::new_user("a@x.test", UserScope::Client).with_client_id("clt_a");
        let other = Principal::new_user("b@x.test", UserScope::Client).with_client_id("clt_b");
        let partner = Principal::new_user("c@x.test", UserScope::Partner).with_client_id("clt_a");
        let clientless = Principal::new_user("d@x.test", UserScope::Client);
        let staff = Principal::new_user("e@x.test", UserScope::Anchor);
        for p in [&own, &other, &partner, &clientless, &staff] {
            assert!(administers(&anchor, p), "{}", p.name);
        }
        assert!(administers(&client, &own));
        assert!(!administers(&client, &other));
        assert!(!administers(&client, &partner));
        assert!(!administers(&client, &clientless));
        assert!(!administers(&client, &staff));
        let super_admin = ctx(UserScope::Client, &["clt_a"], &["platform:*:*:*"]);
        assert!(administers(&super_admin, &clientless));
    }

    #[test]
    fn only_this_applications_sdk_roles_are_its_own() {
        let sdk = |r: &str| RoleAssignment::with_source(r, AssignmentSource::SdkSync);
        assert!(is_own_sdk_role(&sdk("hr:employee"), "hr"));
        assert!(!is_own_sdk_role(&sdk("rfp:buyer"), "hr"));
        assert!(!is_own_sdk_role(&sdk("hrx:employee"), "hr"));
        assert!(!is_own_sdk_role(&sdk("employee"), "hr"));
        assert!(!is_own_sdk_role(&RoleAssignment::new("hr:admin"), "hr"));
    }

    #[test]
    fn the_audit_copy_masks_the_hashes() {
        let command = SyncPrincipalsCommand {
            application_code: "hr".to_string(),
            principals: vec![SyncPrincipalInput {
                email: "jo@inhance.test".to_string(),
                name: "Jo".to_string(),
                roles: vec![],
                active: true,
                password_hash: Some("$2y$10$secret".to_string()),
            }],
            remove_unlisted: false,
        };
        let json = fc_common::audit_redaction::redacted_command_json(&command).unwrap();
        assert_eq!(json["principals"][0]["passwordHash"], "***");
    }
}

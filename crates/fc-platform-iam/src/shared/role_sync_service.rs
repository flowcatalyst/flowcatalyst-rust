//! Role Sync Service
//!
//! Synchronizes the code-defined platform roles into the database. The role
//! catalogue lives in `crate::role::entity::roles` (one source of truth);
//! this service is the IO boundary that pushes that catalogue into
//! `iam_roles` / `iam_role_permissions` at startup and via the BFF
//! `sync-platform` endpoint.
//!
//! Why one source of truth: a previous incarnation duplicated the catalogue
//! here as `RoleDefinition` consts. The two copies drifted — `super-admin`
//! lost the `platform:*:*:*` wildcard and `messaging-admin` never got the
//! scheduled-job permissions — and admins lost access to features the
//! entity definitions said they had. We don't redo that.

use std::collections::{BTreeSet, HashSet};
use tracing::{info, warn};

use crate::role::ceiling;
use crate::role::entity::roles;
use crate::role::entity::{AuthRole, RoleSource};
use crate::role::operations::require_confined;
use crate::role::repository::RoleRepository;
use fc_platform_core::shared::error::Result;
use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::usecase::Caller;
use std::sync::Arc;

/// Counts returned from `sync_code_defined_roles` so callers (the BFF
/// sync-platform endpoint, dev seeding) can surface the diff back to the user.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleSyncCounts {
    pub created: u32,
    pub updated: u32,
    pub removed: u32,
    pub total: u32,
}

/// Role Sync Service
pub struct RoleSyncService {
    role_repo: Arc<RoleRepository>,
}

impl RoleSyncService {
    pub fn new(role_repo: Arc<RoleRepository>) -> Self {
        Self { role_repo }
    }

    /// Sync all code-defined roles to the database, for `caller`.
    /// Call this at application startup (as [`Caller::system`]) or via the
    /// BFF sync-platform endpoint (as the requesting principal).
    pub async fn sync_code_defined_roles(&self, caller: &Caller) -> Result<RoleSyncCounts> {
        self.sync_catalogue(&roles::all(), caller).await
    }

    /// [`Self::sync_code_defined_roles`] with `code_roles` in place of the
    /// built-in catalogue (Go `SyncPlatformRoles` takes its catalogue the
    /// same way).
    ///
    /// Owner ruling 15 (Go F12): a catalogue role may gain only its own
    /// application's permissions, with no super-admin exception (the
    /// catalogue sync never crosses applications), 400
    /// `PERMISSION_OUTSIDE_APPLICATION`. Only what the sync adds is checked:
    /// a new role's permissions, an existing `CODE` role's additions. The
    /// built-in catalogue is all `platform:` roles holding `platform:`
    /// permissions, so it syncs as before.
    ///
    /// The permission ceiling (Go F10, `c56a68c`; owner approval
    /// 2026-10-14): every platform permission the sync adds to or removes
    /// from a role (an existing `CODE` role's change, a new role's
    /// permissions, a swept stale role's) must be one `caller` holds, 403
    /// `PERMISSION_ABOVE_CALLER`. The change is collected over the whole sync
    /// (per catalogue role its added then its removed permissions, then the
    /// swept roles'), after confinement. A same-named role of another source
    /// is skipped untouched and a stale role still assigned is kept, so
    /// neither is checked. The boot sync runs as [`Caller::system`], which
    /// holds every permission.
    ///
    /// Every check runs before the first write, so a refused sync writes
    /// nothing.
    pub async fn sync_catalogue(
        &self,
        code_roles: &[AuthRole],
        caller: &Caller,
    ) -> Result<RoleSyncCounts> {
        info!("Syncing code-defined roles to database...");

        let total = code_roles.len() as u32;
        let mut created = 0u32;
        let mut updated = 0u32;
        // Every permission the sync adds or removes, for the ceiling.
        let mut changed: Vec<String> = Vec::new();

        let mut stored = Vec::with_capacity(code_roles.len());
        for role_def in code_roles {
            let existing = self.role_repo.find_by_name(&role_def.name).await?;
            match &existing {
                Some(existing) if existing.source == RoleSource::Code => {
                    let added: BTreeSet<&str> = role_def
                        .permissions
                        .difference(&existing.permissions)
                        .map(String::as_str)
                        .collect();
                    require_confined(existing.owning_application_code(), added, false)?;
                    changed.extend(ceiling::changed(
                        &sorted(&existing.permissions),
                        &sorted(&role_def.permissions),
                    ));
                }
                Some(_) => {}
                None => {
                    let permissions: BTreeSet<&str> =
                        role_def.permissions.iter().map(String::as_str).collect();
                    require_confined(role_def.owning_application_code(), permissions, false)?;
                    changed.extend(sorted(&role_def.permissions));
                }
            }
            stored.push(existing);
        }

        let stale = self.stale_code_roles(code_roles).await?;
        for role in &stale {
            changed.extend(sorted(&role.permissions));
        }

        ceiling::require_permissions(Some(caller), changed.iter().map(String::as_str))?;

        for (role_def, existing) in code_roles.iter().zip(stored) {
            // Check if role exists
            if let Some(mut existing) = existing {
                // Only update if it's a CODE-sourced role
                if existing.source == RoleSource::Code {
                    // Go F30: a catalogue role that already matches is left
                    // alone: not saved, not counted.
                    let mut changed_row = existing.display_name != role_def.display_name
                        || existing.description != role_def.description
                        || existing.permissions != role_def.permissions;
                    existing.display_name = role_def.display_name.clone();
                    existing.description = role_def.description.clone();
                    existing.permissions = role_def.permissions.clone();
                    // Go F18: heal a row written without its application's id.
                    if existing.application_id.is_none() {
                        existing.application_id = self
                            .owner_application_id(existing.owning_application_code())
                            .await?;
                        if existing.application_id.is_some() {
                            changed_row = true;
                        }
                    }
                    if !changed_row {
                        continue;
                    }
                    existing.updated_at = chrono::Utc::now();
                    self.role_repo.update(&existing).await?;
                    updated += 1;
                } else {
                    warn!(
                        role = %role_def.name,
                        source = ?existing.source,
                        "Role exists with non-CODE source; not overwriting with code definition",
                    );
                }
            } else {
                // Create a fresh DB-side row from the code definition. We
                // re-build via `AuthRole::new` so the id is freshly generated
                // and the timestamps are now-anchored.
                let mut role = AuthRole::new(
                    role_def.application_code.as_str(),
                    role_def.role_name(),
                    role_def.display_name.as_str(),
                );
                role.description = role_def.description.clone();
                role.permissions = role_def.permissions.clone();
                role.source = RoleSource::Code;
                role.application_id = self
                    .owner_application_id(role.owning_application_code())
                    .await?;

                self.role_repo.insert(&role).await?;
                created += 1;
            }
        }

        // Remove stale CODE roles
        for role in &stale {
            info!("Removing stale CODE role: {}", role.name);
            self.role_repo.delete(&role.id).await?;
        }
        let removed = stale.len() as u32;

        info!(created, updated, removed, total, "Code role sync complete",);

        Ok(RoleSyncCounts {
            created,
            updated,
            removed,
            total,
        })
    }

    /// The id a role of application `code` carries (Go F18
    /// `ownerApplicationID`). An application that does not exist yields
    /// `None` with a warning rather than an error: the built-in catalogue
    /// must still be installable on a database whose `platform` application
    /// row has not been seeded yet; the next sync heals the row.
    async fn owner_application_id(&self, code: &str) -> Result<Option<ApplicationId>> {
        let id = self.role_repo.application_id_by_code(code).await?;
        if id.is_none() {
            warn!(
                application_code = code,
                "role written without an application id: no such application"
            );
        }
        Ok(id)
    }

    /// The CODE-sourced roles in the database that no longer exist in code
    /// and may be removed.
    ///
    /// A role still assigned to principals is kept. The operator is expected
    /// to re-assign or strip those users before the role can be deleted —
    /// silently dropping assignments was the source of a
    /// referential-integrity bug (`iam_principal_roles.role_name` has no
    /// DB-level FK; integrity is enforced in code via this guard + the
    /// `RoleRepository` delete cascade).
    async fn stale_code_roles(&self, code_roles: &[AuthRole]) -> Result<Vec<AuthRole>> {
        let code_role_names: HashSet<&str> = code_roles.iter().map(|r| r.name.as_str()).collect();

        let code_roles_in_db = self.role_repo.find_by_source(RoleSource::Code).await?;
        let mut stale = Vec::new();

        for db_role in code_roles_in_db {
            if code_role_names.contains(db_role.name.as_str()) {
                continue;
            }

            let assignments = self.role_repo.count_assignments(&db_role.name).await?;
            if assignments > 0 {
                warn!(
                    role = %db_role.name,
                    assignments,
                    "Skipping removal of stale CODE role — principals still hold it. \
                     Remove the assignments via the admin UI before the role can be deleted.",
                );
                continue;
            }

            stale.push(db_role);
        }

        Ok(stale)
    }
}

/// `permissions` in a stable order (a role holds a set), for the ceiling's
/// message.
fn sorted(permissions: &HashSet<String>) -> Vec<String> {
    let mut sorted: Vec<String> = permissions.iter().cloned().collect();
    sorted.sort();
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::role::entity::permissions_outside_application;

    /// Confinement leaves the boot sync alone: every built-in role holds
    /// only its own application's permissions.
    #[test]
    fn the_built_in_catalogue_is_confined() {
        for role in roles::all() {
            let outside = permissions_outside_application(
                role.owning_application_code(),
                role.permissions.iter().map(String::as_str),
            );
            assert!(outside.is_empty(), "{}: {outside:?}", role.name);
        }
    }
}

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

use std::collections::HashSet;
use tracing::{info, warn};

use crate::role::entity::roles;
use crate::shared::error::Result;
use crate::RoleRepository;
use crate::{AuthRole, RoleSource};

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
    role_repo: std::sync::Arc<RoleRepository>,
}

impl RoleSyncService {
    pub fn new(role_repo: std::sync::Arc<RoleRepository>) -> Self {
        Self { role_repo }
    }

    /// Sync all code-defined roles to the database.
    /// Call this at application startup or via the BFF sync-platform endpoint.
    pub async fn sync_code_defined_roles(&self) -> Result<RoleSyncCounts> {
        info!("Syncing code-defined roles to database...");

        let code_roles = roles::all();
        let total = code_roles.len() as u32;
        let mut created = 0u32;
        let mut updated = 0u32;

        for role_def in &code_roles {
            // Check if role exists
            if let Some(mut existing) = self.role_repo.find_by_name(&role_def.name).await? {
                // Only update if it's a CODE-sourced role
                if existing.source == RoleSource::Code {
                    existing.display_name = role_def.display_name.clone();
                    existing.description = role_def.description.clone();
                    existing.permissions = role_def.permissions.clone();
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

                self.role_repo.insert(&role).await?;
                created += 1;
            }
        }

        // Remove stale CODE roles
        let removed = self.remove_stale_code_roles(&code_roles).await? as u32;

        info!(created, updated, removed, total, "Code role sync complete",);

        Ok(RoleSyncCounts {
            created,
            updated,
            removed,
            total,
        })
    }

    /// Remove CODE-sourced roles from the database that no longer exist in code.
    ///
    /// Refuses to remove a role that is still assigned to principals. The
    /// operator is expected to re-assign or strip those users before the
    /// role can be deleted — silently dropping assignments was the source of
    /// a referential-integrity bug (`iam_principal_roles.role_name` has no
    /// DB-level FK; integrity is enforced in code via this guard + the
    /// `RoleRepository` delete cascade).
    async fn remove_stale_code_roles(&self, code_roles: &[AuthRole]) -> Result<usize> {
        let code_role_names: HashSet<&str> = code_roles.iter().map(|r| r.name.as_str()).collect();

        let code_roles_in_db = self.role_repo.find_by_source(RoleSource::Code).await?;
        let mut removed = 0;

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

            info!("Removing stale CODE role: {}", db_role.name);
            self.role_repo.delete(&db_role.id).await?;
            removed += 1;
        }

        Ok(removed)
    }
}

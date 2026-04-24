//! Role Sync Service
//!
//! Synchronizes code-defined platform roles to the database at startup.
//! Matches Java RoleSyncService behavior.

use std::collections::HashSet;
use tracing::{info, warn};

use crate::{AuthRole, RoleSource};
use crate::RoleRepository;

/// Code-defined role definition
pub struct RoleDefinition {
    pub application_code: &'static str,
    pub role_name: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub permissions: &'static [&'static str],
}

impl RoleDefinition {
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.application_code, self.role_name)
    }
}

/// Platform Super Admin - full access to everything
pub const PLATFORM_SUPER_ADMIN: RoleDefinition = RoleDefinition {
    application_code: "platform",
    role_name: "super-admin",
    display_name: "Platform Super Admin",
    description: "Full access to all platform features and administration",
    permissions: &[
        "platform:iam:user:view",
        "platform:iam:user:create",
        "platform:iam:user:update",
        "platform:iam:user:delete",
        "platform:iam:role:view",
        "platform:iam:role:create",
        "platform:iam:role:update",
        "platform:iam:role:delete",
        "platform:iam:permission:view",
        "platform:iam:service-account:view",
        "platform:iam:service-account:create",
        "platform:iam:service-account:update",
        "platform:iam:service-account:delete",
        "platform:iam:idp:manage",
        "platform:admin:client:view",
        "platform:admin:client:create",
        "platform:admin:client:update",
        "platform:admin:client:delete",
        "platform:admin:application:view",
        "platform:admin:application:create",
        "platform:admin:application:update",
        "platform:admin:application:delete",
        "platform:admin:config:view",
        "platform:admin:config:update",
        "platform:messaging:event:view",
        "platform:messaging:event:view-raw",
        "platform:messaging:event-type:view",
        "platform:messaging:event-type:create",
        "platform:messaging:event-type:update",
        "platform:messaging:event-type:delete",
        "platform:messaging:subscription:view",
        "platform:messaging:subscription:create",
        "platform:messaging:subscription:update",
        "platform:messaging:subscription:delete",
        "platform:messaging:dispatch-job:view",
        "platform:messaging:dispatch-job:view-raw",
        "platform:messaging:dispatch-job:create",
        "platform:messaging:dispatch-job:retry",
        "platform:messaging:dispatch-pool:view",
        "platform:messaging:dispatch-pool:create",
        "platform:messaging:dispatch-pool:update",
        "platform:messaging:dispatch-pool:delete",
    ],
};

/// Platform IAM Admin - user and role management
pub const PLATFORM_IAM_ADMIN: RoleDefinition = RoleDefinition {
    application_code: "platform",
    role_name: "iam-admin",
    display_name: "Platform IAM Admin",
    description: "Manage users, roles, and permissions",
    permissions: &[
        "platform:iam:user:view",
        "platform:iam:user:create",
        "platform:iam:user:update",
        "platform:iam:user:delete",
        "platform:iam:role:view",
        "platform:iam:role:create",
        "platform:iam:role:update",
        "platform:iam:role:delete",
        "platform:iam:permission:view",
        "platform:iam:service-account:view",
        "platform:iam:service-account:create",
        "platform:iam:service-account:update",
        "platform:iam:service-account:delete",
        "platform:iam:idp:manage",
    ],
};

/// Platform Admin - client and application management
pub const PLATFORM_ADMIN: RoleDefinition = RoleDefinition {
    application_code: "platform",
    role_name: "admin",
    display_name: "Platform Admin",
    description: "Manage clients and applications",
    permissions: &[
        "platform:admin:client:view",
        "platform:admin:client:create",
        "platform:admin:client:update",
        "platform:admin:client:delete",
        "platform:admin:application:view",
        "platform:admin:application:create",
        "platform:admin:application:update",
        "platform:admin:application:delete",
        "platform:admin:config:view",
        "platform:admin:config:update",
    ],
};

/// Platform Messaging Admin - event and subscription management
pub const PLATFORM_MESSAGING_ADMIN: RoleDefinition = RoleDefinition {
    application_code: "platform",
    role_name: "messaging-admin",
    display_name: "Platform Messaging Admin",
    description: "Manage events, subscriptions, and dispatch",
    permissions: &[
        "platform:messaging:event:view",
        "platform:messaging:event:view-raw",
        "platform:messaging:event-type:view",
        "platform:messaging:event-type:create",
        "platform:messaging:event-type:update",
        "platform:messaging:event-type:delete",
        "platform:messaging:subscription:view",
        "platform:messaging:subscription:create",
        "platform:messaging:subscription:update",
        "platform:messaging:subscription:delete",
        "platform:messaging:dispatch-job:view",
        "platform:messaging:dispatch-job:view-raw",
        "platform:messaging:dispatch-job:create",
        "platform:messaging:dispatch-job:retry",
        "platform:messaging:dispatch-pool:view",
        "platform:messaging:dispatch-pool:create",
        "platform:messaging:dispatch-pool:update",
        "platform:messaging:dispatch-pool:delete",
    ],
};

/// Platform Viewer - read-only access
pub const PLATFORM_VIEWER: RoleDefinition = RoleDefinition {
    application_code: "platform",
    role_name: "viewer",
    display_name: "Platform Viewer",
    description: "Read-only access to platform data",
    permissions: &[
        "platform:iam:user:view",
        "platform:iam:role:view",
        "platform:iam:permission:view",
        "platform:iam:service-account:view",
        "platform:admin:client:view",
        "platform:admin:application:view",
        "platform:admin:config:view",
        "platform:messaging:event:view",
        "platform:messaging:event-type:view",
        "platform:messaging:subscription:view",
        "platform:messaging:dispatch-job:view",
        "platform:messaging:dispatch-pool:view",
    ],
};

/// All code-defined platform roles
pub const CODE_DEFINED_ROLES: &[&RoleDefinition] = &[
    &PLATFORM_SUPER_ADMIN,
    &PLATFORM_IAM_ADMIN,
    &PLATFORM_ADMIN,
    &PLATFORM_MESSAGING_ADMIN,
    &PLATFORM_VIEWER,
];

/// Role Sync Service
pub struct RoleSyncService {
    role_repo: RoleRepository,
}

impl RoleSyncService {
    pub fn new(role_repo: RoleRepository) -> Self {
        Self { role_repo }
    }

    /// Sync all code-defined roles to the database.
    /// Call this at application startup.
    pub async fn sync_code_defined_roles(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Syncing code-defined roles to database...");

        let mut created = 0;
        let mut updated = 0;

        for role_def in CODE_DEFINED_ROLES {
            let role_name = role_def.full_name();

            // Check if role exists
            if let Some(mut existing) = self.role_repo.find_by_name(&role_name).await? {
                // Only update if it's a CODE-sourced role
                if existing.source == RoleSource::Code {
                    existing.display_name = role_def.display_name.to_string();
                    existing.description = Some(role_def.description.to_string());
                    existing.permissions = role_def.permissions.iter().map(|s| s.to_string()).collect();
                    existing.updated_at = chrono::Utc::now();
                    self.role_repo.update(&existing).await?;
                    updated += 1;
                } else {
                    warn!(
                        "Role {} exists with source {:?}, not overwriting with CODE definition",
                        role_name, existing.source
                    );
                }
            } else {
                // Create new role
                let mut role = AuthRole::new(
                    role_def.application_code,
                    role_def.role_name,
                    role_def.display_name,
                );
                role.description = Some(role_def.description.to_string());
                role.permissions = role_def.permissions.iter().map(|s| s.to_string()).collect();
                role.source = RoleSource::Code;

                self.role_repo.insert(&role).await?;
                created += 1;
            }
        }

        // Remove stale CODE roles
        let removed = self.remove_stale_code_roles().await?;

        info!(
            "Code role sync complete: {} created, {} updated, {} removed",
            created, updated, removed
        );

        Ok(())
    }

    /// Remove CODE-sourced roles from the database that no longer exist in code.
    ///
    /// Refuses to remove a role that is still assigned to principals. The
    /// operator is expected to re-assign or strip those users before the
    /// role can be deleted — silently dropping assignments was the source of
    /// a referential-integrity bug (`iam_principal_roles.role_name` has no
    /// DB-level FK; integrity is enforced in code via this guard + the
    /// `RoleRepository` delete cascade).
    async fn remove_stale_code_roles(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let code_role_names: HashSet<String> = CODE_DEFINED_ROLES
            .iter()
            .map(|r| r.full_name())
            .collect();

        let code_roles_in_db = self.role_repo.find_by_source(RoleSource::Code).await?;
        let mut removed = 0;

        for db_role in code_roles_in_db {
            if code_role_names.contains(&db_role.name) {
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

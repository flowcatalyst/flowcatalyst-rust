//! OIDC User and Role Synchronization Service
//!
//! CRITICAL SECURITY: This service implements the IDP role authorization control.
//!
//! Only IDP roles that are explicitly authorized in the idp_role_mappings table
//! are accepted during OIDC login. This prevents partners/customers from
//! injecting unauthorized roles via compromised or misconfigured IDPs.
//!
//! Example attack prevented:
//! - Partner IDP is compromised and grants all users "super-admin" role
//! - This service rejects the role because it's not in idp_role_mappings
//! - Attack is logged and prevented
//!
//! See Java implementation: OidcSyncService.java

use std::collections::HashSet;
use std::sync::Arc;
use chrono::Utc;
use tracing::{debug, info, warn};

use crate::{Principal, UserScope, ExternalIdentity};
use crate::auth::config_entity::IdpRoleMapping;
use crate::{PrincipalRepository, IdpRoleMappingRepository};
use crate::shared::error::Result;

/// Assignment source for IDP-synced roles
pub const IDP_SYNC_SOURCE: &str = "IDP_SYNC";

/// OIDC User and Role Synchronization Service
pub struct OidcSyncService {
    principal_repo: Arc<PrincipalRepository>,
    idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
}

impl OidcSyncService {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
    ) -> Self {
        Self {
            principal_repo,
            idp_role_mapping_repo,
        }
    }

    /// Synchronize user information from OIDC token.
    /// Creates or updates the user principal based on OIDC claims.
    ///
    /// # Arguments
    /// * `email` - User email from OIDC token
    /// * `name` - User display name from OIDC token
    /// * `external_idp_id` - Subject from OIDC token (IDP's user ID)
    /// * `provider_id` - OIDC provider identifier
    /// * `client_id` - Home tenant ID (None for anchor domain users)
    /// * `scope` - User scope (ANCHOR, PARTNER, or CLIENT)
    ///
    /// # Returns
    /// Synchronized principal
    pub async fn sync_oidc_user(
        &self,
        email: &str,
        name: &str,
        external_idp_id: &str,
        provider_id: &str,
        client_id: Option<&str>,
        scope: UserScope,
    ) -> Result<Principal> {
        // Try to find existing user by email
        let existing = self.principal_repo.find_by_email(email).await?;

        let mut principal = if let Some(mut existing_principal) = existing {
            // Update existing user
            existing_principal.name = name.to_string();

            if let Some(ref mut identity) = existing_principal.user_identity {
                identity.external_id = Some(external_idp_id.to_string());
                identity.provider = Some(provider_id.to_string());
            }

            existing_principal.external_identity = Some(ExternalIdentity {
                provider_id: provider_id.to_string(),
                external_id: external_idp_id.to_string(),
            });

            existing_principal.updated_at = Utc::now();

            self.principal_repo.update(&existing_principal).await?;
            existing_principal
        } else {
            // Create new user
            let mut new_principal = Principal::new_user(email, scope);
            new_principal.name = name.to_string();

            if let Some(ref mut identity) = new_principal.user_identity {
                identity.external_id = Some(external_idp_id.to_string());
                identity.provider = Some(provider_id.to_string());
            }

            new_principal.external_identity = Some(ExternalIdentity {
                provider_id: provider_id.to_string(),
                external_id: external_idp_id.to_string(),
            });

            if let Some(cid) = client_id {
                new_principal.client_id = Some(cid.to_string());
            }

            self.principal_repo.insert(&new_principal).await?;
            new_principal
        };

        // Update last login
        principal.update_last_login();
        self.principal_repo.update(&principal).await?;

        info!(
            principal_id = %principal.id,
            email = %email,
            provider = %provider_id,
            "OIDC user synchronized"
        );

        Ok(principal)
    }

    /// CRITICAL SECURITY: Synchronize IDP roles to internal roles.
    ///
    /// This method implements the IDP role authorization security control.
    /// Only IDP roles that are explicitly authorized in the idp_role_mappings
    /// table are accepted. Any unauthorized role is rejected and logged.
    ///
    /// Flow:
    /// 1. For each IDP role name from the token:
    ///    a. Look up the role in idp_role_mappings
    ///    b. If found: Accept and add the mapped internal role name
    ///    c. If NOT found: REJECT and log as security warning
    /// 2. Remove all existing IDP-sourced roles from the principal
    /// 3. Assign all authorized internal role names with "IDP_SYNC" source
    ///
    /// SECURITY NOTE: This prevents the following attack:
    /// - A compromised or misconfigured IDP grants unauthorized roles (e.g., "super-admin")
    /// - This service rejects the role because it's not in idp_role_mappings
    /// - Platform administrator must explicitly authorize IDP roles before they work
    /// - All rejections are logged for security auditing
    ///
    /// # Arguments
    /// * `principal` - The user principal to sync roles for
    /// * `idp_role_names` - List of role names from the OIDC token (e.g., from realm_access.roles)
    ///
    /// # Returns
    /// Set of accepted internal role names (e.g., "platform:tenant-admin")
    pub async fn sync_idp_roles(
        &self,
        principal: &mut Principal,
        idp_role_names: &[String],
    ) -> Result<HashSet<String>> {
        let mut authorized_role_names: HashSet<String> = HashSet::new();
        let email = principal.email().unwrap_or("unknown").to_string();

        if idp_role_names.is_empty() {
            info!(
                principal_id = %principal.id,
                "No IDP roles provided"
            );
        } else {
            // SECURITY: Only accept IDP roles that are explicitly authorized in idp_role_mappings
            for idp_role_name in idp_role_names {
                let mapping = self.find_idp_role_mapping(idp_role_name).await?;

                if let Some(mapping) = mapping {
                    // This IDP role is authorized - map to internal role name
                    authorized_role_names.insert(mapping.platform_role_name.clone());
                    debug!(
                        principal_id = %principal.id,
                        idp_role = %idp_role_name,
                        internal_role = %mapping.platform_role_name,
                        "Accepted IDP role"
                    );
                } else {
                    // SECURITY: Reject unauthorized IDP role
                    // This prevents malicious/misconfigured IDPs from granting unauthorized access
                    warn!(
                        principal_id = %principal.id,
                        email = %email,
                        idp_role = %idp_role_name,
                        "SECURITY: REJECTED unauthorized IDP role. Role not found in idp_role_mappings table. \
                         Platform administrator must explicitly authorize this IDP role before it can be used."
                    );
                }
            }
        }

        // Remove all existing IDP-sourced roles
        let removed_count = principal.remove_roles_by_source(IDP_SYNC_SOURCE);
        if removed_count > 0 {
            debug!(
                principal_id = %principal.id,
                removed_count = removed_count,
                "Removed old IDP-sourced roles"
            );
        }

        // Assign all authorized internal role names
        let mut assigned_count = 0;
        for role_name in &authorized_role_names {
            // Check if role is already assigned from another source
            if !principal.has_role(role_name) {
                principal.assign_role_with_source(role_name, IDP_SYNC_SOURCE);
                assigned_count += 1;
            } else {
                debug!(
                    principal_id = %principal.id,
                    role = %role_name,
                    "Role already assigned from another source"
                );
            }
        }

        // Save updated principal
        self.principal_repo.update(principal).await?;

        info!(
            principal_id = %principal.id,
            email = %email,
            provided_count = idp_role_names.len(),
            authorized_count = authorized_role_names.len(),
            assigned_count = assigned_count,
            "IDP role sync complete"
        );

        Ok(authorized_role_names)
    }

    /// Full OIDC sync: sync both user info and roles.
    /// This is the main method called during OIDC login callback.
    ///
    /// # Arguments
    /// * `email` - User email from OIDC token
    /// * `name` - User display name from OIDC token
    /// * `external_idp_id` - Subject from OIDC token
    /// * `provider_id` - OIDC provider identifier
    /// * `client_id` - Home tenant ID (None for anchor users)
    /// * `scope` - User scope
    /// * `idp_role_names` - List of role names from OIDC token
    ///
    /// # Returns
    /// Synchronized principal
    pub async fn sync_oidc_login(
        &self,
        email: &str,
        name: &str,
        external_idp_id: &str,
        provider_id: &str,
        client_id: Option<&str>,
        scope: UserScope,
        idp_role_names: &[String],
    ) -> Result<Principal> {
        // Sync user information
        let mut principal = self
            .sync_oidc_user(email, name, external_idp_id, provider_id, client_id, scope)
            .await?;

        // CRITICAL SECURITY: Sync IDP roles with authorization check
        self.sync_idp_roles(&mut principal, idp_role_names).await?;

        Ok(principal)
    }

    /// Find IDP role mapping by IDP role name
    async fn find_idp_role_mapping(&self, idp_role_name: &str) -> Result<Option<IdpRoleMapping>> {
        // For now, search across all IDP types
        // In production, you might want to filter by IDP type
        let mappings = self.idp_role_mapping_repo.find_all().await?;

        Ok(mappings.into_iter().find(|m| m.idp_role_name == idp_role_name))
    }

    /// Audit log all IDP role mappings for a principal.
    /// Used for security auditing and debugging.
    pub async fn audit_idp_roles(&self, principal_id: &str) -> Result<String> {
        let principal = self.principal_repo.find_by_id(principal_id).await?;

        match principal {
            Some(p) => {
                let idp_roles: Vec<_> = p.roles.iter()
                    .filter(|r| r.is_idp_sync())
                    .collect();

                let mut audit = format!(
                    "Principal {} has {} IDP-sourced roles:\n",
                    principal_id,
                    idp_roles.len()
                );

                for assignment in idp_roles {
                    audit.push_str(&format!(
                        "  - Role: {}, assigned at {}\n",
                        assignment.role,
                        assignment.assigned_at.to_rfc3339()
                    ));
                }

                Ok(audit)
            }
            None => Ok(format!("Principal {} not found", principal_id)),
        }
    }

    /// Get IDP roles for a principal
    pub async fn get_idp_roles(&self, principal_id: &str) -> Result<Vec<String>> {
        let principal = self.principal_repo.find_by_id(principal_id).await?;

        Ok(principal
            .map(|p| {
                p.roles
                    .iter()
                    .filter(|r| r.is_idp_sync())
                    .map(|r| r.role.clone())
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idp_sync_source_constant() {
        assert_eq!(IDP_SYNC_SOURCE, "IDP_SYNC");
    }
}

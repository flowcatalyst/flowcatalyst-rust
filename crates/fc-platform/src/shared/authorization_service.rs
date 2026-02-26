//! Authorization Service
//!
//! Permission-based access control with role resolution.

use std::collections::HashSet;
use std::sync::Arc;
use crate::permissions;
use crate::RoleRepository;
use crate::shared::error::{PlatformError, Result};
use crate::AccessTokenClaims;

/// Authorization context for a request
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Principal ID
    pub principal_id: String,

    /// Principal type (USER or SERVICE)
    pub principal_type: String,

    /// User scope
    pub scope: String,

    /// Email (for users)
    pub email: Option<String>,

    /// Display name
    pub name: String,

    /// Client IDs this principal can access
    pub accessible_clients: Vec<String>,

    /// All permissions (resolved from roles)
    pub permissions: HashSet<String>,

    /// Role codes
    pub roles: Vec<String>,
}

impl AuthContext {
    /// Create from JWT claims with resolved permissions
    pub fn from_claims_with_permissions(
        claims: &AccessTokenClaims,
        permissions: HashSet<String>,
    ) -> Self {
        Self {
            principal_id: claims.sub.clone(),
            principal_type: claims.principal_type.clone(),
            scope: claims.scope.clone(),
            email: claims.email.clone(),
            name: claims.name.clone(),
            accessible_clients: claims.clients.clone(),
            permissions,
            roles: claims.roles.clone(),
        }
    }

    /// Check if this context is for an anchor user
    pub fn is_anchor(&self) -> bool {
        self.scope == "ANCHOR"
    }

    /// Check if this context can access a specific client
    pub fn can_access_client(&self, client_id: &str) -> bool {
        self.accessible_clients.contains(&"*".to_string()) ||
            self.accessible_clients.contains(&client_id.to_string())
    }

    /// Check if this context has a specific permission
    pub fn has_permission(&self, permission: &str) -> bool {
        // Direct match
        if self.permissions.contains(permission) {
            return true;
        }

        // Wildcard matching
        let parts: Vec<&str> = permission.split(':').collect();
        if parts.len() >= 2 {
            // Check resource:* wildcard
            let wildcard = format!("{}:*", parts[0]);
            if self.permissions.contains(&wildcard) {
                return true;
            }

            // Check superuser *:*
            if self.permissions.contains(permissions::ADMIN_ALL) {
                return true;
            }
        }

        false
    }

    /// Check if this context has all specified permissions
    pub fn has_all_permissions(&self, required: &[&str]) -> bool {
        required.iter().all(|p| self.has_permission(p))
    }

    /// Check if this context has any of the specified permissions
    pub fn has_any_permission(&self, required: &[&str]) -> bool {
        required.iter().any(|p| self.has_permission(p))
    }

    /// Check if this context has a specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(&role.to_string())
    }
}

/// Authorization service for checking permissions
pub struct AuthorizationService {
    role_repo: Arc<RoleRepository>,
}

impl AuthorizationService {
    pub fn new(role_repo: Arc<RoleRepository>) -> Self {
        Self { role_repo }
    }

    /// Build an authorization context from JWT claims
    /// Resolves all permissions from roles
    pub async fn build_context(&self, claims: &AccessTokenClaims) -> Result<AuthContext> {
        let permissions = self.resolve_permissions(&claims.roles).await?;
        Ok(AuthContext::from_claims_with_permissions(claims, permissions))
    }

    /// Resolve all permissions for a set of role codes
    async fn resolve_permissions(&self, role_codes: &[String]) -> Result<HashSet<String>> {
        if role_codes.is_empty() {
            return Ok(HashSet::new());
        }

        let roles = self.role_repo.find_by_codes(role_codes).await?;
        let mut permissions = HashSet::new();

        for role in roles {
            permissions.extend(role.permissions);
        }

        Ok(permissions)
    }

    /// Check if a principal can perform an action on a resource
    pub fn authorize(
        &self,
        context: &AuthContext,
        permission: &str,
        client_id: Option<&str>,
    ) -> Result<()> {
        // Check permission
        if !context.has_permission(permission) {
            return Err(PlatformError::forbidden(format!(
                "Missing permission: {}",
                permission
            )));
        }

        // Check client access if client-specific
        if let Some(cid) = client_id {
            if !context.can_access_client(cid) {
                return Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    cid
                )));
            }
        }

        Ok(())
    }

    /// Require anchor scope
    pub fn require_anchor(&self, context: &AuthContext) -> Result<()> {
        if !context.is_anchor() {
            return Err(PlatformError::forbidden("Anchor scope required"));
        }
        Ok(())
    }

    /// Require specific permission
    pub fn require_permission(&self, context: &AuthContext, permission: &str) -> Result<()> {
        if !context.has_permission(permission) {
            return Err(PlatformError::forbidden(format!(
                "Permission required: {}",
                permission
            )));
        }
        Ok(())
    }

    /// Require client access
    pub fn require_client_access(&self, context: &AuthContext, client_id: &str) -> Result<()> {
        if !context.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!(
                "Client access required: {}",
                client_id
            )));
        }
        Ok(())
    }
}

/// Common authorization checks
pub mod checks {
    use super::*;

    /// Require anchor scope
    pub fn require_anchor(context: &AuthContext) -> Result<()> {
        if context.is_anchor() {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Anchor access required"))
        }
    }

    /// Check read access to events
    pub fn can_read_events(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::EVENT_VIEW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read events"))
        }
    }

    /// Check raw read access to events (includes payload)
    pub fn can_read_events_raw(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::EVENT_VIEW_RAW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read raw event data"))
        }
    }

    /// Check read access to event types
    pub fn can_read_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::EVENT_TYPE_VIEW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read event types"))
        }
    }

    /// Check create access to event types
    pub fn can_create_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::EVENT_TYPE_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create event types"))
        }
    }

    /// Check update access to event types
    pub fn can_update_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::EVENT_TYPE_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update event types"))
        }
    }

    /// Check delete access to event types
    pub fn can_delete_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::EVENT_TYPE_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete event types"))
        }
    }

    /// Check read access to subscriptions
    pub fn can_read_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::SUBSCRIPTION_VIEW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read subscriptions"))
        }
    }

    /// Check create access to subscriptions
    pub fn can_create_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::SUBSCRIPTION_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create subscriptions"))
        }
    }

    /// Check update access to subscriptions
    pub fn can_update_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::SUBSCRIPTION_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update subscriptions"))
        }
    }

    /// Check delete access to subscriptions
    pub fn can_delete_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::SUBSCRIPTION_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete subscriptions"))
        }
    }

    /// Check read access to dispatch jobs
    pub fn can_read_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::DISPATCH_JOB_VIEW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read dispatch jobs"))
        }
    }

    /// Check raw read access to dispatch jobs (includes payload)
    pub fn can_read_dispatch_jobs_raw(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::DISPATCH_JOB_VIEW_RAW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read raw dispatch job data"))
        }
    }

    /// Check create access to dispatch jobs
    pub fn can_create_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::DISPATCH_JOB_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create dispatch jobs"))
        }
    }

    /// Check retry access to dispatch jobs
    pub fn can_retry_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::messaging::DISPATCH_JOB_RETRY) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot retry dispatch jobs"))
        }
    }

    /// Check admin access (any admin permission)
    pub fn is_admin(context: &AuthContext) -> Result<()> {
        if context.is_anchor() || context.has_permission(permissions::ADMIN_ALL) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Admin access required"))
        }
    }

    /// Check write access to events (create)
    pub fn can_write_events(context: &AuthContext) -> Result<()> {
        // Allow if user has messaging permission or application service permission
        if context.has_any_permission(&[
            permissions::messaging::EVENT_CREATE,
            permissions::application_service::EVENT_CREATE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write events"))
        }
    }

    /// Check write access to event types (create, update, or delete)
    pub fn can_write_event_types(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::messaging::EVENT_TYPE_CREATE,
            permissions::messaging::EVENT_TYPE_UPDATE,
            permissions::messaging::EVENT_TYPE_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write event types"))
        }
    }

    /// Check write access to subscriptions (create, update, or delete)
    pub fn can_write_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::messaging::SUBSCRIPTION_CREATE,
            permissions::messaging::SUBSCRIPTION_UPDATE,
            permissions::messaging::SUBSCRIPTION_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write subscriptions"))
        }
    }

    /// Check write access to dispatch jobs (create or retry)
    pub fn can_write_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::messaging::DISPATCH_JOB_CREATE,
            permissions::messaging::DISPATCH_JOB_RETRY,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write dispatch jobs"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context(permissions: Vec<&str>, scope: &str, clients: Vec<&str>) -> AuthContext {
        AuthContext {
            principal_id: "test123".to_string(),
            principal_type: "USER".to_string(),
            scope: scope.to_string(),
            email: Some("test@example.com".to_string()),
            name: "Test User".to_string(),
            accessible_clients: clients.into_iter().map(String::from).collect(),
            permissions: permissions.into_iter().map(String::from).collect(),
            roles: vec!["test:admin".to_string()],
        }
    }

    #[test]
    fn test_direct_permission() {
        let ctx = create_test_context(vec!["events:read"], "CLIENT", vec!["client1"]);
        assert!(ctx.has_permission("events:read"));
        assert!(!ctx.has_permission("events:write"));
    }

    #[test]
    fn test_wildcard_permission() {
        let ctx = create_test_context(vec!["events:*"], "CLIENT", vec!["client1"]);
        assert!(ctx.has_permission("events:read"));
        assert!(ctx.has_permission("events:write"));
        assert!(!ctx.has_permission("users:read"));
    }

    #[test]
    fn test_superuser_permission() {
        let ctx = create_test_context(vec!["*:*"], "ANCHOR", vec!["*"]);
        assert!(ctx.has_permission("events:read"));
        assert!(ctx.has_permission("users:delete"));
        assert!(ctx.has_permission("anything:everything"));
    }

    #[test]
    fn test_client_access() {
        let ctx = create_test_context(vec![], "CLIENT", vec!["client1", "client2"]);
        assert!(ctx.can_access_client("client1"));
        assert!(ctx.can_access_client("client2"));
        assert!(!ctx.can_access_client("client3"));
    }

    #[test]
    fn test_anchor_all_clients() {
        let ctx = create_test_context(vec![], "ANCHOR", vec!["*"]);
        assert!(ctx.can_access_client("any_client"));
        assert!(ctx.can_access_client("another_client"));
    }
}

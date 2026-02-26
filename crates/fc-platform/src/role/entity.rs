//! Role and Permission Entities
//!
//! Authorization model for role-based access control.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashSet;

/// Role source - where the role definition came from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoleSource {
    /// Defined in code (cannot be modified)
    Code,
    /// Defined in database (can be modified)
    Database,
    /// Synced from external SDK/IDP
    Sdk,
}

impl Default for RoleSource {
    fn default() -> Self {
        Self::Database
    }
}

impl RoleSource {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Code => "CODE",
            Self::Database => "DATABASE",
            Self::Sdk => "SDK",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "CODE" => Self::Code,
            "SDK" => Self::Sdk,
            _ => Self::Database,
        }
    }
}

/// Permission definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    /// Permission string (e.g., "orders:read", "users:write")
    pub permission: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Category for grouping in UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

impl Permission {
    pub fn new(permission: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            permission: permission.into(),
            name: name.into(),
            description: None,
            category: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}

/// Role definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthRole {
    /// TSID as Crockford Base32 string
    pub id: String,

    /// Application ID reference (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,

    /// Full role name with application prefix (e.g., "platform:admin")
    /// Maps to `name` column in iam_roles table
    pub name: String,

    /// Human-readable display name
    pub display_name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Application this role belongs to (denormalized)
    pub application_code: String,

    /// Permissions granted by this role
    /// Loaded from iam_role_permissions junction table
    #[serde(default)]
    pub permissions: HashSet<String>,

    /// Where the role came from
    #[serde(default)]
    pub source: RoleSource,

    /// Whether clients can manage this role
    #[serde(default)]
    pub client_managed: bool,

    /// Audit fields
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AuthRole {
    pub fn new(
        application_code: impl Into<String>,
        role_name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        let app = application_code.into();
        let rname = role_name.into();
        let now = Utc::now();

        Self {
            id: crate::TsidGenerator::generate(),
            application_id: None,
            name: format!("{}:{}", app, rname),
            display_name: display_name.into(),
            description: None,
            application_code: app,
            permissions: HashSet::new(),
            source: RoleSource::Database,
            client_managed: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_permission(mut self, permission: impl Into<String>) -> Self {
        self.permissions.insert(permission.into());
        self
    }

    pub fn with_permissions(mut self, permissions: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for p in permissions {
            self.permissions.insert(p.into());
        }
        self
    }

    pub fn with_source(mut self, source: RoleSource) -> Self {
        self.source = source;
        self
    }

    pub fn with_client_managed(mut self, client_managed: bool) -> Self {
        self.client_managed = client_managed;
        self
    }

    pub fn grant_permission(&mut self, permission: impl Into<String>) {
        self.permissions.insert(permission.into());
        self.updated_at = Utc::now();
    }

    pub fn revoke_permission(&mut self, permission: &str) {
        self.permissions.remove(permission);
        self.updated_at = Utc::now();
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission) || self.has_wildcard_permission(permission)
    }

    /// Check for wildcard permissions
    fn has_wildcard_permission(&self, permission: &str) -> bool {
        if self.permissions.contains("*:*") {
            return true;
        }

        let parts: Vec<&str> = permission.split(':').collect();
        if parts.is_empty() {
            return false;
        }

        let mut prefix = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                prefix.push(':');
            }
            prefix.push_str(part);

            if i < parts.len() - 1 {
                let wildcard = format!("{}:*", prefix);
                if self.permissions.contains(&wildcard) {
                    return true;
                }
            }
        }

        false
    }

    pub fn can_modify(&self) -> bool {
        self.source == RoleSource::Database
    }

    /// Extract short role name from full name
    pub fn role_name(&self) -> &str {
        self.name.split(':').nth(1).unwrap_or(&self.name)
    }
}

/// Convert from SeaORM model to domain entity
/// Note: permissions must be loaded separately from iam_role_permissions
impl From<crate::entities::iam_roles::Model> for AuthRole {
    fn from(m: crate::entities::iam_roles::Model) -> Self {
        // Extract application_code from the role name (part before first colon)
        let application_code = m.application_code.unwrap_or_else(|| {
            m.name.split(':').next().unwrap_or("unknown").to_string()
        });

        Self {
            id: m.id,
            application_id: m.application_id,
            name: m.name,
            display_name: m.display_name,
            description: m.description,
            application_code,
            permissions: HashSet::new(), // Must be loaded from junction table
            source: RoleSource::from_str(&m.source),
            client_managed: m.client_managed,
            created_at: m.created_at.naive_utc().and_utc(),
            updated_at: m.updated_at.naive_utc().and_utc(),
        }
    }
}

/// Platform permissions - Granular format: platform:{category}:{entity}:{action}
/// Matches Java PermissionRegistry for cross-platform compatibility
pub mod permissions {
    /// IAM (Identity & Access Management) permissions
    pub mod iam {
        pub const USER_VIEW: &str = "platform:iam:user:view";
        pub const USER_CREATE: &str = "platform:iam:user:create";
        pub const USER_UPDATE: &str = "platform:iam:user:update";
        pub const USER_DELETE: &str = "platform:iam:user:delete";

        pub const ROLE_VIEW: &str = "platform:iam:role:view";
        pub const ROLE_CREATE: &str = "platform:iam:role:create";
        pub const ROLE_UPDATE: &str = "platform:iam:role:update";
        pub const ROLE_DELETE: &str = "platform:iam:role:delete";

        pub const PERMISSION_VIEW: &str = "platform:iam:permission:view";

        pub const SERVICE_ACCOUNT_VIEW: &str = "platform:iam:service-account:view";
        pub const SERVICE_ACCOUNT_CREATE: &str = "platform:iam:service-account:create";
        pub const SERVICE_ACCOUNT_UPDATE: &str = "platform:iam:service-account:update";
        pub const SERVICE_ACCOUNT_DELETE: &str = "platform:iam:service-account:delete";

        pub const IDP_MANAGE: &str = "platform:iam:idp:manage";

        /// All IAM permissions
        pub const ALL: &[&str] = &[
            USER_VIEW, USER_CREATE, USER_UPDATE, USER_DELETE,
            ROLE_VIEW, ROLE_CREATE, ROLE_UPDATE, ROLE_DELETE,
            PERMISSION_VIEW,
            SERVICE_ACCOUNT_VIEW, SERVICE_ACCOUNT_CREATE, SERVICE_ACCOUNT_UPDATE, SERVICE_ACCOUNT_DELETE,
            IDP_MANAGE,
        ];
    }

    /// Platform Admin permissions (clients, applications, config)
    pub mod admin {
        pub const CLIENT_VIEW: &str = "platform:admin:client:view";
        pub const CLIENT_CREATE: &str = "platform:admin:client:create";
        pub const CLIENT_UPDATE: &str = "platform:admin:client:update";
        pub const CLIENT_DELETE: &str = "platform:admin:client:delete";

        pub const APPLICATION_VIEW: &str = "platform:admin:application:view";
        pub const APPLICATION_CREATE: &str = "platform:admin:application:create";
        pub const APPLICATION_UPDATE: &str = "platform:admin:application:update";
        pub const APPLICATION_DELETE: &str = "platform:admin:application:delete";

        pub const CONFIG_VIEW: &str = "platform:admin:config:view";
        pub const CONFIG_UPDATE: &str = "platform:admin:config:update";

        /// All admin permissions
        pub const ALL: &[&str] = &[
            CLIENT_VIEW, CLIENT_CREATE, CLIENT_UPDATE, CLIENT_DELETE,
            APPLICATION_VIEW, APPLICATION_CREATE, APPLICATION_UPDATE, APPLICATION_DELETE,
            CONFIG_VIEW, CONFIG_UPDATE,
        ];
    }

    /// Messaging permissions (events, event types, subscriptions, dispatch)
    pub mod messaging {
        pub const EVENT_VIEW: &str = "platform:messaging:event:view";
        pub const EVENT_VIEW_RAW: &str = "platform:messaging:event:view-raw";
        pub const EVENT_CREATE: &str = "platform:messaging:event:create";

        pub const EVENT_TYPE_VIEW: &str = "platform:messaging:event-type:view";
        pub const EVENT_TYPE_CREATE: &str = "platform:messaging:event-type:create";
        pub const EVENT_TYPE_UPDATE: &str = "platform:messaging:event-type:update";
        pub const EVENT_TYPE_DELETE: &str = "platform:messaging:event-type:delete";

        pub const SUBSCRIPTION_VIEW: &str = "platform:messaging:subscription:view";
        pub const SUBSCRIPTION_CREATE: &str = "platform:messaging:subscription:create";
        pub const SUBSCRIPTION_UPDATE: &str = "platform:messaging:subscription:update";
        pub const SUBSCRIPTION_DELETE: &str = "platform:messaging:subscription:delete";

        pub const DISPATCH_JOB_VIEW: &str = "platform:messaging:dispatch-job:view";
        pub const DISPATCH_JOB_VIEW_RAW: &str = "platform:messaging:dispatch-job:view-raw";
        pub const DISPATCH_JOB_CREATE: &str = "platform:messaging:dispatch-job:create";
        pub const DISPATCH_JOB_RETRY: &str = "platform:messaging:dispatch-job:retry";

        pub const DISPATCH_POOL_VIEW: &str = "platform:messaging:dispatch-pool:view";
        pub const DISPATCH_POOL_CREATE: &str = "platform:messaging:dispatch-pool:create";
        pub const DISPATCH_POOL_UPDATE: &str = "platform:messaging:dispatch-pool:update";
        pub const DISPATCH_POOL_DELETE: &str = "platform:messaging:dispatch-pool:delete";

        /// All messaging permissions
        pub const ALL: &[&str] = &[
            EVENT_VIEW, EVENT_VIEW_RAW, EVENT_CREATE,
            EVENT_TYPE_VIEW, EVENT_TYPE_CREATE, EVENT_TYPE_UPDATE, EVENT_TYPE_DELETE,
            SUBSCRIPTION_VIEW, SUBSCRIPTION_CREATE, SUBSCRIPTION_UPDATE, SUBSCRIPTION_DELETE,
            DISPATCH_JOB_VIEW, DISPATCH_JOB_VIEW_RAW, DISPATCH_JOB_CREATE, DISPATCH_JOB_RETRY,
            DISPATCH_POOL_VIEW, DISPATCH_POOL_CREATE, DISPATCH_POOL_UPDATE, DISPATCH_POOL_DELETE,
        ];
    }

    /// Application Service permissions (scoped to own application)
    pub mod application_service {
        pub const EVENT_CREATE: &str = "platform:application-service:event:create";

        pub const EVENT_TYPE_VIEW: &str = "platform:application-service:event-type:view";
        pub const EVENT_TYPE_CREATE: &str = "platform:application-service:event-type:create";
        pub const EVENT_TYPE_UPDATE: &str = "platform:application-service:event-type:update";
        pub const EVENT_TYPE_DELETE: &str = "platform:application-service:event-type:delete";

        pub const SUBSCRIPTION_VIEW: &str = "platform:application-service:subscription:view";
        pub const SUBSCRIPTION_CREATE: &str = "platform:application-service:subscription:create";
        pub const SUBSCRIPTION_UPDATE: &str = "platform:application-service:subscription:update";
        pub const SUBSCRIPTION_DELETE: &str = "platform:application-service:subscription:delete";

        pub const ROLE_VIEW: &str = "platform:application-service:role:view";
        pub const ROLE_CREATE: &str = "platform:application-service:role:create";
        pub const ROLE_UPDATE: &str = "platform:application-service:role:update";
        pub const ROLE_DELETE: &str = "platform:application-service:role:delete";

        pub const PERMISSION_VIEW: &str = "platform:application-service:permission:view";
        pub const PERMISSION_SYNC: &str = "platform:application-service:permission:sync";

        /// All application service permissions
        pub const ALL: &[&str] = &[
            EVENT_CREATE,
            EVENT_TYPE_VIEW, EVENT_TYPE_CREATE, EVENT_TYPE_UPDATE, EVENT_TYPE_DELETE,
            SUBSCRIPTION_VIEW, SUBSCRIPTION_CREATE, SUBSCRIPTION_UPDATE, SUBSCRIPTION_DELETE,
            ROLE_VIEW, ROLE_CREATE, ROLE_UPDATE, ROLE_DELETE,
            PERMISSION_VIEW, PERMISSION_SYNC,
        ];
    }

    /// Superuser permission (grants all access)
    pub const ADMIN_ALL: &str = "*:*";
}

/// Built-in platform roles
pub mod roles {
    use super::*;

    /// Platform super admin - full access to everything
    pub fn super_admin() -> AuthRole {
        AuthRole::new("platform", "super-admin", "Platform Super Administrator")
            .with_description("Full access to all platform features")
            .with_permission(permissions::ADMIN_ALL)
            .with_source(RoleSource::Code)
    }

    /// IAM admin - manages users, roles, permissions
    pub fn iam_admin() -> AuthRole {
        let mut role = AuthRole::new("platform", "iam-admin", "IAM Administrator")
            .with_description("Manages users, roles, permissions, and service accounts")
            .with_source(RoleSource::Code);
        for p in permissions::iam::ALL {
            role.permissions.insert((*p).to_string());
        }
        role
    }

    /// Platform admin - manages clients, applications, config
    pub fn platform_admin() -> AuthRole {
        let mut role = AuthRole::new("platform", "platform-admin", "Platform Administrator")
            .with_description("Manages clients, applications, and platform configuration")
            .with_source(RoleSource::Code);
        for p in permissions::admin::ALL {
            role.permissions.insert((*p).to_string());
        }
        role.permissions.insert(permissions::iam::IDP_MANAGE.to_string());
        role
    }

    /// Messaging admin - manages event types, subscriptions, dispatch
    pub fn messaging_admin() -> AuthRole {
        let mut role = AuthRole::new("platform", "messaging-admin", "Messaging Administrator")
            .with_description("Manages event types, subscriptions, and dispatch jobs")
            .with_source(RoleSource::Code);
        for p in permissions::messaging::ALL {
            role.permissions.insert((*p).to_string());
        }
        role
    }

    /// Application service - auto-assigned to application service accounts
    pub fn application_service() -> AuthRole {
        let mut role = AuthRole::new("platform", "application-service", "Application Service Account")
            .with_description("Permissions for application service accounts (scoped to own application)")
            .with_source(RoleSource::Code);
        for p in permissions::application_service::ALL {
            role.permissions.insert((*p).to_string());
        }
        role
    }

    /// Get all built-in roles
    pub fn all() -> Vec<AuthRole> {
        vec![
            super_admin(),
            iam_admin(),
            platform_admin(),
            messaging_admin(),
            application_service(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_matching() {
        let role = AuthRole::new("platform", "admin", "Platform Admin")
            .with_permission(permissions::admin::CLIENT_VIEW)
            .with_permission(permissions::admin::CLIENT_CREATE)
            .with_permission("platform:iam:*");

        assert!(role.has_permission(permissions::admin::CLIENT_VIEW));
        assert!(role.has_permission(permissions::admin::CLIENT_CREATE));
        assert!(!role.has_permission(permissions::admin::CLIENT_DELETE));

        // Wildcard matching
        assert!(role.has_permission(permissions::iam::USER_VIEW));
        assert!(role.has_permission(permissions::iam::ROLE_CREATE));
    }

    #[test]
    fn test_superuser_permission() {
        let role = roles::super_admin();

        assert!(role.has_permission(permissions::admin::CLIENT_VIEW));
        assert!(role.has_permission(permissions::iam::USER_DELETE));
        assert!(role.has_permission(permissions::messaging::EVENT_VIEW));
        assert!(role.has_permission("anything:everything"));
    }

    #[test]
    fn test_built_in_roles() {
        let all_roles = roles::all();
        assert_eq!(all_roles.len(), 5);

        // Super admin has wildcard
        let super_admin = roles::super_admin();
        assert!(super_admin.permissions.contains(permissions::ADMIN_ALL));

        // IAM admin has all IAM permissions
        let iam_admin = roles::iam_admin();
        assert!(iam_admin.has_permission(permissions::iam::USER_VIEW));
        assert!(iam_admin.has_permission(permissions::iam::ROLE_DELETE));
        assert!(!iam_admin.has_permission(permissions::admin::CLIENT_VIEW));

        // Messaging admin has all messaging permissions
        let messaging_admin = roles::messaging_admin();
        assert!(messaging_admin.has_permission(permissions::messaging::EVENT_VIEW));
        assert!(messaging_admin.has_permission(permissions::messaging::DISPATCH_JOB_RETRY));
    }
}

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
            id: crate::TsidGenerator::generate(crate::EntityType::Role),
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

    /// Check for wildcard permissions using 4-level pattern matching
    /// Pattern format: subdomain:context:aggregate:action
    /// Each level can independently use '*' to match any value
    fn has_wildcard_permission(&self, permission: &str) -> bool {
        for pattern in &self.permissions {
            if matches_pattern(permission, pattern) {
                return true;
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

/// Match a required permission against a pattern (4-level: subdomain:context:aggregate:action).
/// Each level in the pattern can be '*' to match any value at that level.
pub fn matches_pattern(permission: &str, pattern: &str) -> bool {
    let perm_parts: Vec<&str> = permission.split(':').collect();
    let pat_parts: Vec<&str> = pattern.split(':').collect();

    // Both must have exactly 4 parts
    if perm_parts.len() != 4 || pat_parts.len() != 4 {
        return false;
    }

    for i in 0..4 {
        if pat_parts[i] != "*" && pat_parts[i] != perm_parts[i] {
            return false;
        }
    }

    true
}

/// Platform permissions - 4-level format: platform:{context}:{aggregate}:{action}
/// Matches TypeScript PermissionRegistry for cross-platform compatibility
pub mod permissions {
    /// Platform Admin context — clients, applications, events, subscriptions, dispatch, etc.
    pub mod admin {
        // Client management
        pub const CLIENT_READ: &str = "platform:admin:client:read";
        pub const CLIENT_CREATE: &str = "platform:admin:client:create";
        pub const CLIENT_UPDATE: &str = "platform:admin:client:update";
        pub const CLIENT_DELETE: &str = "platform:admin:client:delete";
        pub const CLIENT_MANAGE: &str = "platform:admin:client:manage";
        pub const CLIENT_ACTIVATE: &str = "platform:admin:client:activate";
        pub const CLIENT_SUSPEND: &str = "platform:admin:client:suspend";
        pub const CLIENT_DEACTIVATE: &str = "platform:admin:client:deactivate";

        // Anchor domain management
        pub const ANCHOR_DOMAIN_READ: &str = "platform:admin:anchor-domain:read";
        pub const ANCHOR_DOMAIN_CREATE: &str = "platform:admin:anchor-domain:create";
        pub const ANCHOR_DOMAIN_UPDATE: &str = "platform:admin:anchor-domain:update";
        pub const ANCHOR_DOMAIN_DELETE: &str = "platform:admin:anchor-domain:delete";
        pub const ANCHOR_DOMAIN_MANAGE: &str = "platform:admin:anchor-domain:manage";

        // Application management
        pub const APPLICATION_READ: &str = "platform:admin:application:read";
        pub const APPLICATION_CREATE: &str = "platform:admin:application:create";
        pub const APPLICATION_UPDATE: &str = "platform:admin:application:update";
        pub const APPLICATION_DELETE: &str = "platform:admin:application:delete";
        pub const APPLICATION_MANAGE: &str = "platform:admin:application:manage";
        pub const APPLICATION_ACTIVATE: &str = "platform:admin:application:activate";
        pub const APPLICATION_DEACTIVATE: &str = "platform:admin:application:deactivate";
        pub const APPLICATION_ENABLE_CLIENT: &str = "platform:admin:application:enable-client";
        pub const APPLICATION_DISABLE_CLIENT: &str = "platform:admin:application:disable-client";

        // Event type management
        pub const EVENT_TYPE_READ: &str = "platform:admin:event-type:read";
        pub const EVENT_TYPE_CREATE: &str = "platform:admin:event-type:create";
        pub const EVENT_TYPE_UPDATE: &str = "platform:admin:event-type:update";
        pub const EVENT_TYPE_DELETE: &str = "platform:admin:event-type:delete";
        pub const EVENT_TYPE_MANAGE: &str = "platform:admin:event-type:manage";
        pub const EVENT_TYPE_ARCHIVE: &str = "platform:admin:event-type:archive";
        pub const EVENT_TYPE_MANAGE_SCHEMA: &str = "platform:admin:event-type:manage-schema";
        pub const EVENT_TYPE_SYNC: &str = "platform:admin:event-type:sync";

        // Dispatch pool management
        pub const DISPATCH_POOL_READ: &str = "platform:admin:dispatch-pool:read";
        pub const DISPATCH_POOL_CREATE: &str = "platform:admin:dispatch-pool:create";
        pub const DISPATCH_POOL_UPDATE: &str = "platform:admin:dispatch-pool:update";
        pub const DISPATCH_POOL_DELETE: &str = "platform:admin:dispatch-pool:delete";
        pub const DISPATCH_POOL_MANAGE: &str = "platform:admin:dispatch-pool:manage";
        pub const DISPATCH_POOL_SYNC: &str = "platform:admin:dispatch-pool:sync";

        // Connection management
        pub const CONNECTION_READ: &str = "platform:admin:connection:read";
        pub const CONNECTION_CREATE: &str = "platform:admin:connection:create";
        pub const CONNECTION_UPDATE: &str = "platform:admin:connection:update";
        pub const CONNECTION_DELETE: &str = "platform:admin:connection:delete";
        pub const CONNECTION_MANAGE: &str = "platform:admin:connection:manage";

        // Subscription management
        pub const SUBSCRIPTION_READ: &str = "platform:admin:subscription:read";
        pub const SUBSCRIPTION_CREATE: &str = "platform:admin:subscription:create";
        pub const SUBSCRIPTION_UPDATE: &str = "platform:admin:subscription:update";
        pub const SUBSCRIPTION_DELETE: &str = "platform:admin:subscription:delete";
        pub const SUBSCRIPTION_MANAGE: &str = "platform:admin:subscription:manage";
        pub const SUBSCRIPTION_SYNC: &str = "platform:admin:subscription:sync";

        // Event read access
        pub const EVENT_READ: &str = "platform:admin:event:read";
        pub const EVENT_VIEW_RAW: &str = "platform:admin:event:view-raw";

        // Dispatch job read access
        pub const DISPATCH_JOB_READ: &str = "platform:admin:dispatch-job:read";
        pub const DISPATCH_JOB_VIEW_RAW: &str = "platform:admin:dispatch-job:view-raw";

        // Identity provider management
        pub const IDENTITY_PROVIDER_READ: &str = "platform:admin:identity-provider:read";
        pub const IDENTITY_PROVIDER_CREATE: &str = "platform:admin:identity-provider:create";
        pub const IDENTITY_PROVIDER_UPDATE: &str = "platform:admin:identity-provider:update";
        pub const IDENTITY_PROVIDER_DELETE: &str = "platform:admin:identity-provider:delete";
        pub const IDENTITY_PROVIDER_MANAGE: &str = "platform:admin:identity-provider:manage";

        // Email domain mapping management
        pub const EMAIL_DOMAIN_MAPPING_READ: &str = "platform:admin:email-domain-mapping:read";
        pub const EMAIL_DOMAIN_MAPPING_CREATE: &str = "platform:admin:email-domain-mapping:create";
        pub const EMAIL_DOMAIN_MAPPING_UPDATE: &str = "platform:admin:email-domain-mapping:update";
        pub const EMAIL_DOMAIN_MAPPING_DELETE: &str = "platform:admin:email-domain-mapping:delete";
        pub const EMAIL_DOMAIN_MAPPING_MANAGE: &str = "platform:admin:email-domain-mapping:manage";

        // Service account management
        pub const SERVICE_ACCOUNT_READ: &str = "platform:admin:service-account:read";
        pub const SERVICE_ACCOUNT_CREATE: &str = "platform:admin:service-account:create";
        pub const SERVICE_ACCOUNT_UPDATE: &str = "platform:admin:service-account:update";
        pub const SERVICE_ACCOUNT_DELETE: &str = "platform:admin:service-account:delete";
        pub const SERVICE_ACCOUNT_MANAGE: &str = "platform:admin:service-account:manage";

        // CORS origin management
        pub const CORS_ORIGIN_READ: &str = "platform:admin:cors-origin:read";
        pub const CORS_ORIGIN_CREATE: &str = "platform:admin:cors-origin:create";
        pub const CORS_ORIGIN_DELETE: &str = "platform:admin:cors-origin:delete";
        pub const CORS_ORIGIN_MANAGE: &str = "platform:admin:cors-origin:manage";

        // Login attempt & audit
        pub const LOGIN_ATTEMPT_READ: &str = "platform:admin:login-attempt:read";
        pub const AUDIT_LOG_READ: &str = "platform:admin:audit-log:read";
        pub const AUDIT_LOG_EXPORT: &str = "platform:admin:audit-log:export";

        // Batch operations
        pub const BATCH_EVENTS_WRITE: &str = "platform:admin:batch:events-write";
        pub const BATCH_DISPATCH_JOBS_WRITE: &str = "platform:admin:batch:dispatch-jobs-write";
        pub const BATCH_AUDIT_LOGS_WRITE: &str = "platform:admin:batch:audit-logs-write";

        /// All admin permissions
        pub const ALL: &[&str] = &[
            CLIENT_READ, CLIENT_CREATE, CLIENT_UPDATE, CLIENT_DELETE, CLIENT_MANAGE,
            CLIENT_ACTIVATE, CLIENT_SUSPEND, CLIENT_DEACTIVATE,
            ANCHOR_DOMAIN_READ, ANCHOR_DOMAIN_CREATE, ANCHOR_DOMAIN_UPDATE, ANCHOR_DOMAIN_DELETE, ANCHOR_DOMAIN_MANAGE,
            APPLICATION_READ, APPLICATION_CREATE, APPLICATION_UPDATE, APPLICATION_DELETE, APPLICATION_MANAGE,
            APPLICATION_ACTIVATE, APPLICATION_DEACTIVATE, APPLICATION_ENABLE_CLIENT, APPLICATION_DISABLE_CLIENT,
            EVENT_TYPE_READ, EVENT_TYPE_CREATE, EVENT_TYPE_UPDATE, EVENT_TYPE_DELETE, EVENT_TYPE_MANAGE,
            EVENT_TYPE_ARCHIVE, EVENT_TYPE_MANAGE_SCHEMA, EVENT_TYPE_SYNC,
            DISPATCH_POOL_READ, DISPATCH_POOL_CREATE, DISPATCH_POOL_UPDATE, DISPATCH_POOL_DELETE, DISPATCH_POOL_MANAGE, DISPATCH_POOL_SYNC,
            CONNECTION_READ, CONNECTION_CREATE, CONNECTION_UPDATE, CONNECTION_DELETE, CONNECTION_MANAGE,
            SUBSCRIPTION_READ, SUBSCRIPTION_CREATE, SUBSCRIPTION_UPDATE, SUBSCRIPTION_DELETE, SUBSCRIPTION_MANAGE, SUBSCRIPTION_SYNC,
            EVENT_READ, EVENT_VIEW_RAW,
            DISPATCH_JOB_READ, DISPATCH_JOB_VIEW_RAW,
            IDENTITY_PROVIDER_READ, IDENTITY_PROVIDER_CREATE, IDENTITY_PROVIDER_UPDATE, IDENTITY_PROVIDER_DELETE, IDENTITY_PROVIDER_MANAGE,
            EMAIL_DOMAIN_MAPPING_READ, EMAIL_DOMAIN_MAPPING_CREATE, EMAIL_DOMAIN_MAPPING_UPDATE, EMAIL_DOMAIN_MAPPING_DELETE, EMAIL_DOMAIN_MAPPING_MANAGE,
            SERVICE_ACCOUNT_READ, SERVICE_ACCOUNT_CREATE, SERVICE_ACCOUNT_UPDATE, SERVICE_ACCOUNT_DELETE, SERVICE_ACCOUNT_MANAGE,
            CORS_ORIGIN_READ, CORS_ORIGIN_CREATE, CORS_ORIGIN_DELETE, CORS_ORIGIN_MANAGE,
            LOGIN_ATTEMPT_READ,
            AUDIT_LOG_READ, AUDIT_LOG_EXPORT,
            BATCH_EVENTS_WRITE, BATCH_DISPATCH_JOBS_WRITE, BATCH_AUDIT_LOGS_WRITE,
        ];
    }

    /// IAM context — users, roles, access control
    pub mod iam {
        // User management
        pub const USER_READ: &str = "platform:iam:user:read";
        pub const USER_CREATE: &str = "platform:iam:user:create";
        pub const USER_UPDATE: &str = "platform:iam:user:update";
        pub const USER_DELETE: &str = "platform:iam:user:delete";
        pub const USER_MANAGE: &str = "platform:iam:user:manage";
        pub const USER_ACTIVATE: &str = "platform:iam:user:activate";
        pub const USER_DEACTIVATE: &str = "platform:iam:user:deactivate";
        pub const USER_ASSIGN_ROLES: &str = "platform:iam:user:assign-roles";

        // Role management
        pub const ROLE_READ: &str = "platform:iam:role:read";
        pub const ROLE_CREATE: &str = "platform:iam:role:create";
        pub const ROLE_UPDATE: &str = "platform:iam:role:update";
        pub const ROLE_DELETE: &str = "platform:iam:role:delete";
        pub const ROLE_MANAGE: &str = "platform:iam:role:manage";

        // Client access grants
        pub const CLIENT_ACCESS_GRANT: &str = "platform:iam:client-access:grant";
        pub const CLIENT_ACCESS_REVOKE: &str = "platform:iam:client-access:revoke";
        pub const CLIENT_ACCESS_READ: &str = "platform:iam:client-access:read";

        // Permission read
        pub const PERMISSION_READ: &str = "platform:iam:permission:read";

        // Auth config
        pub const AUTH_CONFIG_READ: &str = "platform:iam:auth-config:read";
        pub const AUTH_CONFIG_CREATE: &str = "platform:iam:auth-config:create";
        pub const AUTH_CONFIG_UPDATE: &str = "platform:iam:auth-config:update";
        pub const AUTH_CONFIG_DELETE: &str = "platform:iam:auth-config:delete";
        pub const AUTH_CONFIG_MANAGE: &str = "platform:iam:auth-config:manage";

        /// All IAM permissions
        pub const ALL: &[&str] = &[
            USER_READ, USER_CREATE, USER_UPDATE, USER_DELETE, USER_MANAGE,
            USER_ACTIVATE, USER_DEACTIVATE, USER_ASSIGN_ROLES,
            ROLE_READ, ROLE_CREATE, ROLE_UPDATE, ROLE_DELETE, ROLE_MANAGE,
            CLIENT_ACCESS_GRANT, CLIENT_ACCESS_REVOKE, CLIENT_ACCESS_READ,
            PERMISSION_READ,
            AUTH_CONFIG_READ, AUTH_CONFIG_CREATE, AUTH_CONFIG_UPDATE, AUTH_CONFIG_DELETE, AUTH_CONFIG_MANAGE,
        ];
    }

    /// Auth context — OAuth clients, client auth configs
    pub mod auth {
        // Client auth config
        pub const CLIENT_AUTH_CONFIG_READ: &str = "platform:auth:client-auth-config:read";
        pub const CLIENT_AUTH_CONFIG_CREATE: &str = "platform:auth:client-auth-config:create";
        pub const CLIENT_AUTH_CONFIG_UPDATE: &str = "platform:auth:client-auth-config:update";
        pub const CLIENT_AUTH_CONFIG_DELETE: &str = "platform:auth:client-auth-config:delete";
        pub const CLIENT_AUTH_CONFIG_MANAGE: &str = "platform:auth:client-auth-config:manage";

        // OAuth client management
        pub const OAUTH_CLIENT_READ: &str = "platform:auth:oauth-client:read";
        pub const OAUTH_CLIENT_CREATE: &str = "platform:auth:oauth-client:create";
        pub const OAUTH_CLIENT_UPDATE: &str = "platform:auth:oauth-client:update";
        pub const OAUTH_CLIENT_DELETE: &str = "platform:auth:oauth-client:delete";
        pub const OAUTH_CLIENT_MANAGE: &str = "platform:auth:oauth-client:manage";
        pub const OAUTH_CLIENT_REGENERATE_SECRET: &str = "platform:auth:oauth-client:regenerate-secret";

        /// All auth permissions
        pub const ALL: &[&str] = &[
            CLIENT_AUTH_CONFIG_READ, CLIENT_AUTH_CONFIG_CREATE, CLIENT_AUTH_CONFIG_UPDATE,
            CLIENT_AUTH_CONFIG_DELETE, CLIENT_AUTH_CONFIG_MANAGE,
            OAUTH_CLIENT_READ, OAUTH_CLIENT_CREATE, OAUTH_CLIENT_UPDATE,
            OAUTH_CLIENT_DELETE, OAUTH_CLIENT_MANAGE, OAUTH_CLIENT_REGENERATE_SECRET,
        ];
    }

    /// Application Service permissions (scoped to own application via SDK)
    pub mod application_service {
        pub const EVENT_CREATE: &str = "platform:application-service:event:create";

        pub const EVENT_TYPE_READ: &str = "platform:application-service:event-type:read";
        pub const EVENT_TYPE_CREATE: &str = "platform:application-service:event-type:create";
        pub const EVENT_TYPE_UPDATE: &str = "platform:application-service:event-type:update";
        pub const EVENT_TYPE_DELETE: &str = "platform:application-service:event-type:delete";

        pub const SUBSCRIPTION_READ: &str = "platform:application-service:subscription:read";
        pub const SUBSCRIPTION_CREATE: &str = "platform:application-service:subscription:create";
        pub const SUBSCRIPTION_UPDATE: &str = "platform:application-service:subscription:update";
        pub const SUBSCRIPTION_DELETE: &str = "platform:application-service:subscription:delete";

        pub const ROLE_READ: &str = "platform:application-service:role:read";
        pub const ROLE_CREATE: &str = "platform:application-service:role:create";
        pub const ROLE_UPDATE: &str = "platform:application-service:role:update";
        pub const ROLE_DELETE: &str = "platform:application-service:role:delete";

        pub const PERMISSION_READ: &str = "platform:application-service:permission:read";
        pub const PERMISSION_SYNC: &str = "platform:application-service:permission:sync";

        /// All application service permissions
        pub const ALL: &[&str] = &[
            EVENT_CREATE,
            EVENT_TYPE_READ, EVENT_TYPE_CREATE, EVENT_TYPE_UPDATE, EVENT_TYPE_DELETE,
            SUBSCRIPTION_READ, SUBSCRIPTION_CREATE, SUBSCRIPTION_UPDATE, SUBSCRIPTION_DELETE,
            ROLE_READ, ROLE_CREATE, ROLE_UPDATE, ROLE_DELETE,
            PERMISSION_READ, PERMISSION_SYNC,
        ];
    }

    /// Superuser permission (grants all platform access)
    pub const ADMIN_ALL: &str = "platform:*:*:*";

    // =========================================================================
    // Backward-compatibility aliases (old `messaging::` and `iam::VIEW` names)
    // These allow existing code to compile while we migrate references.
    // =========================================================================
    pub mod messaging {
        pub use super::admin::EVENT_READ as EVENT_VIEW;
        pub use super::admin::EVENT_VIEW_RAW;
        pub use super::admin::EVENT_TYPE_READ as EVENT_TYPE_VIEW;
        pub use super::admin::EVENT_TYPE_CREATE;
        pub use super::admin::EVENT_TYPE_UPDATE;
        pub use super::admin::EVENT_TYPE_DELETE;
        pub use super::admin::SUBSCRIPTION_READ as SUBSCRIPTION_VIEW;
        pub use super::admin::SUBSCRIPTION_CREATE;
        pub use super::admin::SUBSCRIPTION_UPDATE;
        pub use super::admin::SUBSCRIPTION_DELETE;
        pub use super::admin::DISPATCH_JOB_READ as DISPATCH_JOB_VIEW;
        pub use super::admin::DISPATCH_JOB_VIEW_RAW;
        pub use super::admin::DISPATCH_POOL_READ as DISPATCH_POOL_VIEW;
        pub use super::admin::DISPATCH_POOL_CREATE;
        pub use super::admin::DISPATCH_POOL_UPDATE;
        pub use super::admin::DISPATCH_POOL_DELETE;
        // These don't have direct equivalents in admin — provide stubs
        pub const EVENT_CREATE: &str = "platform:admin:event:create";
        pub const DISPATCH_JOB_CREATE: &str = "platform:admin:dispatch-job:create";
        pub const DISPATCH_JOB_RETRY: &str = "platform:admin:dispatch-job:retry";
    }
}

/// Built-in platform roles (matches TypeScript role definitions)
pub mod roles {
    use super::*;

    /// PLATFORM_SUPER_ADMIN — full access to all platform operations
    pub fn super_admin() -> AuthRole {
        AuthRole::new("platform", "super-admin", "Platform Super Admin")
            .with_description("Full access to all platform operations")
            .with_permission(permissions::ADMIN_ALL)
            .with_source(RoleSource::Code)
    }

    /// PLATFORM_ADMIN — manages clients, applications, and platform configuration
    pub fn platform_admin() -> AuthRole {
        AuthRole::new("platform", "platform-admin", "Platform Admin")
            .with_description("Manages clients, applications, and platform configuration")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::CLIENT_READ, permissions::admin::CLIENT_CREATE,
                permissions::admin::CLIENT_UPDATE, permissions::admin::CLIENT_ACTIVATE,
                permissions::admin::CLIENT_SUSPEND, permissions::admin::CLIENT_DEACTIVATE,
                permissions::admin::ANCHOR_DOMAIN_READ, permissions::admin::ANCHOR_DOMAIN_CREATE,
                permissions::admin::ANCHOR_DOMAIN_UPDATE, permissions::admin::ANCHOR_DOMAIN_DELETE,
                permissions::admin::APPLICATION_READ, permissions::admin::APPLICATION_CREATE,
                permissions::admin::APPLICATION_UPDATE, permissions::admin::APPLICATION_DELETE,
                permissions::admin::APPLICATION_ENABLE_CLIENT, permissions::admin::APPLICATION_DISABLE_CLIENT,
                permissions::admin::AUDIT_LOG_READ, permissions::admin::AUDIT_LOG_EXPORT,
                permissions::admin::LOGIN_ATTEMPT_READ,
            ])
    }

    /// PLATFORM_ADMIN_READONLY — view-only access to clients, applications, config
    pub fn platform_admin_readonly() -> AuthRole {
        AuthRole::new("platform", "platform-admin-readonly", "Platform Admin Read-Only")
            .with_description("View-only access to clients, applications, and platform configuration")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::CLIENT_READ,
                permissions::admin::ANCHOR_DOMAIN_READ,
                permissions::admin::APPLICATION_READ,
                permissions::admin::AUDIT_LOG_READ,
                permissions::admin::LOGIN_ATTEMPT_READ,
            ])
    }

    /// PLATFORM_IAM_ADMIN — manages users, roles, and access control
    pub fn iam_admin() -> AuthRole {
        AuthRole::new("platform", "iam-admin", "Platform IAM Admin")
            .with_description("Manages users, roles, and access control")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::iam::USER_READ, permissions::iam::USER_CREATE,
                permissions::iam::USER_UPDATE, permissions::iam::USER_DELETE,
                permissions::iam::USER_ACTIVATE, permissions::iam::USER_DEACTIVATE,
                permissions::iam::USER_ASSIGN_ROLES,
                permissions::iam::ROLE_READ, permissions::iam::ROLE_CREATE,
                permissions::iam::ROLE_UPDATE, permissions::iam::ROLE_DELETE,
                permissions::iam::CLIENT_ACCESS_GRANT, permissions::iam::CLIENT_ACCESS_REVOKE,
                permissions::iam::CLIENT_ACCESS_READ,
            ])
    }

    /// PLATFORM_IAM_READONLY — view-only access to users and roles
    pub fn iam_readonly() -> AuthRole {
        AuthRole::new("platform", "iam-readonly", "Platform IAM Read-Only")
            .with_description("View-only access to users and roles")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::iam::USER_READ,
                permissions::iam::ROLE_READ,
                permissions::iam::CLIENT_ACCESS_READ,
            ])
    }

    /// PLATFORM_AUTH_ADMIN — manages authentication configuration
    pub fn auth_admin() -> AuthRole {
        AuthRole::new("platform", "auth-admin", "Platform Auth Admin")
            .with_description("Manages authentication configuration")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::auth::CLIENT_AUTH_CONFIG_READ, permissions::auth::CLIENT_AUTH_CONFIG_CREATE,
                permissions::auth::CLIENT_AUTH_CONFIG_UPDATE, permissions::auth::CLIENT_AUTH_CONFIG_DELETE,
                permissions::auth::OAUTH_CLIENT_READ, permissions::auth::OAUTH_CLIENT_CREATE,
                permissions::auth::OAUTH_CLIENT_UPDATE, permissions::auth::OAUTH_CLIENT_DELETE,
                permissions::auth::OAUTH_CLIENT_REGENERATE_SECRET,
            ])
    }

    /// PLATFORM_AUTH_READONLY — view-only access to auth configuration
    pub fn auth_readonly() -> AuthRole {
        AuthRole::new("platform", "auth-readonly", "Platform Auth Read-Only")
            .with_description("View-only access to authentication configuration")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::auth::CLIENT_AUTH_CONFIG_READ,
                permissions::auth::OAUTH_CLIENT_READ,
            ])
    }

    /// PLATFORM_AI_AGENT_READONLY — read-only for AI agent integrations
    pub fn ai_agent_readonly() -> AuthRole {
        AuthRole::new("platform", "ai-agent-readonly", "AI Agent Read-Only")
            .with_description("Read-only access to event types and subscriptions for AI agent integrations")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::EVENT_TYPE_READ,
                permissions::admin::SUBSCRIPTION_READ,
            ])
    }

    /// Messaging admin — manages event types, subscriptions, dispatch
    pub fn messaging_admin() -> AuthRole {
        AuthRole::new("platform", "messaging-admin", "Messaging Administrator")
            .with_description("Manages event types, subscriptions, and dispatch jobs")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::EVENT_TYPE_READ, permissions::admin::EVENT_TYPE_CREATE,
                permissions::admin::EVENT_TYPE_UPDATE, permissions::admin::EVENT_TYPE_DELETE,
                permissions::admin::EVENT_TYPE_ARCHIVE, permissions::admin::EVENT_TYPE_MANAGE_SCHEMA,
                permissions::admin::EVENT_TYPE_SYNC,
                permissions::admin::SUBSCRIPTION_READ, permissions::admin::SUBSCRIPTION_CREATE,
                permissions::admin::SUBSCRIPTION_UPDATE, permissions::admin::SUBSCRIPTION_DELETE,
                permissions::admin::SUBSCRIPTION_SYNC,
                permissions::admin::DISPATCH_POOL_READ, permissions::admin::DISPATCH_POOL_CREATE,
                permissions::admin::DISPATCH_POOL_UPDATE, permissions::admin::DISPATCH_POOL_DELETE,
                permissions::admin::DISPATCH_POOL_SYNC,
                permissions::admin::CONNECTION_READ, permissions::admin::CONNECTION_CREATE,
                permissions::admin::CONNECTION_UPDATE, permissions::admin::CONNECTION_DELETE,
                permissions::admin::EVENT_READ, permissions::admin::EVENT_VIEW_RAW,
                permissions::admin::DISPATCH_JOB_READ, permissions::admin::DISPATCH_JOB_VIEW_RAW,
            ])
    }

    /// Application service — auto-assigned to application service accounts
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
            platform_admin(),
            platform_admin_readonly(),
            iam_admin(),
            iam_readonly(),
            auth_admin(),
            auth_readonly(),
            ai_agent_readonly(),
            messaging_admin(),
            application_service(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_pattern() {
        // Exact match
        assert!(matches_pattern("platform:admin:client:read", "platform:admin:client:read"));
        // Action wildcard
        assert!(matches_pattern("platform:admin:client:read", "platform:admin:client:*"));
        // Aggregate + action wildcard
        assert!(matches_pattern("platform:admin:client:read", "platform:admin:*:*"));
        // Full wildcard
        assert!(matches_pattern("platform:admin:client:read", "platform:*:*:*"));
        // Non-match
        assert!(!matches_pattern("platform:admin:client:read", "platform:iam:client:read"));
        // Wrong part count
        assert!(!matches_pattern("platform:admin:client:read", "platform:admin:*"));
        assert!(!matches_pattern("platform:admin", "platform:admin:*:*"));
    }

    #[test]
    fn test_permission_matching() {
        let role = AuthRole::new("platform", "admin", "Platform Admin")
            .with_permission(permissions::admin::CLIENT_READ)
            .with_permission(permissions::admin::CLIENT_CREATE)
            .with_permission("platform:iam:*:*");

        assert!(role.has_permission(permissions::admin::CLIENT_READ));
        assert!(role.has_permission(permissions::admin::CLIENT_CREATE));
        assert!(!role.has_permission(permissions::admin::CLIENT_DELETE));

        // Wildcard matching (4-level)
        assert!(role.has_permission(permissions::iam::USER_READ));
        assert!(role.has_permission(permissions::iam::ROLE_CREATE));
    }

    #[test]
    fn test_superuser_permission() {
        let role = roles::super_admin();

        assert!(role.has_permission(permissions::admin::CLIENT_READ));
        assert!(role.has_permission(permissions::iam::USER_DELETE));
        assert!(role.has_permission(permissions::admin::EVENT_READ));
        assert!(role.has_permission(permissions::auth::OAUTH_CLIENT_READ));
        // platform:*:*:* matches any platform permission
        assert!(role.has_permission("platform:anything:everything:here"));
    }

    #[test]
    fn test_built_in_roles() {
        let all_roles = roles::all();
        assert_eq!(all_roles.len(), 10);

        // Super admin has wildcard
        let super_admin = roles::super_admin();
        assert!(super_admin.permissions.contains(permissions::ADMIN_ALL));

        // IAM admin has IAM permissions but not admin
        let iam_admin = roles::iam_admin();
        assert!(iam_admin.has_permission(permissions::iam::USER_READ));
        assert!(iam_admin.has_permission(permissions::iam::ROLE_DELETE));
        assert!(!iam_admin.has_permission(permissions::admin::CLIENT_READ));

        // Read-only roles
        let iam_ro = roles::iam_readonly();
        assert!(iam_ro.has_permission(permissions::iam::USER_READ));
        assert!(!iam_ro.has_permission(permissions::iam::USER_CREATE));

        let platform_ro = roles::platform_admin_readonly();
        assert!(platform_ro.has_permission(permissions::admin::CLIENT_READ));
        assert!(!platform_ro.has_permission(permissions::admin::CLIENT_CREATE));

        let auth_ro = roles::auth_readonly();
        assert!(auth_ro.has_permission(permissions::auth::OAUTH_CLIENT_READ));
        assert!(!auth_ro.has_permission(permissions::auth::OAUTH_CLIENT_CREATE));

        let ai_ro = roles::ai_agent_readonly();
        assert!(ai_ro.has_permission(permissions::admin::EVENT_TYPE_READ));
        assert!(!ai_ro.has_permission(permissions::admin::EVENT_TYPE_CREATE));
    }
}

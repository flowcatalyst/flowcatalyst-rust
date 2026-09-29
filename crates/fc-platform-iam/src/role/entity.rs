//! Role and Permission Entities
//!
//! Authorization model for role-based access control.

use chrono::{DateTime, Utc};
use fc_platform_core::shared::tsid;
use fc_platform_core::shared::tsid::EntityType;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Role source - where the role definition came from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum RoleSource {
    /// Defined in code (cannot be modified)
    Code,
    /// Defined in database (can be modified)
    #[default]
    Database,
    /// Synced from external SDK/IDP
    Sdk,
}

fc_platform_core::shared::enum_str::str_enum!(RoleSource, "role source", {
    Code => "CODE",
    Database => "DATABASE",
    Sdk => "SDK",
});

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
            id: tsid::generate(EntityType::Role),
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

    pub fn with_permissions(
        mut self,
        permissions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
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

    /// The application this role belongs to: its `application_code`, or,
    /// on a legacy row without one, the first segment of its name.
    pub fn owning_application_code(&self) -> &str {
        if !self.application_code.trim().is_empty() {
            &self.application_code
        } else {
            self.name.split(':').next().unwrap_or(&self.name)
        }
    }

    /// Extract short role name from full name
    pub fn role_name(&self) -> &str {
        self.name.split(':').nth(1).unwrap_or(&self.name)
    }
}

/// The permissions in `permissions` that don't belong to `application_code`:
/// a permission's first segment names its application, and a role may hold
/// only its own application's (owner ruling 15; Java S1.5, a120e236). In
/// order, without repeats.
pub fn permissions_outside_application<'a>(
    application_code: &str,
    permissions: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut outside: Vec<String> = Vec::new();
    for p in permissions {
        let first = p.split(':').next().unwrap_or_default();
        if first != application_code && !outside.iter().any(|o| o == p) {
            outside.push(p.to_string());
        }
    }
    outside
}

pub use fc_platform_core::permissions::{self, matches_pattern};

/// Built-in platform roles. Go's catalogue is the reference
/// (flowcatalyst-go internal/platform/seed/roles.go `PlatformRoles`): the
/// same roles, names, display names, descriptions and permission sets.
/// The function-runner roles and grants are Rust additions, from Java.
/// `role_catalogue_go_parity_test` pins this.
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
        AuthRole::new("platform", "admin", "Platform Admin")
            .with_description("Manages clients, applications, and platform configuration")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::CLIENT_READ,
                permissions::admin::CLIENT_CREATE,
                permissions::admin::CLIENT_UPDATE,
                permissions::admin::CLIENT_ACTIVATE,
                permissions::admin::CLIENT_SUSPEND,
                permissions::admin::CLIENT_DEACTIVATE,
                permissions::admin::ANCHOR_DOMAIN_READ,
                permissions::admin::ANCHOR_DOMAIN_CREATE,
                permissions::admin::ANCHOR_DOMAIN_UPDATE,
                permissions::admin::ANCHOR_DOMAIN_DELETE,
                permissions::admin::APPLICATION_READ,
                permissions::admin::APPLICATION_CREATE,
                permissions::admin::APPLICATION_UPDATE,
                permissions::admin::APPLICATION_DELETE,
                permissions::admin::APPLICATION_ENABLE_CLIENT,
                permissions::admin::APPLICATION_DISABLE_CLIENT,
                permissions::admin::AUDIT_LOG_READ,
                permissions::admin::AUDIT_LOG_EXPORT,
                permissions::admin::LOGIN_ATTEMPT_READ,
                permissions::admin::DOCS_READ,
                permissions::developer::APPLICATION_OPENAPI_MANAGE,
                permissions::admin::CONFIG_READ,
                permissions::admin::CONFIG_MANAGE,
                permissions::admin::CORS_ORIGIN_READ,
                permissions::admin::CORS_ORIGIN_CREATE,
                permissions::admin::CORS_ORIGIN_DELETE,
                // Owner ruling 13 (2026-09-25, Java 458ebf3a): service
                // accounts need these permissions, which no built-in role
                // but super-admin held. What the holder may then give an
                // account is bounded by the role ceiling.
                permissions::admin::SERVICE_ACCOUNT_READ,
                permissions::admin::SERVICE_ACCOUNT_CREATE,
                permissions::admin::SERVICE_ACCOUNT_UPDATE,
                permissions::admin::SERVICE_ACCOUNT_DELETE,
                permissions::admin::SERVICE_ACCOUNT_MANAGE,
            ])
    }

    /// PLATFORM_ADMIN_READONLY — view-only access to clients, applications, config
    pub fn platform_admin_readonly() -> AuthRole {
        AuthRole::new("platform", "admin-readonly", "Platform Admin Read-Only")
            .with_description(
                "View-only access to clients, applications, and platform configuration",
            )
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::CLIENT_READ,
                permissions::admin::ANCHOR_DOMAIN_READ,
                permissions::admin::APPLICATION_READ,
                permissions::admin::AUDIT_LOG_READ,
                permissions::admin::LOGIN_ATTEMPT_READ,
                permissions::admin::DOCS_READ,
                permissions::developer::APPLICATION_OPENAPI_VIEW,
                permissions::admin::CONFIG_READ,
                permissions::admin::CORS_ORIGIN_READ,
            ])
    }

    /// PLATFORM_IAM_ADMIN — manages users, roles, and access control
    pub fn iam_admin() -> AuthRole {
        AuthRole::new("platform", "iam-admin", "Platform IAM Admin")
            .with_description("Manages users, roles, and access control")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::iam::USER_READ,
                permissions::iam::USER_CREATE,
                permissions::iam::USER_UPDATE,
                permissions::iam::USER_DELETE,
                permissions::iam::USER_ACTIVATE,
                permissions::iam::USER_DEACTIVATE,
                permissions::iam::USER_ASSIGN_ROLES,
                permissions::iam::ROLE_READ,
                permissions::iam::ROLE_CREATE,
                permissions::iam::ROLE_UPDATE,
                permissions::iam::ROLE_DELETE,
                permissions::iam::CLIENT_ACCESS_GRANT,
                permissions::iam::CLIENT_ACCESS_REVOKE,
                permissions::iam::CLIENT_ACCESS_READ,
                permissions::admin::IDENTITY_PROVIDER_READ,
                permissions::admin::IDENTITY_PROVIDER_CREATE,
                permissions::admin::IDENTITY_PROVIDER_UPDATE,
                permissions::admin::IDENTITY_PROVIDER_DELETE,
                permissions::admin::EMAIL_DOMAIN_MAPPING_READ,
                permissions::admin::EMAIL_DOMAIN_MAPPING_CREATE,
                permissions::admin::EMAIL_DOMAIN_MAPPING_UPDATE,
                permissions::admin::EMAIL_DOMAIN_MAPPING_DELETE,
                // Owner ruling 13 (Java 458ebf3a), as `platform:admin`.
                permissions::admin::SERVICE_ACCOUNT_READ,
                permissions::admin::SERVICE_ACCOUNT_CREATE,
                permissions::admin::SERVICE_ACCOUNT_UPDATE,
                permissions::admin::SERVICE_ACCOUNT_DELETE,
                permissions::admin::SERVICE_ACCOUNT_MANAGE,
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
                permissions::admin::IDENTITY_PROVIDER_READ,
                permissions::admin::EMAIL_DOMAIN_MAPPING_READ,
                // Owner ruling 13 (Java 458ebf3a).
                permissions::admin::SERVICE_ACCOUNT_READ,
            ])
    }

    /// PLATFORM_CLIENT_ADMIN — delegated user management within the
    /// administrator's own client(s): iam-admin's user permissions without
    /// client-access grants or role authoring (Go seed/roles.go:89-102).
    /// Go also confines each action to the admin's clients and bounds role
    /// assignment to the client's own application roles; that enforcement
    /// lives in the principal routes, not here.
    pub fn client_admin() -> AuthRole {
        AuthRole::new("platform", "client-admin", "Client Administrator")
            .with_description("Manages users within the administrator's own client")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::iam::USER_READ,
                permissions::iam::USER_CREATE,
                permissions::iam::USER_UPDATE,
                permissions::iam::USER_DELETE,
                permissions::iam::USER_ACTIVATE,
                permissions::iam::USER_DEACTIVATE,
                permissions::iam::USER_ASSIGN_ROLES,
                permissions::iam::ROLE_READ,
            ])
    }

    /// PLATFORM_AUTH_ADMIN — manages authentication configuration
    pub fn auth_admin() -> AuthRole {
        AuthRole::new("platform", "auth-admin", "Platform Auth Admin")
            .with_description("Manages authentication configuration")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::auth::CLIENT_AUTH_CONFIG_READ,
                permissions::auth::CLIENT_AUTH_CONFIG_CREATE,
                permissions::auth::CLIENT_AUTH_CONFIG_UPDATE,
                permissions::auth::CLIENT_AUTH_CONFIG_DELETE,
                permissions::auth::OAUTH_CLIENT_READ,
                permissions::auth::OAUTH_CLIENT_CREATE,
                permissions::auth::OAUTH_CLIENT_UPDATE,
                permissions::auth::OAUTH_CLIENT_DELETE,
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
            .with_description(
                "Read-only access to event types and subscriptions for AI agent integrations",
            )
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::EVENT_TYPE_READ,
                permissions::admin::SUBSCRIPTION_READ,
            ])
    }

    /// Messaging admin — manages event types, subscriptions, dispatch, scheduled jobs
    pub fn messaging_admin() -> AuthRole {
        AuthRole::new("platform", "messaging-admin", "Messaging Administrator")
            .with_description(
                "Manages event types, subscriptions, dispatch jobs, and scheduled jobs",
            )
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::EVENT_TYPE_READ,
                permissions::admin::EVENT_TYPE_CREATE,
                permissions::admin::EVENT_TYPE_UPDATE,
                permissions::admin::EVENT_TYPE_DELETE,
                permissions::admin::EVENT_TYPE_ARCHIVE,
                permissions::admin::EVENT_TYPE_MANAGE_SCHEMA,
                permissions::admin::EVENT_TYPE_SYNC,
                permissions::admin::SUBSCRIPTION_READ,
                permissions::admin::SUBSCRIPTION_CREATE,
                permissions::admin::SUBSCRIPTION_UPDATE,
                permissions::admin::SUBSCRIPTION_DELETE,
                permissions::admin::SUBSCRIPTION_SYNC,
                permissions::admin::DISPATCH_POOL_READ,
                permissions::admin::DISPATCH_POOL_CREATE,
                permissions::admin::DISPATCH_POOL_UPDATE,
                permissions::admin::DISPATCH_POOL_DELETE,
                permissions::admin::DISPATCH_POOL_SYNC,
                permissions::admin::CONNECTION_READ,
                permissions::admin::CONNECTION_CREATE,
                permissions::admin::CONNECTION_UPDATE,
                permissions::admin::CONNECTION_DELETE,
                permissions::admin::CONNECTION_SYNC,
                permissions::admin::EVENT_READ,
                permissions::admin::EVENT_VIEW_RAW,
                permissions::admin::DISPATCH_JOB_READ,
                permissions::admin::DISPATCH_JOB_VIEW_RAW,
                permissions::admin::SCHEDULED_JOB_READ,
                permissions::admin::SCHEDULED_JOB_CREATE,
                permissions::admin::SCHEDULED_JOB_UPDATE,
                permissions::admin::SCHEDULED_JOB_DELETE,
                permissions::admin::SCHEDULED_JOB_PAUSE,
                permissions::admin::SCHEDULED_JOB_FIRE,
                permissions::admin::SCHEDULED_JOB_SYNC,
                permissions::admin::SCHEDULED_JOB_INSTANCE_READ,
                permissions::admin::PROCESS_READ,
                permissions::admin::PROCESS_CREATE,
                permissions::admin::PROCESS_UPDATE,
                permissions::admin::PROCESS_DELETE,
                permissions::admin::PROCESS_ARCHIVE,
                permissions::admin::PROCESS_SYNC,
                // Rust addition for the function runner (Java
                // PlatformRoles.java:149-150; Go has no functions): every
                // function grant except host control, which is
                // `function-host` alone.
                permissions::function::FUNCTION_VIEW,
                permissions::function::FUNCTION_MANAGE,
                permissions::function::FUNCTION_PUBLISH,
                permissions::function::FUNCTION_PROMOTE,
                permissions::function::FUNCTION_POLICY_MANAGE,
                permissions::function::FUNCTION_VERSION_INVOKE,
                permissions::function::FUNCTION_SECRET_MANAGE,
                permissions::function::FUNCTION_DOMAIN_MANAGE,
            ])
    }

    /// Platform viewer — read-only across IAM, admin, and messaging.
    /// Kept for compatibility with the legacy `platform:viewer` role.
    pub fn viewer() -> AuthRole {
        AuthRole::new("platform", "viewer", "Platform Viewer")
            .with_description("Read-only access across IAM, admin, and messaging")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::iam::USER_READ,
                permissions::iam::ROLE_READ,
                permissions::iam::CLIENT_ACCESS_READ,
                permissions::admin::CLIENT_READ,
                permissions::admin::APPLICATION_READ,
                permissions::admin::EVENT_READ,
                permissions::admin::EVENT_TYPE_READ,
                permissions::admin::SUBSCRIPTION_READ,
                permissions::admin::DISPATCH_JOB_READ,
                permissions::admin::DISPATCH_POOL_READ,
                permissions::admin::SCHEDULED_JOB_READ,
                permissions::admin::SCHEDULED_JOB_INSTANCE_READ,
                permissions::admin::PROCESS_READ,
                permissions::admin::AUDIT_LOG_READ,
                permissions::admin::LOGIN_ATTEMPT_READ,
                permissions::admin::IDENTITY_PROVIDER_READ,
                permissions::admin::EMAIL_DOMAIN_MAPPING_READ,
                permissions::admin::CONFIG_READ,
                permissions::admin::CORS_ORIGIN_READ,
                // Owner ruling 13 (Java 458ebf3a).
                permissions::admin::SERVICE_ACCOUNT_READ,
                // Owner ruling 2 (Java ad4231be): router monitoring reads.
                permissions::admin::ROUTER_VIEW,
            ])
    }

    /// PLATFORM_ROUTER — the deployed message router's own role: it fetches
    /// its configuration document and nothing else (Go seed/roles.go:
    /// 179-185). It does not grant calling the router's API: that is
    /// [`router_operator`] (owner ruling 2).
    pub fn router() -> AuthRole {
        AuthRole::new("platform", "router", "Router")
            .with_description("Fetches the dispatch router configuration")
            .with_source(RoleSource::Code)
            .with_permission(permissions::admin::DISPATCH_POOL_READ)
    }

    /// PLATFORM_PORTAL_ADMINISTRATOR — manages a client's portal users;
    /// client confinement comes from the holder's own client scope, not
    /// from the role (Go seed/roles.go:187-196).
    pub fn portal_administrator() -> AuthRole {
        AuthRole::new("platform", "portal-administrator", "Portal Administrator")
            .with_description(
                "Manage the client's portal users: invite, suspend, and remove portal identities",
            )
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::iam::PORTAL_USER_READ,
                permissions::iam::PORTAL_USER_MANAGE,
            ])
    }

    /// PLATFORM_DEVELOPER — read-only access to application API documentation
    /// (OpenAPI specs + event types) for applications the principal has access
    /// to. Self-contained: holding this role alone is enough to use the
    /// Developer section in the frontend. Visibility is further scoped to the
    /// principal's `iam_principal_application_access` grants at request time.
    pub fn developer() -> AuthRole {
        AuthRole::new("platform", "developer", "Developer")
            .with_description(
                "Developer portal: API documentation, accessible event types, and a self-service API credential for local testing",
            )
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::developer::APPLICATION_OPENAPI_VIEW,
                permissions::developer::API_CREDENTIAL_MANAGE,
                permissions::admin::EVENT_TYPE_READ,
                permissions::admin::PROCESS_READ,
                permissions::admin::PROCESS_CREATE,
                permissions::admin::PROCESS_UPDATE,
                permissions::admin::PROCESS_DELETE,
                permissions::admin::PROCESS_ARCHIVE,
            ])
    }

    /// Application service — auto-assigned to application service accounts
    pub fn application_service() -> AuthRole {
        let mut role = AuthRole::new(
            "platform",
            "application-service",
            "Application Service Account",
        )
        .with_description(
            "Permissions for application service accounts (scoped to own application)",
        )
        .with_source(RoleSource::Code);
        for p in permissions::application_service::ALL {
            role.permissions.insert((*p).to_string());
        }
        // Owner ruling 2 (Java ad4231be): the SDKs' stuck-message recovery
        // asks the router whether a message is in flight.
        role.permissions
            .insert(permissions::admin::ROUTER_VIEW.to_string());
        role
    }

    /// PLATFORM_FUNCTION_PUBLISHER — what a deployment pipeline's service
    /// account holds (Java PlatformRoles.java:211-217).
    pub fn function_publisher() -> AuthRole {
        AuthRole::new("platform", "function-publisher", "Function Publisher")
            .with_description("Publishes and promotes function versions")
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::function::FUNCTION_VIEW,
                permissions::function::FUNCTION_PUBLISH,
                permissions::function::FUNCTION_PROMOTE,
                permissions::function::FUNCTION_VERSION_INVOKE,
                permissions::function::FUNCTION_SECRET_MANAGE,
            ])
    }

    /// PLATFORM_FUNCTION_HOST — the one permission the function host's
    /// `/control/functions/*` calls need (Java PlatformRoles.java:219-224).
    pub fn function_host() -> AuthRole {
        AuthRole::new("platform", "function-host", "Function Host")
            .with_description("Fetches desired state and reports heartbeats for the function host")
            .with_source(RoleSource::Code)
            .with_permission(permissions::function::FUNCTION_HOST_CONTROL)
    }

    /// PLATFORM_ROUTER_OPERATOR — calls the router's own API: monitoring
    /// reads and operator actions (owner ruling 2 of 2026-09-25, Java
    /// ad4231be, appended as Java appends it).
    pub fn router_operator() -> AuthRole {
        AuthRole::new("platform", "router-operator", "Router Operator")
            .with_description(
                "Monitors and operates the message router: pools, breakers, in-flight messages, publishing",
            )
            .with_source(RoleSource::Code)
            .with_permissions([
                permissions::admin::ROUTER_VIEW,
                permissions::admin::ROUTER_OPERATE,
            ])
    }

    /// Get all built-in roles
    pub fn all() -> Vec<AuthRole> {
        vec![
            super_admin(),
            platform_admin(),
            platform_admin_readonly(),
            iam_admin(),
            iam_readonly(),
            client_admin(),
            auth_admin(),
            auth_readonly(),
            ai_agent_readonly(),
            messaging_admin(),
            viewer(),
            router(),
            portal_administrator(),
            developer(),
            application_service(),
            function_publisher(),
            function_host(),
            router_operator(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_pattern() {
        // Exact match
        assert!(matches_pattern(
            "platform:admin:client:read",
            "platform:admin:client:read"
        ));
        // Action wildcard
        assert!(matches_pattern(
            "platform:admin:client:read",
            "platform:admin:client:*"
        ));
        // Aggregate + action wildcard
        assert!(matches_pattern(
            "platform:admin:client:read",
            "platform:admin:*:*"
        ));
        // Full wildcard
        assert!(matches_pattern(
            "platform:admin:client:read",
            "platform:*:*:*"
        ));
        // Non-match
        assert!(!matches_pattern(
            "platform:admin:client:read",
            "platform:iam:client:read"
        ));
        // Wrong part count
        assert!(!matches_pattern(
            "platform:admin:client:read",
            "platform:admin:*"
        ));
        assert!(!matches_pattern("platform:admin", "platform:admin:*:*"));
    }

    #[test]
    fn permissions_outside_their_application() {
        assert!(permissions_outside_application(
            "orders",
            ["orders:order:create", "orders:order:view"]
        )
        .is_empty());
        assert_eq!(
            permissions_outside_application(
                "orders",
                [
                    "orders:order:create",
                    "platform:*:*:*",
                    "hr:x:y",
                    "platform:*:*:*"
                ]
            ),
            vec!["platform:*:*:*".to_string(), "hr:x:y".to_string()]
        );
        // A prefix is not the application.
        assert_eq!(
            permissions_outside_application("orders", ["ordersx:a:b:c"]),
            vec!["ordersx:a:b:c".to_string()]
        );
        let legacy = AuthRole {
            application_code: String::new(),
            ..AuthRole::new("hr", "clerk", "Clerk")
        };
        assert_eq!(legacy.owning_application_code(), "hr");
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
        // Bump this number whenever you add a built-in role in `roles::all()`.
        // The test is a tripwire against accidentally orphaning a new role
        // from `role_sync_service::seed_built_in_roles`'s consumption path.
        assert_eq!(all_roles.len(), 18);

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

        // Owner ruling 13: the service-account permissions.
        for role in [roles::platform_admin(), roles::iam_admin()] {
            for p in [
                permissions::admin::SERVICE_ACCOUNT_READ,
                permissions::admin::SERVICE_ACCOUNT_CREATE,
                permissions::admin::SERVICE_ACCOUNT_UPDATE,
                permissions::admin::SERVICE_ACCOUNT_DELETE,
                permissions::admin::SERVICE_ACCOUNT_MANAGE,
            ] {
                assert!(role.permissions.contains(p), "{}: {p}", role.name);
            }
        }
        for role in [roles::iam_readonly(), roles::viewer()] {
            assert!(role
                .permissions
                .contains(permissions::admin::SERVICE_ACCOUNT_READ));
            assert!(!role.has_permission(permissions::admin::SERVICE_ACCOUNT_UPDATE));
        }

        // Owner ruling 2: the router's API.
        let view = permissions::admin::ROUTER_VIEW;
        let operate = permissions::admin::ROUTER_OPERATE;
        let operator = roles::router_operator();
        assert!(operator.has_permission(view) && operator.has_permission(operate));
        for role in [roles::viewer(), roles::application_service()] {
            assert!(role.permissions.contains(view), "{}", role.name);
            assert!(!role.has_permission(operate), "{}", role.name);
        }
        assert!(roles::super_admin().has_permission(operate));
        // The router's own identity does not grant calling it.
        let router = roles::router();
        assert!(!router.has_permission(view) && !router.has_permission(operate));

        let ai_ro = roles::ai_agent_readonly();
        assert!(ai_ro.has_permission(permissions::admin::EVENT_TYPE_READ));
        assert!(!ai_ro.has_permission(permissions::admin::EVENT_TYPE_CREATE));
    }
}

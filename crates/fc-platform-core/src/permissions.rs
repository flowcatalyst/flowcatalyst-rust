//! The permission catalogue, and permission pattern matching.
//!
//! Platform permissions - 4-level format: platform:{context}:{aggregate}:{action}
//! Must match the values stored in the database by the TypeScript app.
//! Context mapping: admin = admin, messaging = messaging, iam = iam
//! Action mapping: view (not read), create, update, delete
//!
//! Every aggregate names its permissions from here; the built-in roles that
//! grant them are `role::entity::roles` (fc-platform-iam).

/// Platform Admin context — clients, applications, config
pub mod admin {
    // Client management
    pub const CLIENT_READ: &str = "platform:admin:client:view";
    pub const CLIENT_CREATE: &str = "platform:admin:client:create";
    pub const CLIENT_UPDATE: &str = "platform:admin:client:update";
    pub const CLIENT_DELETE: &str = "platform:admin:client:delete";
    pub const CLIENT_MANAGE: &str = "platform:admin:client:manage";
    pub const CLIENT_ACTIVATE: &str = "platform:admin:client:activate";
    pub const CLIENT_SUSPEND: &str = "platform:admin:client:suspend";
    pub const CLIENT_DEACTIVATE: &str = "platform:admin:client:deactivate";

    // Anchor domain management
    pub const ANCHOR_DOMAIN_READ: &str = "platform:admin:anchor-domain:view";
    pub const ANCHOR_DOMAIN_CREATE: &str = "platform:admin:anchor-domain:create";
    pub const ANCHOR_DOMAIN_UPDATE: &str = "platform:admin:anchor-domain:update";
    pub const ANCHOR_DOMAIN_DELETE: &str = "platform:admin:anchor-domain:delete";
    pub const ANCHOR_DOMAIN_MANAGE: &str = "platform:admin:anchor-domain:manage";

    // Application management
    pub const APPLICATION_READ: &str = "platform:admin:application:view";
    pub const APPLICATION_CREATE: &str = "platform:admin:application:create";
    pub const APPLICATION_UPDATE: &str = "platform:admin:application:update";
    pub const APPLICATION_DELETE: &str = "platform:admin:application:delete";
    pub const APPLICATION_MANAGE: &str = "platform:admin:application:manage";
    pub const APPLICATION_ACTIVATE: &str = "platform:admin:application:activate";
    pub const APPLICATION_DEACTIVATE: &str = "platform:admin:application:deactivate";
    pub const APPLICATION_ENABLE_CLIENT: &str = "platform:admin:application:enable-client";
    pub const APPLICATION_DISABLE_CLIENT: &str = "platform:admin:application:disable-client";

    // Event type management (messaging context in DB)
    pub const EVENT_TYPE_READ: &str = "platform:messaging:event-type:view";
    pub const EVENT_TYPE_CREATE: &str = "platform:messaging:event-type:create";
    pub const EVENT_TYPE_UPDATE: &str = "platform:messaging:event-type:update";
    pub const EVENT_TYPE_DELETE: &str = "platform:messaging:event-type:delete";
    pub const EVENT_TYPE_MANAGE: &str = "platform:messaging:event-type:manage";
    pub const EVENT_TYPE_ARCHIVE: &str = "platform:messaging:event-type:archive";
    pub const EVENT_TYPE_MANAGE_SCHEMA: &str = "platform:messaging:event-type:manage-schema";
    pub const EVENT_TYPE_SYNC: &str = "platform:messaging:event-type:sync";

    // Process documentation (messaging context in DB)
    pub const PROCESS_READ: &str = "platform:messaging:process:view";
    pub const PROCESS_CREATE: &str = "platform:messaging:process:create";
    pub const PROCESS_UPDATE: &str = "platform:messaging:process:update";
    pub const PROCESS_DELETE: &str = "platform:messaging:process:delete";
    pub const PROCESS_MANAGE: &str = "platform:messaging:process:manage";
    pub const PROCESS_ARCHIVE: &str = "platform:messaging:process:archive";
    pub const PROCESS_SYNC: &str = "platform:messaging:process:sync";

    // Dispatch pool management (messaging context in DB)
    pub const DISPATCH_POOL_READ: &str = "platform:messaging:dispatch-pool:view";
    pub const DISPATCH_POOL_CREATE: &str = "platform:messaging:dispatch-pool:create";
    pub const DISPATCH_POOL_UPDATE: &str = "platform:messaging:dispatch-pool:update";
    pub const DISPATCH_POOL_DELETE: &str = "platform:messaging:dispatch-pool:delete";
    pub const DISPATCH_POOL_MANAGE: &str = "platform:messaging:dispatch-pool:manage";
    pub const DISPATCH_POOL_SYNC: &str = "platform:messaging:dispatch-pool:sync";

    // Connection management (messaging context in DB)
    pub const CONNECTION_READ: &str = "platform:messaging:connection:view";
    pub const CONNECTION_CREATE: &str = "platform:messaging:connection:create";
    pub const CONNECTION_UPDATE: &str = "platform:messaging:connection:update";
    pub const CONNECTION_DELETE: &str = "platform:messaging:connection:delete";
    pub const CONNECTION_MANAGE: &str = "platform:messaging:connection:manage";
    pub const CONNECTION_SYNC: &str = "platform:messaging:connection:sync";

    // Subscription management (messaging context in DB)
    pub const SUBSCRIPTION_READ: &str = "platform:messaging:subscription:view";
    pub const SUBSCRIPTION_CREATE: &str = "platform:messaging:subscription:create";
    pub const SUBSCRIPTION_UPDATE: &str = "platform:messaging:subscription:update";
    pub const SUBSCRIPTION_DELETE: &str = "platform:messaging:subscription:delete";
    pub const SUBSCRIPTION_MANAGE: &str = "platform:messaging:subscription:manage";
    pub const SUBSCRIPTION_SYNC: &str = "platform:messaging:subscription:sync";

    // Event read access (messaging context in DB)
    pub const EVENT_READ: &str = "platform:messaging:event:view";
    pub const EVENT_VIEW_RAW: &str = "platform:messaging:event:view-raw";

    // Dispatch job access (messaging context in DB)
    pub const DISPATCH_JOB_READ: &str = "platform:messaging:dispatch-job:view";
    pub const DISPATCH_JOB_VIEW_RAW: &str = "platform:messaging:dispatch-job:view-raw";

    // Scheduled job management (messaging context in DB)
    pub const SCHEDULED_JOB_READ: &str = "platform:messaging:scheduled-job:view";
    pub const SCHEDULED_JOB_CREATE: &str = "platform:messaging:scheduled-job:create";
    pub const SCHEDULED_JOB_UPDATE: &str = "platform:messaging:scheduled-job:update";
    pub const SCHEDULED_JOB_DELETE: &str = "platform:messaging:scheduled-job:delete";
    pub const SCHEDULED_JOB_PAUSE: &str = "platform:messaging:scheduled-job:pause";
    pub const SCHEDULED_JOB_FIRE: &str = "platform:messaging:scheduled-job:fire";
    pub const SCHEDULED_JOB_MANAGE: &str = "platform:messaging:scheduled-job:manage";
    pub const SCHEDULED_JOB_SYNC: &str = "platform:messaging:scheduled-job:sync";
    // The router's own API (owner ruling 2 of 2026-09-25, Java
    // ad4231be): monitoring reads, and every operator action.
    pub const ROUTER_VIEW: &str = "platform:messaging:router:view";
    pub const ROUTER_OPERATE: &str = "platform:messaging:router:operate";
    pub const SCHEDULED_JOB_INSTANCE_READ: &str = "platform:messaging:scheduled-job-instance:view";

    // Identity provider management (iam context in DB)
    pub const IDENTITY_PROVIDER_READ: &str = "platform:iam:idp:view";
    pub const IDENTITY_PROVIDER_CREATE: &str = "platform:iam:idp:create";
    pub const IDENTITY_PROVIDER_UPDATE: &str = "platform:iam:idp:update";
    pub const IDENTITY_PROVIDER_DELETE: &str = "platform:iam:idp:delete";
    pub const IDENTITY_PROVIDER_MANAGE: &str = "platform:iam:idp:manage";

    // Email domain mapping management (iam context in DB)
    pub const EMAIL_DOMAIN_MAPPING_READ: &str = "platform:iam:email-domain-mapping:view";
    pub const EMAIL_DOMAIN_MAPPING_CREATE: &str = "platform:iam:email-domain-mapping:create";
    pub const EMAIL_DOMAIN_MAPPING_UPDATE: &str = "platform:iam:email-domain-mapping:update";
    pub const EMAIL_DOMAIN_MAPPING_DELETE: &str = "platform:iam:email-domain-mapping:delete";
    pub const EMAIL_DOMAIN_MAPPING_MANAGE: &str = "platform:iam:email-domain-mapping:manage";

    // Service account management (iam context in DB)
    pub const SERVICE_ACCOUNT_READ: &str = "platform:iam:service-account:view";
    pub const SERVICE_ACCOUNT_CREATE: &str = "platform:iam:service-account:create";
    pub const SERVICE_ACCOUNT_UPDATE: &str = "platform:iam:service-account:update";
    pub const SERVICE_ACCOUNT_DELETE: &str = "platform:iam:service-account:delete";
    pub const SERVICE_ACCOUNT_MANAGE: &str = "platform:iam:service-account:manage";

    // CORS origin management
    pub const CORS_ORIGIN_READ: &str = "platform:admin:cors-origin:view";
    pub const CORS_ORIGIN_CREATE: &str = "platform:admin:cors-origin:create";
    pub const CORS_ORIGIN_DELETE: &str = "platform:admin:cors-origin:delete";
    pub const CORS_ORIGIN_MANAGE: &str = "platform:admin:cors-origin:manage";

    // Login attempt & audit
    pub const LOGIN_ATTEMPT_READ: &str = "platform:admin:login-attempt:view";
    pub const AUDIT_LOG_READ: &str = "platform:admin:audit-log:view";
    pub const AUDIT_LOG_EXPORT: &str = "platform:admin:audit-log:export";

    // Platform documentation (embedded docs served at /api/docs)
    pub const DOCS_READ: &str = "platform:admin:docs:view";

    // Config management
    pub const CONFIG_READ: &str = "platform:admin:config:view";
    /// Platform-config writes (owner decision #44, 2026-09-27: Java's
    /// V17 name). Go's roles held `…:config:update` instead; a stored
    /// role that still does is read as this ([`CONFIG_UPDATE_GO`]).
    pub const CONFIG_MANAGE: &str = "platform:admin:config:manage";
    /// Go's name for [`CONFIG_MANAGE`]. Never granted by a built-in role;
    /// still honoured on a custom role stored while Go ran, so the
    /// cutover needs no data rewrite and a rollback to Go keeps working.
    pub const CONFIG_UPDATE_GO: &str = "platform:admin:config:update";

    // Batch operations
    pub const BATCH_EVENTS_WRITE: &str = "platform:messaging:batch:events-write";
    pub const BATCH_DISPATCH_JOBS_WRITE: &str = "platform:messaging:batch:dispatch-jobs-write";
    pub const BATCH_AUDIT_LOGS_WRITE: &str = "platform:admin:batch:audit-logs-write";

    /// All admin permissions
    pub const ALL: &[&str] = &[
        CLIENT_READ,
        CLIENT_CREATE,
        CLIENT_UPDATE,
        CLIENT_DELETE,
        CLIENT_MANAGE,
        CLIENT_ACTIVATE,
        CLIENT_SUSPEND,
        CLIENT_DEACTIVATE,
        ANCHOR_DOMAIN_READ,
        ANCHOR_DOMAIN_CREATE,
        ANCHOR_DOMAIN_UPDATE,
        ANCHOR_DOMAIN_DELETE,
        ANCHOR_DOMAIN_MANAGE,
        APPLICATION_READ,
        APPLICATION_CREATE,
        APPLICATION_UPDATE,
        APPLICATION_DELETE,
        APPLICATION_MANAGE,
        APPLICATION_ACTIVATE,
        APPLICATION_DEACTIVATE,
        APPLICATION_ENABLE_CLIENT,
        APPLICATION_DISABLE_CLIENT,
        EVENT_TYPE_READ,
        EVENT_TYPE_CREATE,
        EVENT_TYPE_UPDATE,
        EVENT_TYPE_DELETE,
        EVENT_TYPE_MANAGE,
        EVENT_TYPE_ARCHIVE,
        EVENT_TYPE_MANAGE_SCHEMA,
        EVENT_TYPE_SYNC,
        PROCESS_READ,
        PROCESS_CREATE,
        PROCESS_UPDATE,
        PROCESS_DELETE,
        PROCESS_MANAGE,
        PROCESS_ARCHIVE,
        PROCESS_SYNC,
        DISPATCH_POOL_READ,
        DISPATCH_POOL_CREATE,
        DISPATCH_POOL_UPDATE,
        DISPATCH_POOL_DELETE,
        DISPATCH_POOL_MANAGE,
        DISPATCH_POOL_SYNC,
        CONNECTION_READ,
        CONNECTION_CREATE,
        CONNECTION_UPDATE,
        CONNECTION_DELETE,
        CONNECTION_MANAGE,
        CONNECTION_SYNC,
        SUBSCRIPTION_READ,
        SUBSCRIPTION_CREATE,
        SUBSCRIPTION_UPDATE,
        SUBSCRIPTION_DELETE,
        SUBSCRIPTION_MANAGE,
        SUBSCRIPTION_SYNC,
        EVENT_READ,
        EVENT_VIEW_RAW,
        DISPATCH_JOB_READ,
        DISPATCH_JOB_VIEW_RAW,
        SCHEDULED_JOB_READ,
        SCHEDULED_JOB_CREATE,
        SCHEDULED_JOB_UPDATE,
        SCHEDULED_JOB_DELETE,
        SCHEDULED_JOB_PAUSE,
        SCHEDULED_JOB_FIRE,
        SCHEDULED_JOB_MANAGE,
        SCHEDULED_JOB_SYNC,
        SCHEDULED_JOB_INSTANCE_READ,
        IDENTITY_PROVIDER_READ,
        IDENTITY_PROVIDER_CREATE,
        IDENTITY_PROVIDER_UPDATE,
        IDENTITY_PROVIDER_DELETE,
        IDENTITY_PROVIDER_MANAGE,
        EMAIL_DOMAIN_MAPPING_READ,
        EMAIL_DOMAIN_MAPPING_CREATE,
        EMAIL_DOMAIN_MAPPING_UPDATE,
        EMAIL_DOMAIN_MAPPING_DELETE,
        EMAIL_DOMAIN_MAPPING_MANAGE,
        SERVICE_ACCOUNT_READ,
        SERVICE_ACCOUNT_CREATE,
        SERVICE_ACCOUNT_UPDATE,
        SERVICE_ACCOUNT_DELETE,
        SERVICE_ACCOUNT_MANAGE,
        CORS_ORIGIN_READ,
        CORS_ORIGIN_CREATE,
        CORS_ORIGIN_DELETE,
        CORS_ORIGIN_MANAGE,
        LOGIN_ATTEMPT_READ,
        AUDIT_LOG_READ,
        AUDIT_LOG_EXPORT,
        DOCS_READ,
        CONFIG_READ,
        CONFIG_MANAGE,
        BATCH_EVENTS_WRITE,
        BATCH_DISPATCH_JOBS_WRITE,
        BATCH_AUDIT_LOGS_WRITE,
        ROUTER_VIEW,
        ROUTER_OPERATE,
    ];
}

/// IAM context — users, roles, access control
pub mod iam {
    // User management
    pub const USER_READ: &str = "platform:iam:user:view";
    pub const USER_CREATE: &str = "platform:iam:user:create";
    pub const USER_UPDATE: &str = "platform:iam:user:update";
    pub const USER_DELETE: &str = "platform:iam:user:delete";
    pub const USER_MANAGE: &str = "platform:iam:user:manage";
    pub const USER_ACTIVATE: &str = "platform:iam:user:activate";
    pub const USER_DEACTIVATE: &str = "platform:iam:user:deactivate";
    pub const USER_ASSIGN_ROLES: &str = "platform:iam:user:assign-roles";

    // Role management
    pub const ROLE_READ: &str = "platform:iam:role:view";
    pub const ROLE_CREATE: &str = "platform:iam:role:create";
    pub const ROLE_UPDATE: &str = "platform:iam:role:update";
    pub const ROLE_DELETE: &str = "platform:iam:role:delete";
    pub const ROLE_MANAGE: &str = "platform:iam:role:manage";

    // Client access grants
    pub const CLIENT_ACCESS_GRANT: &str = "platform:iam:client-access:grant";
    pub const CLIENT_ACCESS_REVOKE: &str = "platform:iam:client-access:revoke";
    pub const CLIENT_ACCESS_READ: &str = "platform:iam:client-access:view";

    // Permission read
    pub const PERMISSION_READ: &str = "platform:iam:permission:view";

    // Portal identity plane (Go seed/permissions.go:195-196)
    pub const PORTAL_USER_READ: &str = "platform:iam:portal-user:view";
    pub const PORTAL_USER_MANAGE: &str = "platform:iam:portal-user:manage";

    // Auth config
    pub const AUTH_CONFIG_READ: &str = "platform:iam:auth-config:view";
    pub const AUTH_CONFIG_CREATE: &str = "platform:iam:auth-config:create";
    pub const AUTH_CONFIG_UPDATE: &str = "platform:iam:auth-config:update";
    pub const AUTH_CONFIG_DELETE: &str = "platform:iam:auth-config:delete";
    pub const AUTH_CONFIG_MANAGE: &str = "platform:iam:auth-config:manage";

    /// All IAM permissions
    pub const ALL: &[&str] = &[
        USER_READ,
        USER_CREATE,
        USER_UPDATE,
        USER_DELETE,
        USER_MANAGE,
        USER_ACTIVATE,
        USER_DEACTIVATE,
        USER_ASSIGN_ROLES,
        ROLE_READ,
        ROLE_CREATE,
        ROLE_UPDATE,
        ROLE_DELETE,
        ROLE_MANAGE,
        CLIENT_ACCESS_GRANT,
        CLIENT_ACCESS_REVOKE,
        CLIENT_ACCESS_READ,
        PERMISSION_READ,
        PORTAL_USER_READ,
        PORTAL_USER_MANAGE,
        AUTH_CONFIG_READ,
        AUTH_CONFIG_CREATE,
        AUTH_CONFIG_UPDATE,
        AUTH_CONFIG_DELETE,
        AUTH_CONFIG_MANAGE,
    ];
}

/// Auth context — OAuth clients, client auth configs
pub mod auth {
    // Client auth config
    pub const CLIENT_AUTH_CONFIG_READ: &str = "platform:auth:client-auth-config:view";
    pub const CLIENT_AUTH_CONFIG_CREATE: &str = "platform:auth:client-auth-config:create";
    pub const CLIENT_AUTH_CONFIG_UPDATE: &str = "platform:auth:client-auth-config:update";
    pub const CLIENT_AUTH_CONFIG_DELETE: &str = "platform:auth:client-auth-config:delete";
    pub const CLIENT_AUTH_CONFIG_MANAGE: &str = "platform:auth:client-auth-config:manage";

    // OAuth client management
    pub const OAUTH_CLIENT_READ: &str = "platform:auth:oauth-client:view";
    pub const OAUTH_CLIENT_CREATE: &str = "platform:auth:oauth-client:create";
    pub const OAUTH_CLIENT_UPDATE: &str = "platform:auth:oauth-client:update";
    pub const OAUTH_CLIENT_DELETE: &str = "platform:auth:oauth-client:delete";
    pub const OAUTH_CLIENT_MANAGE: &str = "platform:auth:oauth-client:manage";
    pub const OAUTH_CLIENT_REGENERATE_SECRET: &str = "platform:auth:oauth-client:regenerate-secret";

    /// All auth permissions
    pub const ALL: &[&str] = &[
        CLIENT_AUTH_CONFIG_READ,
        CLIENT_AUTH_CONFIG_CREATE,
        CLIENT_AUTH_CONFIG_UPDATE,
        CLIENT_AUTH_CONFIG_DELETE,
        CLIENT_AUTH_CONFIG_MANAGE,
        OAUTH_CLIENT_READ,
        OAUTH_CLIENT_CREATE,
        OAUTH_CLIENT_UPDATE,
        OAUTH_CLIENT_DELETE,
        OAUTH_CLIENT_MANAGE,
        OAUTH_CLIENT_REGENERATE_SECRET,
    ];
}

/// Application Service permissions (scoped to own application via SDK)
pub mod application_service {
    pub const EVENT_CREATE: &str = "platform:application-service:event:create";

    pub const EVENT_TYPE_READ: &str = "platform:application-service:event-type:view";
    pub const EVENT_TYPE_CREATE: &str = "platform:application-service:event-type:create";
    pub const EVENT_TYPE_UPDATE: &str = "platform:application-service:event-type:update";
    pub const EVENT_TYPE_DELETE: &str = "platform:application-service:event-type:delete";

    pub const SUBSCRIPTION_READ: &str = "platform:application-service:subscription:view";
    pub const SUBSCRIPTION_CREATE: &str = "platform:application-service:subscription:create";
    pub const SUBSCRIPTION_UPDATE: &str = "platform:application-service:subscription:update";
    pub const SUBSCRIPTION_DELETE: &str = "platform:application-service:subscription:delete";

    pub const CONNECTION_READ: &str = "platform:application-service:connection:view";
    pub const CONNECTION_CREATE: &str = "platform:application-service:connection:create";
    pub const CONNECTION_UPDATE: &str = "platform:application-service:connection:update";
    pub const CONNECTION_DELETE: &str = "platform:application-service:connection:delete";

    pub const ROLE_READ: &str = "platform:application-service:role:view";
    pub const ROLE_CREATE: &str = "platform:application-service:role:create";
    pub const ROLE_UPDATE: &str = "platform:application-service:role:update";
    pub const ROLE_DELETE: &str = "platform:application-service:role:delete";

    pub const PERMISSION_READ: &str = "platform:application-service:permission:view";
    pub const PERMISSION_SYNC: &str = "platform:application-service:permission:sync";

    // Scheduled job: SDK callback path (log/complete an instance the platform fired)
    pub const SCHEDULED_JOB_INSTANCE_WRITE: &str =
        "platform:application-service:scheduled-job-instance:write";
    // Scheduled job: SDK sync of definitions
    pub const SCHEDULED_JOB_SYNC: &str = "platform:application-service:scheduled-job:sync";

    // Application documentation pages: SDK sync
    pub const DOCS_SYNC: &str = "platform:application-service:docs:sync";

    // Process documentation: SDK sync of process definitions
    pub const PROCESS_READ: &str = "platform:application-service:process:view";
    pub const PROCESS_SYNC: &str = "platform:application-service:process:sync";

    // Read its own application (and that application's client configs and
    // roles). Confined to the applications the service account is bound to
    // by `checks::can_read_application`; not a grant to read every application.
    pub const APPLICATION_READ: &str = "platform:application-service:application:view";

    // Publish its own application's OpenAPI document (the SDK definitions
    // sync). The handler confines it to the applications it is bound to.
    pub const APPLICATION_OPENAPI_SYNC: &str =
        "platform:application-service:application-openapi:sync";

    /// All application service permissions, in Go's order
    /// (seed/permissions.go:200-225).
    pub const ALL: &[&str] = &[
        EVENT_CREATE,
        EVENT_TYPE_READ,
        EVENT_TYPE_CREATE,
        EVENT_TYPE_UPDATE,
        EVENT_TYPE_DELETE,
        SUBSCRIPTION_READ,
        SUBSCRIPTION_CREATE,
        SUBSCRIPTION_UPDATE,
        SUBSCRIPTION_DELETE,
        CONNECTION_READ,
        CONNECTION_CREATE,
        CONNECTION_UPDATE,
        CONNECTION_DELETE,
        ROLE_READ,
        ROLE_CREATE,
        ROLE_UPDATE,
        ROLE_DELETE,
        PERMISSION_READ,
        PERMISSION_SYNC,
        SCHEDULED_JOB_INSTANCE_WRITE,
        DOCS_SYNC,
        SCHEDULED_JOB_SYNC,
        PROCESS_READ,
        PROCESS_SYNC,
        APPLICATION_READ,
        APPLICATION_OPENAPI_SYNC,
    ];
}

/// Developer portal — applications' OpenAPI specs + event-type discovery
pub mod developer {
    pub const APPLICATION_OPENAPI_VIEW: &str = "platform:developer:application-openapi:view";
    pub const APPLICATION_OPENAPI_SYNC: &str = "platform:developer:application-openapi:sync";
    pub const APPLICATION_OPENAPI_MANAGE: &str = "platform:developer:application-openapi:manage";
    /// Self-service developer client_credentials: create, rotate and
    /// revoke your own credential (Go seed/permissions.go:192).
    pub const API_CREDENTIAL_MANAGE: &str = "platform:developer:api-credential:manage";
}

/// The function registry (Java `shared/auth/Permission.java:271-293`),
/// declared in Java's order.
pub mod function {
    pub const FUNCTION_VIEW: &str = "platform:function:function:view";
    pub const FUNCTION_MANAGE: &str = "platform:function:function:manage";
    pub const FUNCTION_PUBLISH: &str = "platform:function:version:publish";
    pub const FUNCTION_PROMOTE: &str = "platform:function:alias:promote";
    pub const FUNCTION_POLICY_MANAGE: &str = "platform:function:policy:manage";
    /// What the function host's `/control/functions/*` calls need, and
    /// nothing else.
    pub const FUNCTION_HOST_CONTROL: &str = "platform:function:host:control";
    /// Smoke-testing a versioned call.
    pub const FUNCTION_VERSION_INVOKE: &str = "platform:function:version:invoke";
    /// Platform-side config and secrets.
    pub const FUNCTION_SECRET_MANAGE: &str = "platform:function:secret:manage";
    /// Claiming and releasing public hostnames.
    pub const FUNCTION_DOMAIN_MANAGE: &str = "platform:function:domain:manage";

    /// All function permissions
    pub const ALL: &[&str] = &[
        FUNCTION_VIEW,
        FUNCTION_MANAGE,
        FUNCTION_PUBLISH,
        FUNCTION_PROMOTE,
        FUNCTION_POLICY_MANAGE,
        FUNCTION_HOST_CONTROL,
        FUNCTION_VERSION_INVOKE,
        FUNCTION_SECRET_MANAGE,
        FUNCTION_DOMAIN_MANAGE,
    ];
}

/// Superuser permission (grants all platform access)
pub const ADMIN_ALL: &str = "platform:*:*:*";

// =========================================================================
// Backward-compatibility aliases (old `messaging::` and `iam::VIEW` names)
// These allow existing code to compile while we migrate references.
// =========================================================================
pub mod messaging {
    pub use super::admin::DISPATCH_JOB_READ as DISPATCH_JOB_VIEW;
    pub use super::admin::DISPATCH_JOB_VIEW_RAW;
    pub use super::admin::DISPATCH_POOL_CREATE;
    pub use super::admin::DISPATCH_POOL_DELETE;
    pub use super::admin::DISPATCH_POOL_READ as DISPATCH_POOL_VIEW;
    pub use super::admin::DISPATCH_POOL_UPDATE;
    pub use super::admin::EVENT_READ as EVENT_VIEW;
    pub use super::admin::EVENT_TYPE_CREATE;
    pub use super::admin::EVENT_TYPE_DELETE;
    pub use super::admin::EVENT_TYPE_READ as EVENT_TYPE_VIEW;
    pub use super::admin::EVENT_TYPE_UPDATE;
    pub use super::admin::EVENT_VIEW_RAW;
    pub use super::admin::SUBSCRIPTION_CREATE;
    pub use super::admin::SUBSCRIPTION_DELETE;
    pub use super::admin::SUBSCRIPTION_READ as SUBSCRIPTION_VIEW;
    pub use super::admin::SUBSCRIPTION_UPDATE;
    // These don't have direct equivalents in admin — provide stubs
    pub const EVENT_CREATE: &str = "platform:messaging:event:create";
    pub const DISPATCH_JOB_CREATE: &str = "platform:messaging:dispatch-job:create";
    pub const DISPATCH_JOB_RETRY: &str = "platform:messaging:dispatch-job:retry";
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

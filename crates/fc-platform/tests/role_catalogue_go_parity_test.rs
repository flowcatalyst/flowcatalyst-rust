//! Pins Rust's built-in role catalogue (`role::entity::roles::all`) to Go's,
//! the production reference (owner decision #12, 2026-09-25).
//!
//! `GO_ROLES` is transcribed from flowcatalyst-go
//! `internal/platform/seed/roles.go` (`PlatformRoles`), with each permission
//! constant resolved through `internal/platform/seed/permissions.go`, at
//! flowcatalyst-go HEAD 73a6918. Every Go role must exist in Rust with the
//! same display name, description and exactly the same permission set.
//!
//! Rust additions are listed in `RUST_ADDITIONS` and nothing else may
//! differ: the function-runner roles and the function grants on
//! `platform:messaging-admin`, which Java added for functions (Go has no
//! function runner), and owner ruling 13's service-account grants.

use std::collections::{BTreeMap, BTreeSet};

use fc_platform::role::entity::roles;

const GO_ROLES: &[(&str, &str, &str, &[&str])] = &[
    (
        "super-admin",
        "Platform Super Admin",
        "Full access to all platform operations",
        &[
            "platform:*:*:*",
        ],
    ),
    (
        "admin",
        "Platform Admin",
        "Manages clients, applications, and platform configuration",
        &[
            "platform:admin:client:view",
            "platform:admin:client:create",
            "platform:admin:client:update",
            "platform:admin:client:activate",
            "platform:admin:client:suspend",
            "platform:admin:client:deactivate",
            "platform:admin:anchor-domain:view",
            "platform:admin:anchor-domain:create",
            "platform:admin:anchor-domain:update",
            "platform:admin:anchor-domain:delete",
            "platform:admin:application:view",
            "platform:admin:application:create",
            "platform:admin:application:update",
            "platform:admin:application:delete",
            "platform:admin:application:enable-client",
            "platform:admin:application:disable-client",
            "platform:admin:audit-log:view",
            "platform:admin:audit-log:export",
            "platform:admin:login-attempt:view",
            "platform:admin:docs:view",
            "platform:developer:application-openapi:manage",
            "platform:admin:config:view",
            "platform:admin:config:update",
            "platform:admin:cors-origin:view",
            "platform:admin:cors-origin:create",
            "platform:admin:cors-origin:delete",
        ],
    ),
    (
        "admin-readonly",
        "Platform Admin Read-Only",
        "View-only access to clients, applications, and platform configuration",
        &[
            "platform:admin:client:view",
            "platform:admin:anchor-domain:view",
            "platform:admin:application:view",
            "platform:admin:audit-log:view",
            "platform:admin:login-attempt:view",
            "platform:admin:docs:view",
            "platform:developer:application-openapi:view",
            "platform:admin:config:view",
            "platform:admin:cors-origin:view",
        ],
    ),
    (
        "iam-admin",
        "Platform IAM Admin",
        "Manages users, roles, and access control",
        &[
            "platform:iam:user:view",
            "platform:iam:user:create",
            "platform:iam:user:update",
            "platform:iam:user:delete",
            "platform:iam:user:activate",
            "platform:iam:user:deactivate",
            "platform:iam:user:assign-roles",
            "platform:iam:role:view",
            "platform:iam:role:create",
            "platform:iam:role:update",
            "platform:iam:role:delete",
            "platform:iam:client-access:grant",
            "platform:iam:client-access:revoke",
            "platform:iam:client-access:view",
            "platform:iam:idp:view",
            "platform:iam:idp:create",
            "platform:iam:idp:update",
            "platform:iam:idp:delete",
            "platform:iam:email-domain-mapping:view",
            "platform:iam:email-domain-mapping:create",
            "platform:iam:email-domain-mapping:update",
            "platform:iam:email-domain-mapping:delete",
        ],
    ),
    (
        "iam-readonly",
        "Platform IAM Read-Only",
        "View-only access to users and roles",
        &[
            "platform:iam:user:view",
            "platform:iam:role:view",
            "platform:iam:client-access:view",
            "platform:iam:idp:view",
            "platform:iam:email-domain-mapping:view",
        ],
    ),
    (
        "client-admin",
        "Client Administrator",
        "Manages users within the administrator's own client",
        &[
            "platform:iam:user:view",
            "platform:iam:user:create",
            "platform:iam:user:update",
            "platform:iam:user:delete",
            "platform:iam:user:activate",
            "platform:iam:user:deactivate",
            "platform:iam:user:assign-roles",
            "platform:iam:role:view",
        ],
    ),
    (
        "auth-admin",
        "Platform Auth Admin",
        "Manages authentication configuration",
        &[
            "platform:auth:client-auth-config:view",
            "platform:auth:client-auth-config:create",
            "platform:auth:client-auth-config:update",
            "platform:auth:client-auth-config:delete",
            "platform:auth:oauth-client:view",
            "platform:auth:oauth-client:create",
            "platform:auth:oauth-client:update",
            "platform:auth:oauth-client:delete",
            "platform:auth:oauth-client:regenerate-secret",
        ],
    ),
    (
        "auth-readonly",
        "Platform Auth Read-Only",
        "View-only access to authentication configuration",
        &[
            "platform:auth:client-auth-config:view",
            "platform:auth:oauth-client:view",
        ],
    ),
    (
        "ai-agent-readonly",
        "AI Agent Read-Only",
        "Read-only access to event types and subscriptions for AI agent integrations",
        &[
            "platform:messaging:event-type:view",
            "platform:messaging:subscription:view",
        ],
    ),
    (
        "messaging-admin",
        "Messaging Administrator",
        "Manages event types, subscriptions, dispatch jobs, and scheduled jobs",
        &[
            "platform:messaging:event-type:view",
            "platform:messaging:event-type:create",
            "platform:messaging:event-type:update",
            "platform:messaging:event-type:delete",
            "platform:messaging:event-type:archive",
            "platform:messaging:event-type:manage-schema",
            "platform:messaging:event-type:sync",
            "platform:messaging:subscription:view",
            "platform:messaging:subscription:create",
            "platform:messaging:subscription:update",
            "platform:messaging:subscription:delete",
            "platform:messaging:subscription:sync",
            "platform:messaging:dispatch-pool:view",
            "platform:messaging:dispatch-pool:create",
            "platform:messaging:dispatch-pool:update",
            "platform:messaging:dispatch-pool:delete",
            "platform:messaging:dispatch-pool:sync",
            "platform:messaging:connection:view",
            "platform:messaging:connection:create",
            "platform:messaging:connection:update",
            "platform:messaging:connection:delete",
            "platform:messaging:connection:sync",
            "platform:messaging:event:view",
            "platform:messaging:event:view-raw",
            "platform:messaging:dispatch-job:view",
            "platform:messaging:dispatch-job:view-raw",
            "platform:messaging:scheduled-job:view",
            "platform:messaging:scheduled-job:create",
            "platform:messaging:scheduled-job:update",
            "platform:messaging:scheduled-job:delete",
            "platform:messaging:scheduled-job:pause",
            "platform:messaging:scheduled-job:fire",
            "platform:messaging:scheduled-job:sync",
            "platform:messaging:scheduled-job-instance:view",
            "platform:messaging:process:view",
            "platform:messaging:process:create",
            "platform:messaging:process:update",
            "platform:messaging:process:delete",
            "platform:messaging:process:archive",
            "platform:messaging:process:sync",
        ],
    ),
    (
        "viewer",
        "Platform Viewer",
        "Read-only access across IAM, admin, and messaging",
        &[
            "platform:iam:user:view",
            "platform:iam:role:view",
            "platform:iam:client-access:view",
            "platform:admin:client:view",
            "platform:admin:application:view",
            "platform:messaging:event:view",
            "platform:messaging:event-type:view",
            "platform:messaging:subscription:view",
            "platform:messaging:dispatch-job:view",
            "platform:messaging:dispatch-pool:view",
            "platform:messaging:scheduled-job:view",
            "platform:messaging:scheduled-job-instance:view",
            "platform:messaging:process:view",
            "platform:admin:audit-log:view",
            "platform:admin:login-attempt:view",
            "platform:iam:idp:view",
            "platform:iam:email-domain-mapping:view",
            "platform:admin:config:view",
            "platform:admin:cors-origin:view",
        ],
    ),
    (
        "router",
        "Router",
        "Fetches the dispatch router configuration",
        &[
            "platform:messaging:dispatch-pool:view",
        ],
    ),
    (
        "portal-administrator",
        "Portal Administrator",
        "Manage the client's portal users: invite, suspend, and remove portal identities",
        &[
            "platform:iam:portal-user:view",
            "platform:iam:portal-user:manage",
        ],
    ),
    (
        "developer",
        "Developer",
        "Developer portal: API documentation, accessible event types, and a self-service API credential for local testing",
        &[
            "platform:developer:application-openapi:view",
            "platform:developer:api-credential:manage",
            "platform:messaging:event-type:view",
            "platform:messaging:process:view",
            "platform:messaging:process:create",
            "platform:messaging:process:update",
            "platform:messaging:process:delete",
            "platform:messaging:process:archive",
        ],
    ),
    (
        "application-service",
        "Application Service Account",
        "Permissions for application service accounts (scoped to own application)",
        &[
            "platform:application-service:event:create",
            "platform:application-service:event-type:view",
            "platform:application-service:event-type:create",
            "platform:application-service:event-type:update",
            "platform:application-service:event-type:delete",
            "platform:application-service:subscription:view",
            "platform:application-service:subscription:create",
            "platform:application-service:subscription:update",
            "platform:application-service:subscription:delete",
            "platform:application-service:connection:view",
            "platform:application-service:connection:create",
            "platform:application-service:connection:update",
            "platform:application-service:connection:delete",
            "platform:application-service:role:view",
            "platform:application-service:role:create",
            "platform:application-service:role:update",
            "platform:application-service:role:delete",
            "platform:application-service:permission:view",
            "platform:application-service:permission:sync",
            "platform:application-service:scheduled-job-instance:write",
            "platform:application-service:docs:sync",
            "platform:application-service:scheduled-job:sync",
            "platform:application-service:process:view",
            "platform:application-service:process:sync",
        ],
    ),
];

/// Rust-only additions, by role: the function runner's (Java
/// `PlatformRoles.java:149-150, 211-224`), and the service-account
/// permissions of owner ruling 13 (2026-09-25, Java 458ebf3a), which Go's
/// admins never needed because anchor scope passed its gates.
const RUST_ADDITIONS: &[(&str, &[&str])] = &[
    (
        "admin",
        &[
            "platform:iam:service-account:view",
            "platform:iam:service-account:create",
            "platform:iam:service-account:update",
            "platform:iam:service-account:delete",
            "platform:iam:service-account:manage",
        ],
    ),
    (
        "iam-admin",
        &[
            "platform:iam:service-account:view",
            "platform:iam:service-account:create",
            "platform:iam:service-account:update",
            "platform:iam:service-account:delete",
            "platform:iam:service-account:manage",
        ],
    ),
    ("iam-readonly", &["platform:iam:service-account:view"]),
    ("viewer", &["platform:iam:service-account:view"]),
    (
        "messaging-admin",
        &[
            "platform:function:function:view",
            "platform:function:function:manage",
            "platform:function:version:publish",
            "platform:function:alias:promote",
            "platform:function:policy:manage",
            "platform:function:version:invoke",
            "platform:function:secret:manage",
            "platform:function:domain:manage",
        ],
    ),
    (
        "function-publisher",
        &[
            "platform:function:function:view",
            "platform:function:version:publish",
            "platform:function:alias:promote",
            "platform:function:version:invoke",
            "platform:function:secret:manage",
        ],
    ),
    ("function-host", &["platform:function:host:control"]),
];

#[test]
fn built_in_roles_match_go() {
    let rust: BTreeMap<String, _> = roles::all()
        .into_iter()
        .map(|r| {
            assert_eq!(r.application_code, "platform", "{}", r.name);
            let short = r.name.strip_prefix("platform:").unwrap().to_string();
            (short, r)
        })
        .collect();
    let additions: BTreeMap<&str, BTreeSet<&str>> = RUST_ADDITIONS
        .iter()
        .map(|(name, perms)| (*name, perms.iter().copied().collect()))
        .collect();

    // The role names: Go's, plus the Rust-only function roles.
    let mut expected_names: BTreeSet<&str> = GO_ROLES.iter().map(|r| r.0).collect();
    expected_names.extend(["function-publisher", "function-host"]);
    let rust_names: BTreeSet<&str> = rust.keys().map(String::as_str).collect();
    assert_eq!(rust_names, expected_names, "built-in role names");

    for (name, display, description, go_perms) in GO_ROLES {
        let role = &rust[*name];
        assert_eq!(role.display_name, *display, "{name}: display name");
        assert_eq!(
            role.description.as_deref(),
            Some(*description),
            "{name}: description"
        );
        let go: BTreeSet<&str> = go_perms.iter().copied().collect();
        assert_eq!(go.len(), go_perms.len(), "{name}: Go lists a duplicate");
        let mut expected = go.clone();
        if let Some(extra) = additions.get(name) {
            assert!(extra.is_disjoint(&go), "{name}: an addition Go has");
            expected.extend(extra);
        }
        let actual: BTreeSet<&str> = role.permissions.iter().map(String::as_str).collect();
        assert_eq!(actual, expected, "{name}: permissions");
    }

    for name in ["function-publisher", "function-host"] {
        let actual: BTreeSet<&str> = rust[name].permissions.iter().map(String::as_str).collect();
        assert_eq!(actual, additions[name], "{name}: permissions");
    }
}

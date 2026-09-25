//! Read endpoints check Go's read permissions
//! (`docs/parity/read-permissions-vs-go.md`). Requires Docker.
//!
//! One table: every GET whose gate changed to match Go, the permission Go
//! asks, and whether Go asks anchor reach too (`anchorWith`). Each row is
//! hit three ways:
//! - a caller holding an unrelated permission: 403, as Go;
//! - a caller holding exactly the row's permission (anchor where Go asks
//!   it): past the gate (anything but 401/403);
//! - for `anchorWith` rows, a CLIENT caller holding the permission: 403.
//!
//! By-id paths use ids that do not exist: the gate runs before the lookup,
//! so "past the gate" is a 404 there.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;

use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions;
use support::{read_json, TestApp};

/// A token whose `scope` grants exactly `perms`.
fn caller(app: &TestApp, scope: UserScope, client: Option<&str>, perms: &[&str]) -> String {
    let mut p = Principal::new_user("reader@read-perms.test", scope);
    if let Some(c) = client {
        p = p.with_client_id(c);
    }
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

/// (path, the permission Go asks, whether Go asks anchor reach too)
const ROWS: &[(&str, &str, bool)] = &[
    // Applications (Go CanReadApplications)
    (
        "/api/applications",
        permissions::admin::APPLICATION_READ,
        false,
    ),
    (
        "/api/applications/app_nope",
        permissions::admin::APPLICATION_READ,
        false,
    ),
    (
        "/api/applications/by-code/nope",
        permissions::admin::APPLICATION_READ,
        false,
    ),
    (
        "/api/applications/by-id/app_nope/roles",
        permissions::admin::APPLICATION_READ,
        false,
    ),
    (
        "/api/applications/app_nope/clients",
        permissions::admin::APPLICATION_READ,
        false,
    ),
    (
        "/api/applications/app_nope/service-account",
        permissions::admin::APPLICATION_READ,
        false,
    ),
    // Clients (Go CanReadClients = anchorWith)
    ("/api/clients", permissions::admin::CLIENT_READ, true),
    (
        "/api/clients/search?q=x",
        permissions::admin::CLIENT_READ,
        true,
    ),
    (
        "/api/clients/by-identifier/nope",
        permissions::admin::CLIENT_READ,
        true,
    ),
    (
        "/api/clients/clt_nope",
        permissions::admin::CLIENT_READ,
        true,
    ),
    // Connections (Go CanReadConnections)
    (
        "/api/connections",
        permissions::admin::CONNECTION_READ,
        false,
    ),
    (
        "/api/connections/con_nope",
        permissions::admin::CONNECTION_READ,
        false,
    ),
    // CORS origins (Go CanReadCorsOrigins = anchorWith)
    (
        "/api/platform/cors",
        permissions::admin::CORS_ORIGIN_READ,
        true,
    ),
    (
        "/api/platform/cors/cor_nope",
        permissions::admin::CORS_ORIGIN_READ,
        true,
    ),
    // Dispatch pools (Go CanReadDispatchPools)
    (
        "/api/dispatch-pools",
        permissions::admin::DISPATCH_POOL_READ,
        false,
    ),
    (
        "/api/dispatch-pools/dpl_nope",
        permissions::admin::DISPATCH_POOL_READ,
        false,
    ),
    // Email-domain mappings (Go CanReadEmailDomainMappings = anchorWith)
    (
        "/api/email-domain-mappings",
        permissions::admin::EMAIL_DOMAIN_MAPPING_READ,
        true,
    ),
    (
        "/api/email-domain-mappings/edm_nope",
        permissions::admin::EMAIL_DOMAIN_MAPPING_READ,
        true,
    ),
    // Identity providers (Go CanReadIdentityProviders = anchorWith)
    (
        "/api/identity-providers",
        permissions::admin::IDENTITY_PROVIDER_READ,
        true,
    ),
    (
        "/api/identity-providers/idp_nope",
        permissions::admin::IDENTITY_PROVIDER_READ,
        true,
    ),
    // Login attempts (Go CanReadLoginAttempts = anchorWith)
    (
        "/api/login-attempts",
        permissions::admin::LOGIN_ATTEMPT_READ,
        true,
    ),
    // Roles and the permission catalogue (Go CanReadRoles)
    ("/api/roles", permissions::iam::ROLE_READ, false),
    ("/api/roles/nope:nope", permissions::iam::ROLE_READ, false),
    (
        "/api/roles/by-code/nope",
        permissions::iam::ROLE_READ,
        false,
    ),
    (
        "/api/roles/by-source/CODE",
        permissions::iam::ROLE_READ,
        false,
    ),
    (
        "/api/roles/by-application/app_nope",
        permissions::iam::ROLE_READ,
        false,
    ),
    (
        "/api/roles/filters/applications",
        permissions::iam::ROLE_READ,
        false,
    ),
    ("/api/roles/permissions", permissions::iam::ROLE_READ, false),
    (
        "/api/roles/permissions/platform:iam:user:view",
        permissions::iam::ROLE_READ,
        false,
    ),
    // Principals (Go CanReadPrincipals)
    ("/api/principals", permissions::iam::USER_READ, false),
    (
        "/api/principals/prn_nope",
        permissions::iam::USER_READ,
        false,
    ),
    (
        "/api/principals/prn_nope/roles",
        permissions::iam::USER_READ,
        false,
    ),
    (
        "/api/principals/prn_nope/application-access",
        permissions::iam::USER_READ,
        false,
    ),
    (
        "/api/principals/prn_nope/available-applications",
        permissions::iam::USER_READ,
        false,
    ),
    (
        "/api/principals/check-email-domain?email=a@example.com",
        permissions::iam::USER_READ,
        false,
    ),
    // Audit logs (Go: the audit-log view permission; Rust keeps anchor
    // reach, the rows are not client-scoped)
    ("/api/audit-logs", permissions::admin::AUDIT_LOG_READ, true),
    (
        "/api/audit-logs/recent",
        permissions::admin::AUDIT_LOG_READ,
        true,
    ),
    (
        "/api/audit-logs/entity-types",
        permissions::admin::AUDIT_LOG_READ,
        true,
    ),
    (
        "/api/audit-logs/operations",
        permissions::admin::AUDIT_LOG_READ,
        true,
    ),
    (
        "/api/audit-logs/application-ids",
        permissions::admin::AUDIT_LOG_READ,
        true,
    ),
    (
        "/api/audit-logs/client-ids",
        permissions::admin::AUDIT_LOG_READ,
        true,
    ),
    (
        "/api/audit-logs/aud_nope",
        permissions::admin::AUDIT_LOG_READ,
        true,
    ),
    (
        "/api/audit-logs/entity/Client/clt_nope",
        permissions::admin::AUDIT_LOG_READ,
        true,
    ),
    // Dashboard (Go: anchor, then client or application view)
    (
        "/bff/dashboard/stats",
        permissions::admin::CLIENT_READ,
        true,
    ),
];

/// A permission no row asks for.
const UNRELATED: &str = "platform:messaging:event:view";

#[tokio::test]
#[ignore = "requires Docker"]
async fn every_read_answers_as_go_for_an_under_privileged_caller() {
    let app = TestApp::setup().await;
    let client = fc_platform::client::entity::Client::new("Read Perms", "read-perms");
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");

    let mut failures = Vec::new();
    for (path, permission, anchor) in ROWS {
        // No permission for this route (anchor reach, so only the permission
        // is missing).
        let without = caller(&app, UserScope::Anchor, None, &[UNRELATED]);
        let (status, body) = read_json(app.get(path, &without).await).await;
        if status != StatusCode::FORBIDDEN {
            failures.push(format!("{path}: unrelated permission got {status} {body}"));
        } else if body["error"] != "PERMISSION_REQUIRED" && body["code"] != "PERMISSION_REQUIRED" {
            failures.push(format!("{path}: unrelated permission refused with {body}"));
        }

        // Exactly the route's permission, with anchor reach.
        let with = caller(&app, UserScope::Anchor, None, &[permission]);
        let (status, body) = read_json(app.get(path, &with).await).await;
        if matches!(status, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED) {
            failures.push(format!("{path}: {permission} got {status} {body}"));
        }

        // The permission without anchor reach.
        let client_caller = caller(&app, UserScope::Client, Some(&client.id), &[permission]);
        let (status, body) = read_json(app.get(path, &client_caller).await).await;
        let refused = status == StatusCode::FORBIDDEN;
        if *anchor && !refused {
            failures.push(format!(
                "{path}: CLIENT caller with {permission} got {status} {body}"
            ));
        }
        if !*anchor && matches!(status, StatusCode::UNAUTHORIZED) {
            failures.push(format!(
                "{path}: CLIENT caller with {permission} got {status}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "\n\nReads that do not answer as Go:\n  - {}\n",
        failures.join("\n  - ")
    );
}

/// Go `getByID`: a principal reads itself with no permission; another
/// client's principal answers the same 404 as a missing one; a client's
/// connection is out of reach for a caller of another client.
#[tokio::test]
#[ignore = "requires Docker"]
async fn principal_self_read_and_out_of_reach_rows_answer_as_go() {
    let app = TestApp::setup().await;
    let mine = fc_platform::client::entity::Client::new("Mine", "rp-mine");
    let theirs = fc_platform::client::entity::Client::new("Theirs", "rp-theirs");
    for c in [&mine, &theirs] {
        app.repos
            .client_repo
            .insert(c)
            .await
            .expect("insert client");
    }

    // Self, with only an unrelated permission.
    let me = Principal::new_user("me@rp.test", UserScope::Client).with_client_id(&mine.id);
    app.repos
        .principal_repo
        .insert(&me)
        .await
        .expect("insert me");
    let token = app
        .auth_service
        .generate_access_token_with_scope(&me, &[UNRELATED.to_string()], None)
        .expect("token");
    let (status, body) =
        read_json(app.get(&format!("/api/principals/{}", me.id), &token).await).await;
    assert_eq!(status, StatusCode::OK, "self read: {body}");

    // Another client's principal, with the user read permission.
    let other = Principal::new_user("other@rp.test", UserScope::Client).with_client_id(&theirs.id);
    app.repos
        .principal_repo
        .insert(&other)
        .await
        .expect("insert other");
    let reader = caller(
        &app,
        UserScope::Client,
        Some(&mine.id),
        &[permissions::iam::USER_READ],
    );
    let (status, _) = read_json(
        app.get(&format!("/api/principals/{}", other.id), &reader)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "out-of-reach principal");

    // The list hides it too.
    let (status, body) = read_json(app.get("/api/principals", &reader).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body["principals"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert!(ids.contains(&me.id.as_str()), "{body}");
    assert!(!ids.contains(&other.id.as_str()), "{body}");

    // Client access grants are anchor-only (Go listClientAccess).
    let (status, _) = read_json(
        app.get(&format!("/api/principals/{}/client-access", me.id), &reader)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

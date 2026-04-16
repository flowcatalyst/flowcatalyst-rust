//! Auth-denial + list-endpoint happy-path tests.
//!
//! Split into two sections:
//!   1. **Auth enforcement** — every anchor-only write rejects a
//!      client-scoped token with 403 Forbidden, and every authenticated
//!      endpoint rejects unauthenticated requests with 401.
//!   2. **List endpoints** — each main list endpoint returns 200 with
//!      a well-formed response for an authorised anchor caller. These
//!      smoke-test read-path hydration + pagination wrappers end-to-end.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::json;

use support::TestApp;

// ── 1. Auth enforcement ─────────────────────────────────────────────────────

/// A non-anchor token must be 403 on anchor-only write endpoints. Covers
/// a representative slice of the Phase 2 migrations.
#[tokio::test]
#[ignore = "requires Docker"]
async fn client_scoped_token_cannot_write_anchor_only_resources() {
    let app = TestApp::setup().await;
    let token = app.client_user_token("clt_some_client_id");

    // (method, path, body) — each expected to 403.
    let attempts: &[(&str, &str, serde_json::Value)] = &[
        ("POST", "/api/clients", json!({ "identifier": "x", "name": "X" })),
        ("POST", "/api/anchor-domains", json!({ "domain": "x.example.com" })),
        (
            "POST",
            "/api/oauth-clients",
            json!({ "clientName": "X", "clientType": "PUBLIC", "redirectUris": [] }),
        ),
        (
            "POST",
            "/api/idp-role-mappings",
            json!({ "idpType": "a", "idpRoleName": "b", "platformRoleName": "c" }),
        ),
    ];

    for (method, path, body) in attempts {
        let resp = match *method {
            "POST" => app.post(path, &token, body.clone()).await,
            "PUT" => app.put(path, &token, body.clone()).await,
            _ => unreachable!(),
        };
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{} {} should 403 for client-scoped token, got {}",
            method,
            path,
            resp.status()
        );
    }
}

/// Unauthenticated requests must be 401 on authenticated endpoints.
#[tokio::test]
#[ignore = "requires Docker"]
async fn unauthenticated_requests_rejected_with_401() {
    let app = TestApp::setup().await;

    let paths = &[
        "/api/clients",
        "/api/anchor-domains",
        "/api/oauth-clients",
        "/api/subscriptions",
        "/api/event-types",
        "/api/principals",
        "/api/roles",
    ];

    for path in paths {
        let resp = app.get_unauth(path).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "GET {} unauthenticated should 401, got {}",
            path,
            resp.status()
        );
    }
}

// ── 2. List endpoints (read-path smoke) ─────────────────────────────────────

/// Anchor-admin (scope=ANCHOR + ADMIN_ALL permission) can list each main
/// resource with 200. Scope-only endpoints (`require_anchor`) pass on scope;
/// permission-based endpoints (`can_read_*`) pass via the wildcard permission.
#[tokio::test]
#[ignore = "requires Docker"]
async fn anchor_admin_lists_empty_resources() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;

    let paths = &[
        "/api/clients",
        "/api/anchor-domains",
        "/api/oauth-clients",
        "/api/auth-configs",
        "/api/idp-role-mappings",
        "/api/subscriptions",
        "/api/event-types",
        "/api/principals",
        "/api/roles",
        "/api/applications",
        "/api/connections",
        "/api/dispatch-pools",
        "/api/service-accounts",
        "/api/identity-providers",
        "/api/email-domain-mappings",
    ];

    for path in paths {
        let resp = app.get(path, &token).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GET {} should 200 for anchor-admin, got {}",
            path,
            resp.status()
        );
    }
}

/// Frontend filter-options endpoints (BFF) return 200. These are the paths
/// the admin UI dropdowns call to populate their filter chips.
#[tokio::test]
#[ignore = "requires Docker"]
async fn filter_option_endpoints_return_ok() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;

    let paths = &[
        "/bff/filter-options/dispatch-jobs",
        "/bff/event-types/filters/aggregates",
        "/bff/event-types/filters/applications",
        "/bff/event-types/filters/subdomains",
    ];

    for path in paths {
        let resp = app.get(path, &token).await;
        assert!(
            resp.status().is_success(),
            "GET {} should succeed, got {}",
            path,
            resp.status()
        );
    }
}

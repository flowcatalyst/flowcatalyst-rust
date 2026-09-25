//! The profile-only gate (Go `ProfileOnlyWithoutRole`): a USER with no
//! platform role reaches only `/auth/*` and `GET /api/me`. Mirrors Go's
//! TestProfileOnlyWithoutRole and the parity scenario
//! `platform/profile-only.json`. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;

use fc_platform::domain::{Principal, UserScope};
use support::{read_json, TestApp};

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_roleless_user_reaches_only_its_profile() {
    let app = TestApp::setup().await;
    let client = fc_platform::client::entity::Client::new("Profile Only", "profile-only");
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");

    let roleless = Principal::new_user("roleless@profile-only.test", UserScope::Client)
        .with_client_id(&client.id);
    let roleless_anchor = Principal::new_user("anchor@profile-only.test", UserScope::Anchor);
    for p in [&roleless, &roleless_anchor] {
        app.repos.principal_repo.insert(p).await.expect("insert");
        let session = app.auth_service.generate_session_token(p).expect("session");

        for path in [
            "/bff/roles",
            "/api/clients",
            "/api/me/clients",
            "/api/principals",
        ] {
            let (status, body) = read_json(app.get_with_session(path, &session).await).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
            assert_eq!(body["error"], "NO_PLATFORM_ROLE", "{path}: {body}");
            assert_eq!(
                body["message"],
                "Your account has no platform access. Only your profile is available."
            );
        }
        for path in ["/api/me", "/auth/me"] {
            let (status, body) = read_json(app.get_with_session(path, &session).await).await;
            assert_eq!(status, StatusCode::OK, "{path}: {body}");
        }
        // A bearer of the same role-less user is gated the same way.
        let bearer = app.auth_service.generate_access_token(p).expect("token");
        let (status, body) = read_json(app.get("/bff/roles", &bearer).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"], "NO_PLATFORM_ROLE");
    }

    // An administrator is never role-less.
    let admin = app.anchor_admin_token().await;
    let (status, body) = read_json(app.get("/bff/roles", &admin).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A service account without roles is exempt: its route answers.
    let service = app.service_account_token(&client.id);
    let (status, body) = read_json(app.get("/api/clients", &service).await).await;
    assert_ne!(body["error"], "NO_PLATFORM_ROLE", "{status} {body}");

    // No credential: the route's own refusal, Go's 403 UNAUTHENTICATED
    // (shared/middleware.rs `AuthError::unauthenticated`).
    let (status, body) = read_json(app.get_unauth("/api/clients").await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "UNAUTHENTICATED", "{body}");
}

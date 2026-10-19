//! Go ledger items F29-F33 mirrored 2026-10-19 (docs/go-mirror/2026-10-19-identity-fixes-and-catch-up.md).
//! Requires a Postgres (see `support::db`).

use crate::support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fc_platform::client::entity::Client;
use support::TestApp;
use tower::ServiceExt;

async fn setup() -> TestApp {
    support::set_app_key();
    TestApp::setup().await
}

/// F29: the router-config document carries an ETag and answers 304 to a
/// matching `If-None-Match`; a changed document changes the ETag.
#[tokio::test]
#[ignore = "requires Docker"]
async fn router_config_is_conditional() {
    let app = setup().await;
    let admin = app.anchor_admin_token().await;
    let fetch = |if_none_match: Option<String>| {
        let router = app.router.clone();
        let admin = admin.clone();
        async move {
            let mut req = Request::builder()
                .uri("/api/dispatch/router-config")
                .header("authorization", format!("Bearer {admin}"));
            if let Some(v) = if_none_match {
                req = req.header("if-none-match", v);
            }
            router
                .oneshot(req.body(Body::empty()).unwrap())
                .await
                .unwrap()
        }
    };

    let first = fetch(None).await;
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first
        .headers()
        .get("etag")
        .expect("an ETag")
        .to_str()
        .unwrap()
        .to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");

    for same in [
        etag.clone(),
        format!("W/{etag}"),
        format!("\"other\", {etag}"),
        "*".to_string(),
    ] {
        let resp = fetch(Some(same.clone())).await;
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED, "{same}");
        assert_eq!(resp.headers().get("etag").unwrap().to_str().unwrap(), etag);
    }
    let resp = fetch(Some("\"stale\"".to_string())).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // A new client adds its queues: the validator no longer matches.
    app.repos
        .client_repo
        .insert(&Client::new("Fresh", "fresh"))
        .await
        .unwrap();
    let resp = fetch(Some(etag.clone())).await;
    assert_eq!(resp.status(), StatusCode::OK, "the document changed");
    assert_ne!(resp.headers().get("etag").unwrap().to_str().unwrap(), etag);
}

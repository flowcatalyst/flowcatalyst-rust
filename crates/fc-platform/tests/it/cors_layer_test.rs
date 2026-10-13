//! fc-server's CORS layer over the CORS Origins table, through the whole
//! platform router as the binary layers it (`AuthLayer`, then
//! `fc_platform::cors::cors_layer`).
//!
//! An allow-listed origin may call the API cross-origin with its own bearer
//! token and read the answer; it is never told it may send credentials, so
//! a browser neither completes a credentialed preflight nor exposes a
//! response to a request that carried the user's `fc_session` cookie. An
//! origin not on the list gets no CORS headers.
//!
//! Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::body::Body;
use axum::http::request::Builder;
use axum::http::{Method, Request, Response, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use fc_platform::cors::{cors_layer, AllowedOrigins};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{permissions, AuthRole};
use support::{read_json, TestApp};

const PARTNER: &str = "https://partner.example.com";
const EVIL: &str = "https://evil.example.com";

/// A super-admin user stored with its role: its session cookie and its
/// bearer token.
async fn admin(app: &TestApp) -> (String, String) {
    let role =
        AuthRole::new("platform", "cors-super", "Super").with_permission(permissions::ADMIN_ALL);
    app.repos.role_repo.insert(&role).await.unwrap();
    let mut user = Principal::new_user("root@cors.test", UserScope::Anchor);
    user.assign_role("platform:cors-super");
    app.repos.principal_repo.insert(&user).await.unwrap();
    (
        app.auth_service.generate_session_token(&user).unwrap(),
        app.auth_service.generate_access_token(&user).unwrap(),
    )
}

fn header<'a>(resp: &'a Response<Body>, name: &str) -> Option<&'a str> {
    resp.headers().get(name).map(|v| v.to_str().unwrap())
}

fn varies_on_origin(resp: &Response<Body>) -> bool {
    resp.headers().get_all("vary").iter().any(|v| {
        v.to_str()
            .unwrap()
            .split(',')
            .any(|h| h.trim().eq_ignore_ascii_case("origin"))
    })
}

async fn send(router: &Router, req: Request<Body>) -> Response<Body> {
    router.clone().oneshot(req).await.unwrap()
}

/// A cross-site browser request from `origin`.
fn from(origin: &str, method: Method, path: &str) -> Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "platform.example.com")
        .header("origin", origin)
        .header("sec-fetch-site", "cross-site")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_allow_listed_origin_reads_with_a_bearer_but_never_with_the_cookie() {
    let app = TestApp::setup().await;
    let (session, bearer) = admin(&app).await;

    // The origin goes on the list the way an operator puts it there.
    let resp = app
        .post(
            "/api/platform/cors",
            &bearer,
            json!({ "origin": PARTNER, "description": "partner app" }),
        )
        .await;
    let (status, created) = read_json(resp).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    // As fc-server loads it.
    let origins = AllowedOrigins::new();
    assert_eq!(origins.refresh(&app.repos.cors_repo).await.unwrap(), 1);
    let router = app.router.clone().layer(cors_layer(origins.clone()));

    // The preflight for a bearer call: allowed, without credentials.
    let resp = send(
        &router,
        from(PARTNER, Method::OPTIONS, "/api/clients")
            .header("access-control-request-method", "GET")
            .header("access-control-request-headers", "authorization")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    assert_eq!(header(&resp, "access-control-allow-origin"), Some(PARTNER));
    assert_eq!(header(&resp, "access-control-allow-credentials"), None);
    assert!(varies_on_origin(&resp));

    // The bearer call itself: answered, readable by the partner page.
    let resp = send(
        &router,
        from(PARTNER, Method::GET, "/api/clients")
            .header("authorization", format!("Bearer {bearer}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header(&resp, "access-control-allow-origin"), Some(PARTNER));
    assert_eq!(header(&resp, "access-control-allow-credentials"), None);
    assert!(varies_on_origin(&resp));

    // The same read with the user's cookie (a page using `credentials:
    // "include"`): the platform answers it (a GET, not refused by the
    // cookie rule) with the user's data, so what stops the partner page
    // reading it is the missing `Allow-Credentials`.
    let resp = send(
        &router,
        from(PARTNER, Method::GET, "/api/clients")
            .header("cookie", format!("fc_session={session}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header(&resp, "access-control-allow-credentials"), None);

    // A credentialed preflight for a cookie write (JSON, DELETE): no
    // `Allow-Credentials`, so the browser never sends the write.
    let resp = send(
        &router,
        from(PARTNER, Method::OPTIONS, "/api/clients/cli_x")
            .header("access-control-request-method", "DELETE")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(header(&resp, "access-control-allow-credentials"), None);

    // An origin not on the list: no CORS headers, bearer or cookie.
    for credential in [
        ("authorization", format!("Bearer {bearer}")),
        ("cookie", format!("fc_session={session}")),
    ] {
        let resp = send(
            &router,
            from(EVIL, Method::GET, "/api/clients")
                .header(credential.0, credential.1.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(header(&resp, "access-control-allow-origin"), None);
        assert_eq!(header(&resp, "access-control-allow-credentials"), None);
        assert!(varies_on_origin(&resp));
    }

    // Removing the origin takes it off the list at the next refresh.
    let id = created["id"].as_str().unwrap();
    let resp = app
        .delete(&format!("/api/platform/cors/{id}"), &bearer)
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    assert_eq!(origins.refresh(&app.repos.cors_repo).await.unwrap(), 0);
    let resp = send(
        &router,
        from(PARTNER, Method::GET, "/api/clients")
            .header("authorization", format!("Bearer {bearer}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(header(&resp, "access-control-allow-origin"), None);
}

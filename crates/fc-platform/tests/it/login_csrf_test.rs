//! Login CSRF on the public sign-in routes (Go `CredentialCSRF`, docs F16
//! follow-up; Go internal/server/logincsrf_pg_test.go), through the whole
//! platform router.
//!
//! On every route that signs a browser in or sets its credentials, an
//! unsafe request a browser marks as from another origin is 403
//! `CROSS_ORIGIN_REQUEST`, and a body that is not JSON is 415
//! `UNSUPPORTED_MEDIA_TYPE` for every caller. A request with neither
//! `Sec-Fetch-Site` nor `Origin` passes (a non-browser caller). The route
//! table itself is pinned by `route_table_snapshot_test`
//! (`every_public_unsafe_route_is_decided`).
//!
//! Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

use fc_platform::auth::password_service::PasswordService;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::shared::session_csrf::sign_in_routes;
use support::{read_json, TestApp};

const HOST: &str = "platform.example.com";
const PASSWORD: &str = "Correct-Horse-Battery-9157";

#[derive(Clone, Copy, Default)]
struct Headers {
    host: Option<&'static str>,
    sec_fetch_site: Option<&'static str>,
    origin: Option<&'static str>,
    content_type: Option<&'static str>,
}

async fn post(app: &TestApp, path: &str, h: Headers, body: Option<&str>) -> Response<Body> {
    let mut b = Request::post(path).header("host", h.host.unwrap_or(HOST));
    if let Some(v) = h.sec_fetch_site {
        b = b.header("sec-fetch-site", v);
    }
    if let Some(v) = h.origin {
        b = b.header("origin", v);
    }
    if let Some(v) = h.content_type {
        b = b.header("content-type", v);
    }
    let body = body.map_or_else(Body::empty, |s| Body::from(s.to_string()));
    app.router
        .clone()
        .oneshot(b.body(body).unwrap())
        .await
        .unwrap()
}

fn sets_session(resp: &Response<Body>) -> bool {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|c| c.starts_with("fc_session=") && !c.starts_with("fc_session=;"))
}

async fn code(resp: Response<Body>) -> (StatusCode, Value) {
    read_json(resp).await
}

const JSON: Option<&str> = Some("application/json");

/// What a page on another origin can make a browser send, and the
/// non-JSON bodies, on every guarded route: refused before the handler.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_sign_in_routes_refuse_cross_origin_and_non_json_requests() {
    let app = TestApp::setup().await;
    let refused: [(&str, Headers, Option<&str>, StatusCode, &str); 8] = [
        (
            "cross-site form post as text/plain",
            Headers {
                sec_fetch_site: Some("cross-site"),
                origin: Some("https://attacker.test"),
                content_type: Some("text/plain"),
                ..Headers::default()
            },
            Some("{}"),
            StatusCode::FORBIDDEN,
            "CROSS_ORIGIN_REQUEST",
        ),
        (
            "same-site sibling, JSON",
            Headers {
                sec_fetch_site: Some("same-site"),
                origin: Some("https://evil.example.com"),
                content_type: JSON,
                ..Headers::default()
            },
            Some("{}"),
            StatusCode::FORBIDDEN,
            "CROSS_ORIGIN_REQUEST",
        ),
        (
            "old browser, foreign Origin",
            Headers {
                origin: Some("https://evil.example.com"),
                content_type: JSON,
                ..Headers::default()
            },
            Some("{}"),
            StatusCode::FORBIDDEN,
            "CROSS_ORIGIN_REQUEST",
        ),
        (
            "Origin null",
            Headers {
                origin: Some("null"),
                content_type: JSON,
                ..Headers::default()
            },
            Some("{}"),
            StatusCode::FORBIDDEN,
            "CROSS_ORIGIN_REQUEST",
        ),
        (
            "same-origin text/plain",
            Headers {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("text/plain;charset=UTF-8"),
                ..Headers::default()
            },
            Some("{}"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
        ),
        (
            "non-browser form body",
            Headers {
                content_type: Some("application/x-www-form-urlencoded"),
                ..Headers::default()
            },
            Some("a=b"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
        ),
        (
            "multipart",
            Headers {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("multipart/form-data; boundary=x"),
                ..Headers::default()
            },
            Some("--x--"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
        ),
        (
            "a body without Content-Type (a Blob)",
            Headers {
                sec_fetch_site: Some("same-origin"),
                ..Headers::default()
            },
            Some("{}"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
        ),
    ];
    let routes = sign_in_routes();
    assert_eq!(routes.len(), 15, "{routes:?}");
    for path in routes {
        for (name, headers, body, status, error) in refused {
            let (got, resp) = code(post(&app, path, headers, body).await).await;
            assert_eq!(got, status, "{path}, {name}: {resp}");
            assert_eq!(resp["error"], error, "{path}, {name}");
        }
        // The browser's own requests and a non-browser JSON caller reach
        // the handler (which answers on its own terms).
        let allowed: [(&str, Headers); 4] = [
            (
                "same-origin",
                Headers {
                    sec_fetch_site: Some("same-origin"),
                    origin: Some("https://platform.example.com"),
                    content_type: JSON,
                    ..Headers::default()
                },
            ),
            (
                "user-initiated",
                Headers {
                    sec_fetch_site: Some("none"),
                    content_type: JSON,
                    ..Headers::default()
                },
            ),
            (
                "old browser, Origin matches Host",
                Headers {
                    origin: Some("http://platform.example.com"),
                    content_type: JSON,
                    ..Headers::default()
                },
            ),
            (
                "no browser headers",
                Headers {
                    content_type: Some("application/json; charset=utf-8"),
                    ..Headers::default()
                },
            ),
        ];
        for (name, headers) in allowed {
            let (status, resp) = code(post(&app, path, headers, Some("{}")).await).await;
            assert_ne!(
                resp["error"], "CROSS_ORIGIN_REQUEST",
                "{path}, {name}: {status}"
            );
            assert_ne!(
                resp["error"], "UNSUPPORTED_MEDIA_TYPE",
                "{path}, {name}: {status}"
            );
        }
    }

    // Left: check-domain reads, logout issues nothing.
    for path in ["/auth/check-domain", "/auth/logout", "/auth/refresh"] {
        let (status, resp) = code(
            post(
                &app,
                path,
                Headers {
                    sec_fetch_site: Some("cross-site"),
                    origin: Some("https://attacker.test"),
                    content_type: Some("text/plain"),
                    ..Headers::default()
                },
                Some(r#"{"email":"x@example.com"}"#),
            )
            .await,
        )
        .await;
        assert_ne!(resp["error"], "CROSS_ORIGIN_REQUEST", "{path}: {status}");
        assert_ne!(resp["error"], "UNSUPPORTED_MEDIA_TYPE", "{path}: {status}");
    }
}

/// The attack itself: a cross-site `<form enctype="text/plain">` posting
/// the attacker's credentials signed the victim's browser in. Now no
/// cookie is set unless the request is the SPA's (same-origin JSON) or a
/// non-browser JSON caller's.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_cross_site_form_cannot_sign_a_browser_in() {
    let app = TestApp::setup_with_issuer("https://platform.example.com").await;
    let mut user = Principal::new_user("victim-of-login-csrf@example.com", UserScope::Anchor);
    if let Some(identity) = user.user_identity.as_mut() {
        identity.password_hash = Some(PasswordService::default().hash_password(PASSWORD).unwrap());
    }
    app.repos.principal_repo.insert(&user).await.unwrap();
    let credentials =
        format!(r#"{{"email":"victim-of-login-csrf@example.com","password":"{PASSWORD}"}}"#);

    let attempts: [(&str, Headers, StatusCode); 3] = [
        (
            "cross-site text/plain form",
            Headers {
                sec_fetch_site: Some("cross-site"),
                origin: Some("https://attacker.test"),
                content_type: Some("text/plain"),
                ..Headers::default()
            },
            StatusCode::FORBIDDEN,
        ),
        (
            "same-site sibling",
            Headers {
                sec_fetch_site: Some("same-site"),
                origin: Some("https://evil.example.com"),
                content_type: JSON,
                ..Headers::default()
            },
            StatusCode::FORBIDDEN,
        ),
        (
            "same-origin text/plain",
            Headers {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("text/plain"),
                ..Headers::default()
            },
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ];
    for (name, headers, status) in attempts {
        let resp = post(&app, "/auth/login", headers, Some(&credentials)).await;
        assert_eq!(resp.status(), status, "{name}");
        assert!(!sets_session(&resp), "{name}: a session cookie was set");
    }

    let signed_in: [(&str, Headers); 4] = [
        (
            "the SPA",
            Headers {
                sec_fetch_site: Some("same-origin"),
                origin: Some("https://platform.example.com"),
                content_type: JSON,
                ..Headers::default()
            },
        ),
        (
            "curl",
            Headers {
                content_type: JSON,
                ..Headers::default()
            },
        ),
        (
            "an old browser on this host",
            Headers {
                origin: Some("https://platform.example.com"),
                content_type: JSON,
                ..Headers::default()
            },
        ),
        (
            "behind a proxy that rewrites Host",
            Headers {
                host: Some("internal:8080"),
                sec_fetch_site: Some("same-site"),
                origin: Some("https://platform.example.com"),
                content_type: JSON,
            },
        ),
    ];
    for (name, headers) in signed_in {
        let resp = post(&app, "/auth/login", headers, Some(&credentials)).await;
        assert_eq!(resp.status(), StatusCode::OK, "{name}");
        assert!(sets_session(&resp), "{name}: no session cookie");
    }
}

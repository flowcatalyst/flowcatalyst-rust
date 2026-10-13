//! The session-cookie request guard (Go `SessionCSRF`, docs F16; Go
//! internal/server/crossorigin_pg_test.go): through the whole platform
//! router, with the requests a browser sends.
//!
//! - an unsafe request the `fc_session` cookie authenticates must come from
//!   this origin (`Sec-Fetch-Site` same-origin/none, or else an `Origin`
//!   naming this host or the public base URL); with neither header it is
//!   refused: 403 `CROSS_ORIGIN_REQUEST`;
//! - its body, when it has one, must be JSON: 415 `UNSUPPORTED_MEDIA_TYPE`;
//! - a bearer request is subject to neither rule, and neither are the
//!   routes Go serves outside its authenticated group (OAuth, sign-in).
//!
//! Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{permissions, AuthRole};
use support::{read_json, TestApp};

const HOST: &str = "platform.example.com";
const EVIL: &str = "https://evil.example.com";

/// A request as a browser (or an SDK) sends it.
#[derive(Clone, Default)]
struct Req {
    method: Option<Method>,
    path: String,
    host: Option<&'static str>,
    session: Option<String>,
    bearer: Option<String>,
    authorization: Option<String>,
    sec_fetch_site: Option<&'static str>,
    origin: Option<&'static str>,
    content_type: Option<&'static str>,
    body: Option<String>,
}

async fn send(app: &TestApp, r: Req) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(r.method.unwrap_or(Method::POST))
        .uri(&r.path)
        .header("host", r.host.unwrap_or(HOST));
    if let Some(s) = &r.session {
        b = b.header("cookie", format!("theme=dark; fc_session={s}"));
    }
    if let Some(t) = &r.bearer {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    if let Some(a) = &r.authorization {
        b = b.header("authorization", a.as_str());
    }
    if let Some(v) = r.sec_fetch_site {
        b = b.header("sec-fetch-site", v);
    }
    if let Some(v) = r.origin {
        b = b.header("origin", v);
    }
    if let Some(v) = r.content_type {
        b = b.header("content-type", v);
    }
    let body = r.body.map_or_else(Body::empty, Body::from);
    let resp = app
        .router
        .clone()
        .oneshot(b.body(body).unwrap())
        .await
        .unwrap();
    read_json(resp).await
}

/// A super-admin user stored with its role: its session cookie and its
/// bearer token.
async fn admin(app: &TestApp) -> (String, String) {
    if app
        .repos
        .role_repo
        .find_by_name("platform:xo-super")
        .await
        .unwrap()
        .is_none()
    {
        let role =
            AuthRole::new("platform", "xo-super", "Super").with_permission(permissions::ADMIN_ALL);
        app.repos.role_repo.insert(&role).await.unwrap();
    }
    let mut user = Principal::new_user("root@xo.test", UserScope::Anchor);
    user.assign_role("platform:xo-super");
    app.repos.principal_repo.insert(&user).await.unwrap();
    (
        app.auth_service.generate_session_token(&user).unwrap(),
        app.auth_service.generate_access_token(&user).unwrap(),
    )
}

async fn suspended_client(app: &TestApp, identifier: &str) -> String {
    let mut client = Client::new(identifier, identifier);
    client.suspend("test");
    app.repos.client_repo.insert(&client).await.unwrap();
    client.id.to_string()
}

async fn client_status(app: &TestApp, id: &str) -> String {
    let (status,): (String,) = sqlx::query_as("SELECT status FROM tnt_clients WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    status
}

async fn count(app: &TestApp, sql: &str, bind: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql)
        .bind(bind)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    n
}

async fn clients_named(app: &TestApp, identifier: &str) -> i64 {
    count(
        app,
        "SELECT COUNT(*) FROM tnt_clients WHERE identifier = $1",
        identifier,
    )
    .await
}

async fn domains_named(app: &TestApp, hostname: &str) -> i64 {
    count(
        app,
        "SELECT COUNT(*) FROM fnr_domains WHERE hostname = $1",
        hostname,
    )
    .await
}

fn client_body(identifier: &str) -> Option<String> {
    Some(format!(
        r#"{{"name":"{identifier}","identifier":"{identifier}"}}"#
    ))
}

fn domain_body(hostname: &str) -> Option<String> {
    Some(format!(r#"{{"hostname":"{hostname}"}}"#))
}

/// What a page on another origin can make a browser send with the cookie:
/// SameSite=Lax keeps it off cross-SITE POSTs, but a sibling subdomain is
/// same-site, so the cookie rides along. Refused on every kind of route —
/// typed JSON bodies, bodies read as bytes, body-less actions, DELETE — and
/// nothing is written.
#[tokio::test]
#[ignore = "requires Docker"]
async fn cross_origin_cookie_writes_are_refused() {
    let app = TestApp::setup().await;
    let (session, _) = admin(&app).await;
    let suspended = suspended_client(&app, "xo-suspended").await;
    let deletable = suspended_client(&app, "xo-deletable").await;

    // The provenance variants a browser (or a non-browser) can produce.
    let variants: [(&str, Option<&'static str>, Option<&'static str>); 5] = [
        ("same-site sibling", Some("same-site"), Some(EVIL)),
        (
            "cross-site",
            Some("cross-site"),
            Some("https://attacker.test"),
        ),
        ("old browser, foreign Origin", None, Some(EVIL)),
        ("neither header", None, None),
        ("Origin null", None, Some("null")),
    ];
    for (variant, sec_fetch_site, origin) in variants {
        let base = Req {
            session: Some(session.clone()),
            sec_fetch_site,
            origin,
            ..Req::default()
        };
        let cases = [
            (
                "body-less activate",
                Req {
                    path: format!("/api/clients/{suspended}/activate"),
                    ..base.clone()
                },
            ),
            (
                "text/plain body read as bytes",
                Req {
                    path: "/api/function-domains".into(),
                    content_type: Some("text/plain"),
                    body: domain_body("xo1.example.org"),
                    ..base.clone()
                },
            ),
            (
                "JSON body with no Content-Type, read as bytes",
                Req {
                    path: "/api/function-domains".into(),
                    body: domain_body("xo2.example.org"),
                    ..base.clone()
                },
            ),
            (
                "JSON create",
                Req {
                    path: "/api/clients".into(),
                    content_type: Some("application/json"),
                    body: client_body("xo-created"),
                    ..base.clone()
                },
            ),
            (
                "BFF text/plain",
                Req {
                    path: "/bff/roles/permissions".into(),
                    content_type: Some("text/plain"),
                    body: Some(r#"{"permission":"xo:evil:thing:view"}"#.into()),
                    ..base.clone()
                },
            ),
            (
                "DELETE",
                Req {
                    method: Some(Method::DELETE),
                    path: format!("/api/clients/{deletable}"),
                    ..base.clone()
                },
            ),
            (
                "session self-service",
                Req {
                    path: "/auth/2fa/methods/totp/begin".into(),
                    ..base.clone()
                },
            ),
        ];
        for (name, req) in cases {
            let (status, body) = send(&app, req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{variant}, {name}: {body}");
            assert_eq!(body["error"], "CROSS_ORIGIN_REQUEST", "{variant}, {name}");
            assert_eq!(
                body["message"],
                "This request was refused because it did not come from this site. Requests \
                 authenticated by the session cookie must be sent by the platform's own pages; \
                 API clients authenticate with a bearer token."
            );
        }
    }
    assert_eq!(client_status(&app, &suspended).await, "SUSPENDED");
    assert_eq!(client_status(&app, &deletable).await, "SUSPENDED");
    assert_eq!(domains_named(&app, "xo1.example.org").await, 0);
    assert_eq!(domains_named(&app, "xo2.example.org").await, 0);
    assert_eq!(clients_named(&app, "xo-created").await, 0);

    // A cross-origin request with a non-JSON body: provenance first (403).
    let (status, body) = send(
        &app,
        Req {
            path: "/api/clients".into(),
            session: Some(session.clone()),
            sec_fetch_site: Some("cross-site"),
            content_type: Some("text/plain"),
            body: client_body("xo-created"),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("CROSS_ORIGIN_REQUEST"))
    );

    // A role-less user is refused the same way: the guard runs before the
    // profile-only gate.
    let roleless = Principal::new_user("roleless@xo.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&roleless).await.unwrap();
    let roleless_session = app.auth_service.generate_session_token(&roleless).unwrap();
    let (status, body) = send(
        &app,
        Req {
            path: format!("/api/clients/{suspended}/activate"),
            session: Some(roleless_session),
            sec_fetch_site: Some("same-site"),
            origin: Some(EVIL),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "CROSS_ORIGIN_REQUEST");
}

/// The SPA's own requests pass: same-origin fetch metadata, a direct
/// navigation (`none`), an old browser whose `Origin` names this host, the
/// public base URL behind a proxy that rewrites `Host`, and every GET.
#[tokio::test]
#[ignore = "requires Docker"]
async fn same_origin_cookie_requests_are_allowed() {
    let app = TestApp::setup_with_issuer("https://platform.example.com/").await;
    let (session, _) = admin(&app).await;

    let cases: [(
        &str,
        Option<&'static str>,
        Option<&'static str>,
        &'static str,
    ); 5] = [
        (
            "same-origin",
            Some("same-origin"),
            Some("https://platform.example.com"),
            HOST,
        ),
        ("user-initiated", Some("none"), None, HOST),
        (
            "old browser, Origin matches Host",
            None,
            Some("http://platform.example.com"),
            HOST,
        ),
        (
            "rewritten Host, public Origin",
            None,
            Some("https://platform.example.com"),
            "internal:8080",
        ),
        (
            "rewritten Host, public Origin, same-site fetch metadata",
            Some("same-site"),
            Some("https://platform.example.com"),
            "internal:8080",
        ),
    ];
    for (i, (name, sec_fetch_site, origin, host)) in cases.into_iter().enumerate() {
        let base = Req {
            session: Some(session.clone()),
            sec_fetch_site,
            origin,
            host: Some(host),
            ..Req::default()
        };
        let client = suspended_client(&app, &format!("xo-ok-{i}")).await;
        let (status, body) = send(
            &app,
            Req {
                path: format!("/api/clients/{client}/activate"),
                ..base.clone()
            },
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");
        assert_eq!(client_status(&app, &client).await, "ACTIVE", "{name}");

        let identifier = format!("xo-new-{i}");
        let (status, body) = send(
            &app,
            Req {
                path: "/api/clients".into(),
                content_type: Some("application/json; charset=utf-8"),
                body: client_body(&identifier),
                ..base.clone()
            },
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{name}: {body}");
        assert_eq!(clients_named(&app, &identifier).await, 1);
    }

    // Behind a proxy that rewrites Host, an Origin that is neither the
    // request's host nor the public base URL is still refused.
    let (status, body) = send(
        &app,
        Req {
            path: "/api/clients".into(),
            host: Some("internal:8080"),
            session: Some(session.clone()),
            origin: Some("http://platform.example.com"),
            content_type: Some("application/json"),
            body: client_body("xo-wrong-scheme"),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // A GET is never refused, whatever its provenance.
    let (status, body) = send(
        &app,
        Req {
            method: Some(Method::GET),
            path: "/api/clients".into(),
            session: Some(session.clone()),
            sec_fetch_site: Some("cross-site"),
            origin: Some("https://attacker.test"),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// From this origin, a cookie-authenticated body must be JSON: text/plain,
/// form and multipart bodies (and a body with no Content-Type) are what a
/// page on another origin can send without a preflight.
#[tokio::test]
#[ignore = "requires Docker"]
async fn non_json_cookie_bodies_are_refused() {
    let app = TestApp::setup().await;
    let (session, _) = admin(&app).await;
    let content_types: [Option<&'static str>; 5] = [
        Some("text/plain;charset=UTF-8"),
        Some("application/x-www-form-urlencoded"),
        Some("multipart/form-data; boundary=x"),
        Some("application/wasm"),
        None,
    ];
    for content_type in content_types {
        for (path, body) in [
            ("/api/clients", client_body("xo-form")),
            ("/api/function-domains", domain_body("xo-form.example.org")),
            (
                "/bff/roles/permissions",
                Some(r#"{"permission":"xo:form:thing:view"}"#.into()),
            ),
        ] {
            let (status, resp) = send(
                &app,
                Req {
                    path: path.into(),
                    session: Some(session.clone()),
                    sec_fetch_site: Some("same-origin"),
                    content_type,
                    body,
                    ..Req::default()
                },
            )
            .await;
            assert_eq!(
                status,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "{path} {content_type:?}: {resp}"
            );
            assert_eq!(resp["error"], "UNSUPPORTED_MEDIA_TYPE");
            assert_eq!(
                resp["message"],
                "Requests authenticated by the session cookie must send a JSON body \
                 (Content-Type: application/json)."
            );
        }
    }
    assert_eq!(clients_named(&app, "xo-form").await, 0);
    assert_eq!(domains_named(&app, "xo-form.example.org").await, 0);

    // The same body as JSON, and a +json type, are accepted.
    let (status, body) = send(
        &app,
        Req {
            path: "/api/function-domains".into(),
            session: Some(session.clone()),
            sec_fetch_site: Some("same-origin"),
            content_type: Some("application/merge-patch+json"),
            body: domain_body("xo-json.example.org"),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// Bearer callers (SDKs, service accounts, the router) send no cookie and
/// no browser headers, or arbitrary ones; neither rule applies to them, even
/// with the cookie beside the bearer.
#[tokio::test]
#[ignore = "requires Docker"]
async fn bearer_requests_are_not_subject_to_the_cookie_rules() {
    let app = TestApp::setup().await;
    let (session, bearer) = admin(&app).await;

    let cases = [
        (
            "foreign Origin, cross-site",
            Req {
                sec_fetch_site: Some("cross-site"),
                origin: Some(EVIL),
                ..Req::default()
            },
        ),
        ("no browser headers", Req::default()),
        (
            "beside the cookie",
            Req {
                session: Some(session.clone()),
                sec_fetch_site: Some("same-site"),
                origin: Some(EVIL),
                ..Req::default()
            },
        ),
    ];
    for (i, (name, headers)) in cases.into_iter().enumerate() {
        let base = Req {
            bearer: Some(bearer.clone()),
            ..headers
        };
        let client = suspended_client(&app, &format!("xo-bearer-{i}")).await;
        let (status, body) = send(
            &app,
            Req {
                path: format!("/api/clients/{client}/activate"),
                ..base.clone()
            },
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");

        // A bearer's body is read as before (a byte-reading route decodes
        // text/plain), and an artifact-style non-JSON type is not refused.
        let host = format!("xo-bearer-{i}.example.org");
        let (status, body) = send(
            &app,
            Req {
                path: "/api/function-domains".into(),
                content_type: Some("text/plain"),
                body: domain_body(&host),
                ..base.clone()
            },
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{name}: {body}");
    }

    // An Authorization header of another scheme decides on its own: the
    // cookie is not consulted, so the request is unauthenticated, not
    // refused as cross-origin.
    let (status, body) = send(
        &app,
        Req {
            path: "/api/clients".into(),
            authorization: Some("Basic dXNlcjpwYXNz".into()),
            session: Some(session.clone()),
            sec_fetch_site: Some("cross-site"),
            content_type: Some("text/plain"),
            body: client_body("xo-basic"),
            ..Req::default()
        },
    )
    .await;
    assert_ne!(body["error"], "CROSS_ORIGIN_REQUEST", "{status} {body}");
    assert_ne!(body["error"], "UNSUPPORTED_MEDIA_TYPE", "{status} {body}");
    assert_eq!(clients_named(&app, "xo-basic").await, 0);
}

/// The routes Go serves outside its authenticated group authenticate their
/// own callers or sign a browser in: the cookie rule does not answer them
/// (each fails, or not, on its own terms). A stale cookie authenticates
/// nothing, so the request is refused as unauthenticated, not as
/// cross-origin.
#[tokio::test]
#[ignore = "requires Docker"]
async fn public_routes_and_stale_cookies_are_not_cookie_sessions() {
    let app = TestApp::setup().await;
    let (session, _) = admin(&app).await;
    let cross = Req {
        session: Some(session.clone()),
        sec_fetch_site: Some("cross-site"),
        origin: Some("https://attacker.test"),
        ..Req::default()
    };
    for (path, content_type, body) in [
        (
            "/oauth/token",
            Some("application/x-www-form-urlencoded"),
            Some("grant_type=client_credentials&client_id=nope&client_secret=nope".to_string()),
        ),
        (
            "/auth/login",
            Some("text/plain"),
            Some(r#"{"email":"root@xo.test","password":"nope"}"#.into()),
        ),
        ("/auth/logout", None, None),
        ("/auth/2fa/verify", Some("text/plain"), Some("{}".into())),
        (
            "/auth/password-reset/request",
            Some("text/plain"),
            Some(r#"{"email":"x@xo.test"}"#.into()),
        ),
        (
            "/api/dispatch/process",
            Some("text/plain"),
            Some("{}".into()),
        ),
    ] {
        let (status, resp) = send(
            &app,
            Req {
                path: path.into(),
                content_type,
                body,
                ..cross.clone()
            },
        )
        .await;
        assert_ne!(
            resp["error"], "CROSS_ORIGIN_REQUEST",
            "{path}: {status} {resp}"
        );
        assert_ne!(
            resp["error"], "UNSUPPORTED_MEDIA_TYPE",
            "{path}: {status} {resp}"
        );
    }

    let (status, body) = send(
        &app,
        Req {
            path: "/api/clients".into(),
            session: Some("not-a-session".into()),
            sec_fetch_site: Some("cross-site"),
            content_type: Some("text/plain"),
            body: client_body("xo-stale"),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "UNAUTHENTICATED", "{body}");
}

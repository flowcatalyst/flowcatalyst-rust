//! The router API's platform-bearer guard (owner ruling 2 of 2026-09-25,
//! decision #21; Java `PlatformTokenFilterTest`), over the real routes with
//! real RS256 tokens from a fake platform's JWKS:
//!
//! - 401 without a token, with a forged, expired, identity or session
//!   token, and when there is no platform to verify against;
//! - 403 `PERMISSION_REQUIRED` without the route's permission (reads need
//!   `router:view`, everything else `router:operate`);
//! - 200 with it, super-admin's wildcard included;
//! - health, metrics, the dashboard page and its sign-in helpers stay open;
//! - the mock, test, benchmark and seed routes are absent outside dev mode;
//! - decision #43's transitional `AUTH_MODE=NONE` answers and says so on
//!   the health and monitoring output;
//! - the dashboard's PKCE sign-in helpers.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use fc_common::{MediationOutcome, Message};
use fc_platform_jwks::testing::{Claims, TestPlatform, AUTHORIZATION_ENDPOINT};
use fc_queue::QueuePublisher;
use fc_router::{
    api::{
        create_router_with_options, DashboardSignIn, PlatformAuth, RouterDeps, RouterOptions,
        ROUTER_OPERATE, ROUTER_VIEW,
    },
    HealthService, HealthServiceConfig, Mediator, QueueManager, WarningService,
    WarningServiceConfig,
};
use http_body_util::BodyExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tower::ServiceExt;

struct NoOpPublisher;

#[async_trait]
impl QueuePublisher for NoOpPublisher {
    fn identifier(&self) -> &str {
        "noop"
    }
    async fn publish(&self, _message: Message) -> fc_queue::Result<String> {
        Ok("noop".to_string())
    }
    async fn publish_batch(&self, messages: Vec<Message>) -> fc_queue::Result<Vec<String>> {
        Ok(messages.iter().map(|_| "noop".to_string()).collect())
    }
}

struct NoOpMediator;

#[async_trait]
impl Mediator for NoOpMediator {
    async fn mediate(&self, _message: &Message) -> MediationOutcome {
        MediationOutcome::success(200)
    }
}

fn build_app(options: RouterOptions) -> axum::Router {
    let manager = Arc::new(QueueManager::with_shared_mediator_for_testing(
        Arc::new(NoOpMediator) as Arc<dyn Mediator>,
    ));
    let warnings = Arc::new(WarningService::new(WarningServiceConfig::default()));
    let health = Arc::new(HealthService::new(
        HealthServiceConfig::default(),
        warnings.clone(),
    ));
    let breakers = manager.circuit_breaker_registry().clone();
    create_router_with_options(
        RouterDeps {
            publisher: Arc::new(NoOpPublisher),
            queue_manager: manager,
            warning_service: warnings,
            health_service: health,
            circuit_breaker_registry: breakers,
        },
        options,
    )
}

/// A router guarded by `platform`'s tokens, as deployed (no dev routes).
fn guarded(platform: &TestPlatform) -> axum::Router {
    build_app(RouterOptions {
        platform_auth: Some(PlatformAuth::new(Some(&platform.url))),
        ..RouterOptions::default()
    })
}

struct Answer {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

async fn call(
    app: &axum::Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Answer {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let response = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    Answer {
        status,
        headers,
        body,
    }
}

async fn get(app: &axum::Router, path: &str, token: Option<&str>) -> Answer {
    call(app, Method::GET, path, token, None).await
}

async fn post(app: &axum::Router, path: &str, token: Option<&str>) -> Answer {
    call(app, Method::POST, path, token, None).await
}

fn assert_unauthorized(a: &Answer, what: &str) {
    assert_eq!(a.status, StatusCode::UNAUTHORIZED, "{what}: {:?}", a.body);
    assert_eq!(a.body["error"], "UNAUTHORIZED", "{what}");
    assert_eq!(
        a.headers.get("www-authenticate").unwrap(),
        "Bearer realm=\"FlowCatalyst Router\"",
        "{what}"
    );
    assert_eq!(a.headers.get("x-auth-mode").unwrap(), "BEARER", "{what}");
}

#[tokio::test]
async fn no_token_and_bad_tokens_are_401() {
    let platform = TestPlatform::start().await;
    let app = guarded(&platform);
    let view = [ROUTER_VIEW, ROUTER_OPERATE];

    let a = get(&app, "/monitoring/pools", None).await;
    assert_unauthorized(&a, "no token");
    assert_eq!(a.body["message"], "missing bearer token");

    let garbage = get(&app, "/monitoring/pools", Some("not-a-jwt")).await;
    assert_unauthorized(&garbage, "garbage");

    let forged = platform.mint_forged(&Claims::api(&view));
    assert_unauthorized(
        &get(&app, "/monitoring/pools", Some(&forged)).await,
        "forged",
    );

    let expired = platform.mint(&Claims::api(&view).expires_in(-10));
    assert_unauthorized(
        &get(&app, "/monitoring/pools", Some(&expired)).await,
        "expired",
    );

    // An identity token (a relying party's login token) is refused...
    let identity = platform.mint(&Claims::api(&view).token_use(Some("identity")));
    let a = get(&app, "/monitoring/pools", Some(&identity)).await;
    assert_unauthorized(&a, "identity token");
    assert_eq!(
        a.body["message"],
        "an identity token is not an API credential"
    );

    // ... and so is a session cookie's token, which has no token_use.
    let session = platform.mint(&Claims::api(&view).token_use(None));
    let a = get(&app, "/monitoring/pools", Some(&session)).await;
    assert_unauthorized(&a, "session token");
    assert_eq!(a.body["message"], "an API access token is required");

    // Writes too.
    assert_unauthorized(&post(&app, "/messages", None).await, "publish, no token");
}

#[tokio::test]
async fn reads_need_view_and_writes_need_operate() {
    let platform = TestPlatform::start().await;
    let app = guarded(&platform);
    let nothing = platform.mint(&Claims::api(&["platform:iam:user:view"]));
    let view = platform.mint(&Claims::api(&[ROUTER_VIEW]));
    let operate = platform.mint(&Claims::api(&[ROUTER_OPERATE]));
    let both = platform.mint(&Claims::api(&[ROUTER_VIEW, ROUTER_OPERATE]));
    let super_admin = platform.mint(&Claims::api(&["platform:*:*:*"]));

    // 403 without the permission, naming it.
    let a = get(&app, "/monitoring/pools", Some(&nothing)).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);
    assert_eq!(a.body["error"], "PERMISSION_REQUIRED");
    assert_eq!(a.body["message"], format!("{ROUTER_VIEW} required"));

    let reset = "/monitoring/circuit-breakers/reset-all";
    let a = post(&app, reset, Some(&view)).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);
    assert_eq!(a.body["message"], format!("{ROUTER_OPERATE} required"));
    // operate does not imply view.
    let a = get(&app, "/monitoring/pools", Some(&operate)).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);

    // 200 with it.
    for token in [&view, &both, &super_admin] {
        let a = get(&app, "/monitoring/pools", Some(token)).await;
        assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
        assert_eq!(
            get(&app, "/monitoring/health", Some(token)).await.status,
            StatusCode::OK
        );
    }
    for token in [&operate, &both, &super_admin] {
        let a = post(&app, reset, Some(token)).await;
        assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    }

    // The in-flight checks done over POST are reads: the SDKs' stuck-message
    // recovery holds only view.
    let a = call(
        &app,
        Method::POST,
        "/monitoring/in-flight-messages/check-batch",
        Some(&view),
        Some(json!({"messageIds": ["m1"]})),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    assert_eq!(a.body["m1"], false);
    let a = get(
        &app,
        "/monitoring/in-flight-messages/check?messageId=m1",
        Some(&view),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);

    // Publishing is an operation.
    let publish =
        json!({"id": "m1", "poolCode": "DEFAULT", "mediationTarget": "http://localhost:9/x"});
    let a = call(
        &app,
        Method::POST,
        "/messages",
        Some(&view),
        Some(publish.clone()),
    )
    .await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);
    let a = call(
        &app,
        Method::POST,
        "/messages",
        Some(&operate),
        Some(publish),
    )
    .await;
    assert_ne!(a.status, StatusCode::FORBIDDEN);
    assert_ne!(a.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn public_routes_stay_open() {
    let platform = TestPlatform::start().await;
    let app = guarded(&platform);
    for path in [
        "/health",
        "/health/live",
        "/health/ready",
        "/health/startup",
        "/q/health",
        "/q/health/live",
        "/q/health/ready",
        "/metrics",
        "/q/metrics",
        "/dashboard.html",
        "/monitoring/dashboard",
        "/dashboard/auth-config",
    ] {
        let a = get(&app, path, None).await;
        assert_ne!(a.status, StatusCode::UNAUTHORIZED, "{path}");
        assert_ne!(a.status, StatusCode::FORBIDDEN, "{path}");
        assert_ne!(a.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn dev_routes_are_absent_outside_dev_mode() {
    let platform = TestPlatform::start().await;
    let operate = platform.mint(&Claims::api(&["platform:*:*:*"]));
    let deployed = guarded(&platform);
    let dev_paths = [
        "/api/test/fast",
        "/api/test/stats/reset",
        "/api/benchmark/process",
        "/api/benchmark/reset",
        "/api/seed/messages",
    ];
    for path in dev_paths {
        let a = post(&deployed, path, Some(&operate)).await;
        assert_eq!(
            a.status,
            StatusCode::NOT_FOUND,
            "{path}: absent, not protected"
        );
    }
    let a = get(&deployed, "/api/test/stats", Some(&operate)).await;
    assert_eq!(a.status, StatusCode::NOT_FOUND);

    // Mounted in dev mode (behind the same guard when one is set).
    let dev = build_app(RouterOptions {
        platform_auth: Some(PlatformAuth::new(Some(&platform.url))),
        dev_routes: true,
        ..RouterOptions::default()
    });
    assert_unauthorized(
        &post(&dev, "/api/test/fast", None).await,
        "dev route, no token",
    );
    let a = post(&dev, "/api/test/fast", Some(&operate)).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
}

#[tokio::test]
async fn no_platform_fails_closed() {
    let platform = TestPlatform::start().await;
    let app = build_app(RouterOptions {
        platform_auth: Some(PlatformAuth::new(None)),
        ..RouterOptions::default()
    });
    let token = platform.mint(&Claims::api(&["platform:*:*:*"]));
    let a = get(&app, "/monitoring/pools", Some(&token)).await;
    assert_unauthorized(&a, "no platform");
    assert!(a.body["message"]
        .as_str()
        .unwrap()
        .contains("no platform to verify tokens against"));
    assert_ne!(
        get(&app, "/health", None).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_guard_applies_under_the_mount_prefix_too() {
    let platform = TestPlatform::start().await;
    let app = build_app(RouterOptions {
        platform_auth: Some(PlatformAuth::new(Some(&platform.url))),
        router_http_prefix: Some("/router".into()),
        ..RouterOptions::default()
    });
    let view = platform.mint(&Claims::api(&[ROUTER_VIEW]));
    assert_unauthorized(&get(&app, "/router/monitoring/pools", None).await, "nested");
    assert_eq!(
        get(&app, "/router/monitoring/pools", Some(&view))
            .await
            .status,
        StatusCode::OK
    );
    // The permission is decided on the path below the prefix.
    let a = call(
        &app,
        Method::POST,
        "/router/monitoring/in-flight-messages/check-batch",
        Some(&view),
        Some(json!({"messageIds": []})),
    )
    .await;
    assert_ne!(a.status, StatusCode::FORBIDDEN, "{:?}", a.body);
    assert_ne!(
        get(&app, "/router/dashboard.html", None).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn transitional_none_answers_and_warns_on_the_health_output() {
    let warning = fc_router::api::platform_auth::UNAUTHENTICATED_WARNING;
    // Decision #43: AUTH_MODE=NONE outside dev mode, no guard, the warning.
    let open = build_app(RouterOptions {
        auth_warning: Some(warning.to_string()),
        ..RouterOptions::default()
    });
    assert_eq!(
        get(&open, "/monitoring/pools", None).await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&open, "/health", None).await.body["authWarning"],
        warning
    );
    assert_eq!(
        get(&open, "/monitoring/health", None).await.body["authWarning"],
        warning
    );
    assert_eq!(
        get(&open, "/monitoring", None).await.body["auth_warning"],
        warning
    );
    // The dev routes stay absent there too.
    assert_eq!(
        post(&open, "/api/test/fast", None).await.status,
        StatusCode::NOT_FOUND
    );

    // Guarded: no warning.
    let platform = TestPlatform::start().await;
    let view = platform.mint(&Claims::api(&[ROUTER_VIEW]));
    let app = guarded(&platform);
    assert!(get(&app, "/health", None)
        .await
        .body
        .get("authWarning")
        .is_none());
    assert!(get(&app, "/monitoring/health", Some(&view))
        .await
        .body
        .get("authWarning")
        .is_none());
}

// ── Diagnostics ────────────────────────────────────────────────────────────

/// `/diagnostics/*` sits behind the guard: view for the snapshots and
/// lookups, operate for the task dump (a GET that pauses the runtime).
#[tokio::test]
async fn diagnostics_need_view_and_the_task_dump_needs_operate() {
    let platform = TestPlatform::start().await;
    let app = guarded(&platform);
    let view = platform.mint(&Claims::api(&[ROUTER_VIEW]));
    let operate = platform.mint(&Claims::api(&[ROUTER_OPERATE]));

    for path in [
        "/diagnostics/runtime?sampleMs=0",
        "/diagnostics/task-dump",
        "/diagnostics/messages/m1",
        "/diagnostics/groups/g1",
        "/diagnostics/events",
    ] {
        assert_unauthorized(&get(&app, path, None).await, path);
    }

    let a = get(&app, "/diagnostics/runtime?sampleMs=0", Some(&view)).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    assert!(a.body["runtime"]["tokio"]["workers"].as_u64().is_some());
    assert!(a.body["router"]["flightRecorder"]["capacity"]
        .as_u64()
        .is_some());
    let a = get(&app, "/diagnostics/messages/m1", Some(&view)).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    assert_eq!(a.body["status"], "NOT_IN_PIPELINE");

    let a = get(&app, "/diagnostics/task-dump", Some(&view)).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);
    assert_eq!(a.body["message"], format!("{ROUTER_OPERATE} required"));

    let a = get(&app, "/diagnostics/task-dump", Some(&operate)).await;
    if fc_common::diagnostics::TASKDUMP_AVAILABLE {
        assert_eq!(a.status, StatusCode::OK);
    } else {
        assert_eq!(a.status, StatusCode::NOT_IMPLEMENTED, "{:?}", a.body);
        assert_eq!(a.body["error"], "TASK_DUMP_NOT_AVAILABLE");
    }
}

/// Never anonymous: on a router left open outside dev mode (decision #43's
/// transitional AUTH_MODE=NONE) the diagnostics refuse, while in dev mode
/// they answer.
#[tokio::test]
async fn diagnostics_refuse_on_an_open_router_outside_dev_mode() {
    let open = build_app(RouterOptions {
        auth_warning: Some(fc_router::api::platform_auth::UNAUTHENTICATED_WARNING.to_string()),
        ..RouterOptions::default()
    });
    for path in [
        "/diagnostics/runtime?sampleMs=0",
        "/diagnostics/task-dump",
        "/diagnostics/messages/m1",
        "/diagnostics/groups/g1",
        "/diagnostics/events",
    ] {
        let a = get(&open, path, None).await;
        assert_eq!(a.status, StatusCode::FORBIDDEN, "{path}: {:?}", a.body);
        assert_eq!(a.body["error"], "DIAGNOSTICS_REQUIRE_AUTH", "{path}");
    }

    let dev = build_app(RouterOptions {
        dev_routes: true,
        ..RouterOptions::default()
    });
    let a = get(&dev, "/diagnostics/runtime?sampleMs=0", None).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
}

// ── Dashboard sign-in (authorization code + PKCE) ──────────────────────────

/// A fake platform whose `/oauth/token` records the form it was sent and
/// answers `answer` with `status`.
async fn platform_with_token_endpoint(
    status: StatusCode,
    answer: Value,
) -> (TestPlatform, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let token = axum::Router::new().route(
        "/oauth/token",
        axum::routing::post(move |body: String| {
            let record = record.clone();
            let answer = answer.clone();
            async move {
                record.lock().push(body);
                (status, axum::Json(answer))
            }
        }),
    );
    (TestPlatform::start_with(token).await, seen)
}

fn with_sign_in(platform: &TestPlatform, client_id: Option<&str>) -> axum::Router {
    let guard = PlatformAuth::new(Some(&platform.url));
    let sign_in = Arc::new(DashboardSignIn::new(guard.key_source(), client_id));
    build_app(RouterOptions {
        platform_auth: Some(guard),
        dashboard_sign_in: Some(sign_in),
        ..RouterOptions::default()
    })
}

#[tokio::test]
async fn auth_config_names_the_platform_authorize_url_and_the_scope() {
    let platform = TestPlatform::start().await;
    let app = with_sign_in(&platform, Some("oac_dashboard"));
    let a = get(&app, "/dashboard/auth-config", None).await;
    assert_eq!(a.status, StatusCode::OK);
    assert_eq!(
        a.body,
        json!({
            "enabled": true,
            "authorizationEndpoint": AUTHORIZATION_ENDPOINT,
            "clientId": "oac_dashboard",
            "scope": "platform:messaging:router:view platform:messaging:router:operate",
        })
    );

    // Off without a client id, without a platform, and without the guard.
    let off =
        json!({"enabled": false, "authorizationEndpoint": null, "clientId": null, "scope": null});
    let no_client = with_sign_in(&platform, None);
    assert_eq!(
        get(&no_client, "/dashboard/auth-config", None).await.body,
        off
    );
    let no_platform = build_app(RouterOptions {
        platform_auth: Some(PlatformAuth::new(None)),
        dashboard_sign_in: Some(Arc::new(DashboardSignIn::new(None, Some("oac_dashboard")))),
        ..RouterOptions::default()
    });
    assert_eq!(
        get(&no_platform, "/dashboard/auth-config", None).await.body,
        off
    );
    let dev = build_app(RouterOptions {
        dev_routes: true,
        ..RouterOptions::default()
    });
    assert_eq!(get(&dev, "/dashboard/auth-config", None).await.body, off);
    let a = call(
        &dev,
        Method::POST,
        "/dashboard/token",
        None,
        Some(json!({"code": "c", "codeVerifier": "v", "redirectUri": "r"})),
    )
    .await;
    assert_eq!(a.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_code_exchange_is_proxied_as_the_public_client_and_trimmed() {
    let (platform, seen) = platform_with_token_endpoint(
        StatusCode::OK,
        json!({
            "access_token": "at-1",
            "token_type": "Bearer",
            "expires_in": 300,
            "refresh_token": "rt-1",
            "id_token": "idt-1",
        }),
    )
    .await;
    let app = with_sign_in(&platform, Some("oac_dashboard"));
    let a = call(
        &app,
        Method::POST,
        "/dashboard/token",
        None,
        Some(json!({
            "code": "code-1",
            "codeVerifier": "verifier-1",
            "redirectUri": "https://router.example.test/router/dashboard.html",
        })),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    // Only the access token and its lifetime: the refresh and ID tokens stay
    // out of the browser.
    assert_eq!(a.body, json!({"accessToken": "at-1", "expiresIn": 300}));

    let forms = seen.lock().clone();
    assert_eq!(forms.len(), 1);
    let form: Vec<(String, String)> = url::form_urlencoded::parse(forms[0].as_bytes())
        .into_owned()
        .collect();
    let field = |k: &str| {
        form.iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(field("grant_type"), Some("authorization_code"));
    assert_eq!(field("code"), Some("code-1"));
    assert_eq!(field("code_verifier"), Some("verifier-1"));
    assert_eq!(
        field("redirect_uri"),
        Some("https://router.example.test/router/dashboard.html")
    );
    assert_eq!(field("client_id"), Some("oac_dashboard"));
    assert_eq!(
        field("client_secret"),
        None,
        "a public client has no secret"
    );
}

#[tokio::test]
async fn a_refused_exchange_passes_on_the_oauth_error_code_only() {
    let (platform, _) = platform_with_token_endpoint(
        StatusCode::BAD_REQUEST,
        json!({"error": "invalid_grant", "error_description": "code expired"}),
    )
    .await;
    let app = with_sign_in(&platform, Some("oac_dashboard"));
    let body = json!({"code": "c", "codeVerifier": "v", "redirectUri": "r"});
    let a = call(&app, Method::POST, "/dashboard/token", None, Some(body)).await;
    assert_eq!(a.status, StatusCode::BAD_REQUEST);
    assert_eq!(a.body, json!({"error": "invalid_grant"}));

    // A malformed or incomplete request never reaches the platform.
    let a = call(
        &app,
        Method::POST,
        "/dashboard/token",
        None,
        Some(json!({"code": "c"})),
    )
    .await;
    assert_eq!(a.status, StatusCode::BAD_REQUEST);
}

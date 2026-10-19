//! The platform router: the route modules, then the cross-cutting layers.
//!
//! Every route lives in its module's `routes(ctx)` (e.g. `client::routes`),
//! which builds its own state from the [`PlatformContext`] and returns its
//! routes at their full paths, with any per-route-group layers (rate
//! limits, error mapping) applied there. This file only lists the modules
//! and adds what spans all of them: the OpenAPI documents and Swagger UI,
//! `/health`, Go's extractor-rejection envelope, the SPA, and the
//! profile-only gate. It imports no handler or state type
//! (`tests/it/route_wiring_convention_test.rs`).
//!
//! The binaries add their own layers on top (`AuthLayer`, tracing, CORS).

#![allow(
    clippy::absolute_paths,
    reason = "the module list: the route-wiring scanner reads each `crate::<module>::routes(ctx)`"
)]

use axum::{response::Json, routing::get, Router};
use utoipa_swagger_ui::SwaggerUi;

use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use std::sync::Arc;

// Health
pub const PATH_HEALTH: &str = "/health";

// Swagger
pub const PATH_SWAGGER_UI: &str = "/swagger-ui";
pub const PATH_OPENAPI_SPEC: &str = "/q/openapi";
/// Unfiltered OpenAPI spec including `/bff/*` paths that share handlers
/// with their `/api/*` siblings. The `/bff/*` tier is frontend-only and
/// not part of the SDK contract; this endpoint is for internal tooling
/// (full-surface exploration, developer portal previews) where the BFF
/// shapes are useful even though they aren't programmable.
pub const PATH_OPENAPI_SPEC_FULL: &str = "/q/openapi-full";

/// Assemble the full platform router and its published OpenAPI document
/// from `ctx`.
///
/// The returned `Router` includes all API routes, the health endpoint,
/// Swagger UI, and SPA serving (if `static_dir` is set). It does **not**
/// include auth middleware, CORS, or tracing layers: the binaries add those.
pub fn build(ctx: &PlatformContext) -> (Router, serde_json::Value) {
    // The route modules, in order. The order is part of the OpenAPI
    // document: utoipa keeps the first component schema of a name
    // (`StatusChangeResponse` is two types), and axum lists a path's
    // methods in the `Allow` header in the order they were merged.
    let AggregateRoutes { documented, plain } = AggregateRoutes::new()
        .merge(crate::event::routes(ctx))
        .merge(crate::event_type::routes(ctx))
        .merge(crate::process::routes(ctx))
        .merge(crate::scheduled_job::routes(ctx))
        .merge(crate::dispatch_job::routes(ctx))
        .merge(crate::client::routes(ctx))
        .merge(crate::principal::routes(ctx))
        .merge(crate::mfa::routes(ctx))
        .merge(crate::developer_credential::routes(ctx))
        .merge(crate::role::routes(ctx))
        .merge(crate::subscription::routes(ctx))
        .merge(crate::auth::routes(ctx))
        .merge(crate::audit::routes(ctx))
        .merge(crate::shared::routes(ctx))
        .merge(crate::function::routes(ctx))
        .merge(crate::dispatch_job_actions::routes(ctx))
        .merge(crate::app_docs::routes(ctx))
        .merge(crate::application::routes(ctx))
        .merge(crate::appusers::routes(ctx))
        .merge(crate::email_domain_mapping::routes(ctx))
        .merge(crate::service_account::routes(ctx))
        .merge(crate::platform_config::routes(ctx))
        .merge(crate::webauthn::routes(ctx))
        .merge(crate::dispatch_pool::routes(ctx))
        .merge(crate::connection::routes(ctx))
        .merge(crate::cors::routes(ctx))
        .merge(crate::identity_provider::routes(ctx))
        .merge(crate::login_attempt::routes(ctx))
        .merge(crate::portal::routes(ctx));

    // 1. The documented routes (auto-collected in the OpenAPI spec).
    let (router, mut openapi) = documented.split_for_parts();

    // Capture the full spec (including `/bff/*` paths) before we
    // strip BFF entries from the public surface. Served at
    // `PATH_OPENAPI_SPEC_FULL` for internal tooling — pre-serialised
    // once at boot since the spec is fixed for the process lifetime.
    let openapi_full_bytes: axum::body::Bytes = serde_json::to_vec(&openapi)
        .map(axum::body::Bytes::from)
        .unwrap_or_default();

    // Strip `/bff/*` paths from the spec. The BFF tier is internal to the
    // frontend and intentionally not part of the programmable surface; it
    // shouldn't appear in Swagger or `/q/openapi`. Some BFF routers share
    // handlers with their `/api/*` siblings and have to be mounted via
    // `OpenApiRouter` for routing, so we filter post-build rather than
    // requiring every contributor to remember the convention.
    openapi
        .paths
        .paths
        .retain(|path, _| !path.starts_with("/bff/"));

    // The operations Go documents that are routed through the plain
    // routes (`shared::openapi_contract`).
    openapi.merge(crate::shared::openapi_contract::documented_plain_routes());

    // 3. Set OpenAPI metadata
    openapi.info.title = "FlowCatalyst Platform API".to_string();
    openapi.info.version = fc_common::BUILD_VERSION.to_string();
    // No `info.description`: Go's document has none.
    openapi.info.description = None;
    // `OpenApiRouter::new()` seeds `info` from utoipa-axum's *own* crate
    // metadata (its author as contact, "MIT OR Apache-2.0" as license),
    // so these must be set explicitly or the published spec advertises
    // the wrong license.
    openapi.info.contact = Some(
        utoipa::openapi::ContactBuilder::new()
            .name(Some("FlowCatalyst"))
            .email(Some("support@flowcatalyst.io"))
            .build(),
    );
    openapi.info.license = Some(
        utoipa::openapi::LicenseBuilder::new()
            .name(env!("CARGO_PKG_LICENSE"))
            .identifier(Some(env!("CARGO_PKG_LICENSE")))
            .build(),
    );

    // 2. The published document, reshaped to Go's document conventions
    //    (one `default` ErrorModel response, optional members not
    //    nullable, no orphan schemas); see
    //    `shared::openapi_contract::shape_as_go_contract`. Served as JSON
    //    (utoipa's model cannot read back every schema it writes, e.g.
    //    `{}`), fixed for the process lifetime.
    let mut openapi = serde_json::to_value(&openapi).unwrap_or(serde_json::Value::Null);
    crate::shared::openapi_contract::shape_as_go_contract(&mut openapi);
    // 3. The SDK class of each operation, as the `x-sdk` vendor extension (an
    //    operation no list decides stays unstamped, i.e. internal; the tests
    //    fail on it: `tests/it/sdk_class_test.rs`).
    for problem in crate::shared::sdk_class::classify(&mut openapi) {
        tracing::warn!(%problem, "an operation has no SDK class; it is published as internal");
    }

    // Snapshot the platform's own OpenAPI document for the Developer
    // portal. Compile-time-derived from utoipa, so a single capture at
    // boot is correct for the lifetime of this binary; "Sync All" pushes
    // this value into the seeded `code='platform'` application row.
    let platform_openapi = Arc::new(openapi.clone());

    // 4. The plain routes (not in the OpenAPI document), after the
    //    developer portal, which needs the document.
    let app = Router::new()
        .merge(router)
        .merge(crate::shared::routes::developer_portal_routes(
            ctx,
            platform_openapi,
        ))
        .merge(plain);

    // Go's spec routes (internal/server/wire_spec.go): the programmable
    // document (BFF-stripped, as /q/openapi) as JSON and YAML, no auth.
    let app = app.merge(crate::shared::openapi_api::openapi_router(&openapi));

    let app = app
        // Health
        .route(PATH_HEALTH, get(health_handler))
        // Swagger UI (serves `/swagger-ui` + `/q/openapi`, BFF-stripped)
        .merge(
            SwaggerUi::new(PATH_SWAGGER_UI)
                .external_url_unchecked(PATH_OPENAPI_SPEC, openapi.clone()),
        )
        // Full OpenAPI spec including `/bff/*`. JSON only — not mounted
        // into Swagger UI to keep the default UI aligned with the SDK
        // contract. Body is pre-serialised at boot.
        .route(
            PATH_OPENAPI_SPEC_FULL,
            get({
                let body = openapi_full_bytes;
                move || {
                    let body = body.clone();
                    async move {
                        (
                            [(axum::http::header::CONTENT_TYPE, "application/json")],
                            body,
                        )
                    }
                }
            }),
        );

    // Extractor rejections (unreadable body, query or path) answer in
    // Go's envelope: 400 `VALIDATION`, or `invalid_request` on /oauth.
    let app = app.layer(axum::middleware::from_fn(
        crate::shared::rejection::go_rejections,
    ));

    // SPA serving (if static_dir is configured). No static_dir: no root
    // handler. The binary can add its own (fc-dev uses embedded assets,
    // fc-server may redirect to Swagger).
    let app = match ctx.config.static_dir {
        Some(ref static_dir) => serve_spa(app, static_dir),
        None => app,
    };

    // A USER with no platform role reaches only its own profile (Go
    // `ProfileOnlyWithoutRole`). Runs inside the binaries' `AuthLayer`,
    // which installs the auth services it authenticates with.
    let app = app.layer(axum::middleware::from_fn(
        crate::shared::profile_only::profile_only_without_role,
    ));

    // An unsafe request the session cookie authenticated must come from
    // this origin and carry JSON (Go `SessionCSRF`, F16); bearer callers
    // are not affected. Outside the profile-only gate, so it runs first
    // (right after the binaries' `AuthLayer`, as in Go) and a refused
    // request gets the same answer whatever the caller's roles. The public
    // base URL (the JWT issuer) is trusted as `Origin`, for a proxy that
    // rewrites `Host`.
    let app = app.layer(axum::middleware::from_fn_with_state(
        Arc::new(
            crate::shared::session_csrf::SessionCsrf::for_public_base_url(ctx.auth.auth.issuer()),
        ),
        crate::shared::session_csrf::session_csrf,
    ));

    (app, openapi)
}

/// Serve the SPA in `static_dir` under `app` ([`serve_spa_files`]). A
/// directory without `index.html` serves nothing.
pub fn serve_spa(app: Router, static_dir: &str) -> Router {
    let root = std::path::PathBuf::from(static_dir);
    if root.join("index.html").exists() {
        tracing::info!(dir = %static_dir, "Serving static frontend files with SPA fallback");
        serve_spa_files(app, crate::shared::spa::SpaFiles::Dir(root))
    } else {
        tracing::warn!(dir = %static_dir, "Static dir set but index.html not found");
        app
    }
}

/// Serve the SPA from `files` under `app` (Go `frontend.Handler`, the rules
/// in `shared::spa`): the answer for every path no route claims, and the
/// GET of the client routes that share a path with an API route registered
/// for another method (`POST /auth/login`: without these, axum answers the
/// GET with 405).
pub fn serve_spa_files(app: Router, files: crate::shared::spa::SpaFiles) -> Router {
    let files = Arc::new(files);
    let spa =
        move |method: axum::http::Method, uri: axum::http::Uri, headers: axum::http::HeaderMap| {
            let files = Arc::clone(&files);
            async move { crate::shared::spa::respond(&files, &method, &uri, &headers).await }
        };
    app.route("/auth/login", get(spa.clone()))
        .route("/auth/forgot-password", get(spa.clone()))
        .route("/auth/reset-password", get(spa.clone()))
        // Invites land on the set-password framing of the same page.
        .route("/auth/set-password", get(spa.clone()))
        .fallback(spa)
}

/// The platform API listener's timeouts (owner ruling 10): keep-alive idle
/// 75 s, 30 s to read a request, nothing while a handler runs. A function
/// artifact upload (up to 256 MiB) is read against a 30 s stall deadline
/// instead of a total one, as Java reads its streaming uploads.
pub fn listener_timeouts() -> fc_http_listener::ListenerTimeouts {
    fc_http_listener::ListenerTimeouts::new(
        r#"{"error":"REQUEST_TIMEOUT","code":"REQUEST_TIMEOUT","message":"the request was not received in time"}"#,
    )
    .with_streamed_uploads(is_artifact_upload)
}

/// `PUT /api/functions/{address}/artifacts/{digest}`.
fn is_artifact_upload(method: &axum::http::Method, uri: &axum::http::Uri) -> bool {
    let Some(rest) = uri.path().strip_prefix("/api/functions/") else {
        return false;
    };
    let segments: Vec<&str> = rest.split('/').collect();
    method == axum::http::Method::PUT
        && segments.len() == 3
        && segments[1] == "artifacts"
        && !segments[0].is_empty()
        && !segments[2].is_empty()
}

/// Serve the platform API on `listener` with [`listener_timeouts`] until
/// `shutdown` completes, then let in-flight requests finish. In place of
/// `axum::serve`, which has no per-connection timeouts.
pub async fn serve_api(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send,
) {
    fc_http_listener::serve(listener, app, listener_timeouts(), shutdown).await
}

// =============================================================================
// Health handler (simple inline version matching the binary crates)
// =============================================================================

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "UP",
        "version": fc_common::BUILD_VERSION
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn only_an_artifact_upload_is_read_against_a_stall_deadline() {
        let upload = |m: axum::http::Method, p: &str| is_artifact_upload(&m, &p.parse().unwrap());
        let digest = format!("sha256:{}", "a".repeat(64));
        assert!(upload(
            axum::http::Method::PUT,
            &format!("/api/functions/shop.default.hello/artifacts/{digest}")
        ));
        assert!(!upload(
            axum::http::Method::GET,
            &format!("/api/functions/shop.default.hello/artifacts/{digest}")
        ));
        assert!(!upload(
            axum::http::Method::PUT,
            "/api/functions/shop.default.hello"
        ));
        assert!(!upload(
            axum::http::Method::PUT,
            "/api/functions//artifacts/x"
        ));
        assert!(!upload(
            axum::http::Method::PUT,
            "/api/principals/x/artifacts/y"
        ));
        assert_eq!(
            listener_timeouts().request_read,
            fc_http_listener::REQUEST_READ
        );
    }

    /// Go frontend/handler_test.go's fixture: a shell, hashed assets, an
    /// unhashed root file.
    fn fixture() -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().unwrap();
        // A file outside the served directory, for the traversal check.
        std::fs::write(dir.path().join("outside-the-spa.txt"), "secret").unwrap();
        let dist = dir.path().join("dist");
        std::fs::create_dir_all(dist.join("assets")).unwrap();
        std::fs::write(dist.join("index.html"), "<html>shell</html>").unwrap();
        std::fs::write(dist.join("favicon.svg"), "<svg/>").unwrap();
        std::fs::write(dist.join("assets/index-abc123.js"), "1").unwrap();
        std::fs::write(dist.join("assets/index-abc123.css"), "a{}").unwrap();
        let app = serve_spa(
            Router::new().route("/api/real", axum::routing::post(|| async { "ok" })),
            dist.to_str().unwrap(),
        );
        (dir, app)
    }

    struct Got {
        status: StatusCode,
        headers: axum::http::HeaderMap,
        body: String,
    }

    impl Got {
        fn header(&self, name: &str) -> &str {
            self.headers
                .get(name)
                .map(|v| v.to_str().unwrap())
                .unwrap_or("")
        }
        fn is_index(&self) -> bool {
            self.body.contains("shell")
        }
    }

    async fn send(app: &Router, method: &str, path: &str, headers: &[(&str, &str)]) -> Got {
        let mut req = Request::builder().method(method).uri(path);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = http_body_util::BodyExt::collect(res.into_body())
            .await
            .unwrap()
            .to_bytes();
        Got {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).to_string(),
        }
    }

    /// Go `TestSPAFallbackForClientRoutes` and
    /// `TestAPIPrefixMatchesWholeSegment`.
    #[tokio::test]
    async fn client_routes_get_the_shell() {
        let (_dir, app) = fixture();
        for path in [
            "/",
            "/index.html",
            "/dashboard",
            "/clients/clt_0ABC",
            "/auth/login",
            "/auth/reset-password",
            "/authorization/roles/orders:admin",
            "/portal/login",
            "/apiary",
        ] {
            let got = send(&app, "GET", path, &[]).await;
            assert_eq!(got.status, StatusCode::OK, "{path}");
            assert!(got.is_index(), "{path}: {}", got.body);
        }
    }

    /// Go `TestMissingAssetIs404NotIndex`: a stale hashed chunk, a missing
    /// root file, never the shell, never cached.
    #[tokio::test]
    async fn a_missing_asset_is_a_404_not_the_shell() {
        let (_dir, app) = fixture();
        for path in [
            "/assets/index-OLDHASH.js",
            "/assets/Page-OLD.css",
            "/assets/",
            "/assets",
            "/assets/no-extension",
            "/favicon.ico",
            "/robots.txt",
            "/some/deep/chunk-x.js",
            "/manifest.webmanifest",
            "/logo.PNG",
        ] {
            let got = send(&app, "GET", path, &[]).await;
            assert_eq!(got.status, StatusCode::NOT_FOUND, "{path}");
            assert!(!got.is_index(), "{path}");
            assert_eq!(got.header("cache-control"), "no-store", "{path}");
        }
    }

    /// Go `TestUnknownAPIPathIs404JSON`, plus the method rules: an unknown
    /// API path is a JSON 404 for every method; a GET to an API route
    /// registered for another method stays 405.
    #[tokio::test]
    async fn an_unknown_api_path_is_a_json_404() {
        let (_dir, app) = fixture();
        for path in [
            "/api/nope",
            "/api",
            "/bff/nope",
            "/oauth/nope",
            "/.well-known/nope",
            "/control/nope",
            "/q/nope",
        ] {
            for method in ["GET", "POST", "DELETE"] {
                let got = send(&app, method, path, &[("accept", "text/html")]).await;
                assert_eq!(got.status, StatusCode::NOT_FOUND, "{method} {path}");
                assert_eq!(got.header("content-type"), "application/json");
                let body: serde_json::Value = serde_json::from_str(&got.body).unwrap();
                assert_eq!(body["error"], "NOT_FOUND", "{method} {path}");
                assert_eq!(body["message"], format!("No route for {method} {path}"));
                assert_eq!(got.header("cache-control"), "no-store");
            }
        }
        let got = send(&app, "GET", "/api/real", &[]).await;
        assert_eq!(got.status, StatusCode::METHOD_NOT_ALLOWED);
        assert!(!got.is_index());
    }

    /// Go `TestUnsafeMethodOnClientRouteIsNotServed`.
    #[tokio::test]
    async fn an_unsafe_method_on_a_client_route_is_405() {
        let (_dir, app) = fixture();
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            let got = send(&app, method, "/dashboard", &[]).await;
            assert_eq!(got.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
            assert!(!got.is_index());
            assert_eq!(got.header("allow"), "GET, HEAD");
        }
    }

    /// Go `TestCacheHeaders`: the shell revalidates with its ETag (304), the
    /// hashed assets are immutable, an unhashed root file revalidates.
    #[tokio::test]
    async fn cache_headers_and_revalidation() {
        let (_dir, app) = fixture();
        let index = send(&app, "GET", "/clients", &[]).await;
        assert_eq!(index.header("cache-control"), "no-cache");
        let etag = index.header("etag").to_string();
        assert!(etag.starts_with('"') && etag.len() == 34, "{etag}");
        for path in ["/", "/index.html", "/auth/login"] {
            assert_eq!(
                send(&app, "GET", path, &[]).await.header("etag"),
                etag,
                "{path}"
            );
        }
        let revalidated = send(&app, "GET", "/clients", &[("if-none-match", &etag)]).await;
        assert_eq!(revalidated.status, StatusCode::NOT_MODIFIED);
        assert!(revalidated.body.is_empty());
        assert_eq!(revalidated.header("etag"), etag);
        let other = send(&app, "GET", "/clients", &[("if-none-match", "\"other\"")]).await;
        assert_eq!(other.status, StatusCode::OK);

        let js = send(&app, "GET", "/assets/index-abc123.js", &[]).await;
        assert_eq!(js.status, StatusCode::OK);
        assert_eq!(
            js.header("cache-control"),
            "public, max-age=31536000, immutable"
        );
        assert!(
            js.header("content-type").contains("javascript"),
            "{}",
            js.header("content-type")
        );
        assert_eq!(js.body, "1");

        let favicon = send(&app, "GET", "/favicon.svg", &[]).await;
        assert_eq!(favicon.header("cache-control"), "no-cache");
        assert!(!favicon.header("etag").is_empty());
        assert_eq!(favicon.header("content-type"), "image/svg+xml");

        let head = send(&app, "HEAD", "/assets/index-abc123.css", &[]).await;
        assert_eq!(head.status, StatusCode::OK);
        assert!(head.body.is_empty());
        let head = send(&app, "HEAD", "/dashboard", &[]).await;
        assert_eq!(head.status, StatusCode::OK);
        assert_eq!(head.header("cache-control"), "no-cache");
    }

    /// Go `TestSecurityHeaders`: on every answer, and a CSP that restricts
    /// neither scripts, styles nor connections.
    #[tokio::test]
    async fn every_answer_carries_the_security_headers() {
        let (_dir, app) = fixture();
        for (method, path) in [
            ("GET", "/"),
            ("GET", "/dashboard"),
            ("GET", "/auth/login"),
            ("GET", "/assets/index-abc123.js"),
            ("GET", "/assets/missing.js"),
            ("GET", "/api/nope"),
            ("POST", "/dashboard"),
        ] {
            let got = send(&app, method, path, &[]).await;
            assert_eq!(got.header("x-content-type-options"), "nosniff", "{path}");
            assert_eq!(got.header("x-frame-options"), "DENY", "{path}");
            assert_eq!(got.header("referrer-policy"), "same-origin", "{path}");
            let csp = got.header("content-security-policy");
            for d in [
                "frame-ancestors 'none'",
                "object-src 'none'",
                "base-uri 'self'",
            ] {
                assert!(csp.contains(d), "{path}: {csp}");
            }
            for d in ["default-src", "script-src", "style-src", "connect-src"] {
                assert!(!csp.contains(d), "{path}: {csp}");
            }
        }
    }

    /// Go `TestPathTraversalIsNotServed`, encoded and plain.
    #[tokio::test]
    async fn path_traversal_is_not_served() {
        let (_dir, app) = fixture();
        for path in [
            "/assets/../../outside-the-spa.txt",
            "/assets/%2e%2e/%2e%2e/outside-the-spa.txt",
            "/..%2foutside-the-spa.txt",
        ] {
            let got = send(&app, "GET", path, &[]).await;
            assert!(!got.body.contains("secret"), "{path}");
        }
    }
}

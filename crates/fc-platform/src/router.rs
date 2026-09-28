//! The platform router: the route modules, then the cross-cutting layers.
//!
//! Every route lives in its module's `routes(ctx)` (e.g. `client::routes`),
//! which builds its own state from the [`PlatformContext`] and returns its
//! routes at their full paths, with any per-route-group layers (rate
//! limits, error mapping) applied there. This file only lists the modules
//! and adds what spans all of them: the OpenAPI documents and Swagger UI,
//! `/health`, Go's extractor-rejection envelope, the SPA, and the
//! profile-only gate. It imports no handler or state type
//! (`tests/route_wiring_convention_test.rs`).
//!
//! The binaries add their own layers on top (`AuthLayer`, tracing, CORS).

use axum::{
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
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

    (app, openapi)
}

/// Serve the SPA in `static_dir` under `app`: hashed `/assets/*` immutable,
/// the shell never cacheable, and the shell as the fallback for any path no
/// route claims. A directory without `index.html` serves nothing.
pub fn serve_spa(app: Router, static_dir: &str) -> Router {
    let index_path = std::path::PathBuf::from(static_dir).join("index.html");
    if index_path.exists() {
        use axum::http::header::CACHE_CONTROL;
        use axum::http::HeaderValue;
        use tower_http::services::{ServeDir, ServeFile};
        use tower_http::set_header::SetResponseHeaderLayer;

        tracing::info!(dir = %static_dir, "Serving static frontend files with SPA fallback");

        let assets_dir = std::path::PathBuf::from(static_dir).join("assets");
        let assets_service = tower::ServiceBuilder::new()
            .layer(SetResponseHeaderLayer::overriding(
                CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ))
            .service(ServeDir::new(&assets_dir));

        // SPA routes that conflict with API nests (e.g., /auth/login vs POST /auth/login).
        // Without these, the /auth nest returns 405 for GET requests the SPA should handle.
        let spa_index = index_path.clone();
        let spa_handler = get(move || {
            let path = spa_index.clone();
            async move {
                match tokio::fs::read_to_string(&path).await {
                    Ok(html) => (
                        [(CACHE_CONTROL, SPA_SHELL_CACHE_CONTROL)],
                        axum::response::Html(html),
                    )
                        .into_response(),
                    Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
        });

        // The shell — `/`, `/index.html` by name, and the SPA
        // fallback for any other path — is never cacheable, so a
        // browser never keeps a stale SPA after a deploy (Java
        // 8fd35a8b). Every other static file keeps default caching.
        let fallback_service = tower::ServiceBuilder::new()
            .layer(SetResponseHeaderLayer::overriding(
                CACHE_CONTROL,
                |res: &axum::http::Response<_>| {
                    is_html(res.headers())
                        .then(|| HeaderValue::from_static(SPA_SHELL_CACHE_CONTROL))
                },
            ))
            .service(ServeDir::new(static_dir).fallback(ServeFile::new(index_path)));

        app.route("/auth/login", spa_handler.clone())
            .route("/auth/forgot-password", spa_handler.clone())
            .route("/auth/reset-password", spa_handler.clone())
            // Invites land on the set-password framing of the same page.
            .route("/auth/set-password", spa_handler)
            .nest_service("/assets", assets_service)
            .fallback_service(fallback_service)
    } else {
        tracing::warn!(dir = %static_dir, "Static dir set but index.html not found");
        app
    }
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

/// `Cache-Control` of every SPA shell response (index.html, whether asked
/// for by name, as `/`, or as the fallback for a client-side route). Hashed
/// `/assets/*` keep `public, max-age=31536000, immutable`.
pub const SPA_SHELL_CACHE_CONTROL: &str = "no-cache, no-store, must-revalidate";

fn is_html(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"))
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
    use axum::http::{header::CACHE_CONTROL, Request, StatusCode};
    use tower::ServiceExt;

    async fn cache_control(app: &Router, path: &str) -> (StatusCode, Option<String>) {
        let res = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let cc = res
            .headers()
            .get(CACHE_CONTROL)
            .map(|v| v.to_str().unwrap().to_string());
        (res.status(), cc)
    }

    /// Java 8fd35a8b: every shell response is never cacheable; hashed
    /// assets stay immutable and other static files keep default caching.
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

    #[tokio::test]
    async fn the_spa_shell_is_never_cacheable_and_assets_stay_immutable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>shell</html>").unwrap();
        std::fs::write(dir.path().join("robots.txt"), "User-agent: *").unwrap();
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/index-abc123.js"), "1").unwrap();
        let app = serve_spa(Router::new(), dir.path().to_str().unwrap());

        let shell = Some(SPA_SHELL_CACHE_CONTROL.to_string());
        for path in [
            "/",
            "/index.html",
            "/applications/app_1",
            "/auth/login",
            "/auth/reset-password",
        ] {
            assert_eq!(
                cache_control(&app, path).await,
                (StatusCode::OK, shell.clone()),
                "{path}"
            );
        }
        assert_eq!(
            cache_control(&app, "/assets/index-abc123.js").await,
            (
                StatusCode::OK,
                Some("public, max-age=31536000, immutable".to_string())
            )
        );
        assert_eq!(
            cache_control(&app, "/robots.txt").await,
            (StatusCode::OK, None)
        );
    }
}

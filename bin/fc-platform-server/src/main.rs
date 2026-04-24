//! FlowCatalyst Platform Server
//!
//! Production server for platform REST APIs:
//! - BFF APIs: events, event-types, dispatch-jobs, filter-options
//! - Admin APIs: clients, principals, roles, subscriptions, etc.
//! - Monitoring APIs: health, metrics, leader status
//!
//! ## Environment Variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `FC_API_PORT` | `3000` | HTTP API port |
//! | `FC_METRICS_PORT` | `9090` | Metrics/health port |
//! | `FC_DATABASE_URL` | `postgresql://localhost:5432/flowcatalyst` | PostgreSQL connection URL |
//! | `FC_JWT_PRIVATE_KEY_PATH` | - | Path to RSA private key PEM |
//! | `FC_JWT_PUBLIC_KEY_PATH` | - | Path to RSA public key PEM |
//! | `FLOWCATALYST_JWT_PRIVATE_KEY` | - | RSA private key PEM content (env) |
//! | `FLOWCATALYST_JWT_PUBLIC_KEY` | - | RSA public key PEM content (env) |
//! | `FC_JWT_ISSUER` | `flowcatalyst` | JWT issuer claim |
//! | `RUST_LOG` | `info` | Log level |

use std::sync::Arc;
use axum::{
    routing::get,
    response::Json,
    Router,
};
use tower_http::cors::{CorsLayer, AllowOrigin};
// SetResponseHeaderLayer moved to PlatformRoutes
use tower_http::trace::TraceLayer;
// SPA serving moved to PlatformRoutes
use axum::http::{Method, HeaderValue, header as http_header};
use anyhow::Result;
use tracing::info;
use tokio::net::TcpListener;

use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::repository::{
    Repositories,
    CorsOriginRepository,
};
use fc_platform::usecase::PgUnitOfWork;
use fc_platform::seed::DevDataSeeder;


use fc_common::config::{env_or, env_or_parse};

#[tokio::main]
async fn main() -> Result<()> {
    fc_common::logging::init_logging("fc-platform-server");

    info!("Starting FlowCatalyst Platform Server");

    // Configuration from environment
    let api_port: u16 = env_or_parse("FC_API_PORT", 3000);
    let metrics_port: u16 = env_or_parse("FC_METRICS_PORT", 9090);
    let database_url = env_or("FC_DATABASE_URL", "postgresql://localhost:5432/flowcatalyst");
    let jwt_issuer = std::env::var("FC_JWT_ISSUER")
        .or_else(|_| std::env::var("FC_EXTERNAL_BASE_URL"))
        .or_else(|_| std::env::var("EXTERNAL_BASE_URL"))
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    // Connect to PostgreSQL
    info!("Connecting to PostgreSQL...");
    let pg_pool = fc_platform::shared::database::create_pool(&database_url).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL connection failed: {}", e))?;

    // Run PostgreSQL migrations
    fc_platform::shared::database::run_migrations(&pg_pool).await
        .map_err(|e| anyhow::anyhow!("PostgreSQL migrations failed: {}", e))?;

    // Referential-integrity scan — warns about orphaned junction rows.
    fc_platform::shared::integrity_scan::run(&pg_pool).await;

    // Seed development data if in dev mode
    let dev_mode = std::env::var("FC_DEV_MODE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    if dev_mode {
        let seeder = DevDataSeeder::new(pg_pool.clone());
        if let Err(e) = seeder.seed().await {
            tracing::warn!("Dev data seeding skipped (data may already exist): {}", e);
        }
    }

    // Initialize repositories
    let repos = Repositories::new(&pg_pool);
    info!("Repositories initialized");

    // Load CORS allowed origins from database into a shared cache
    let cors_origins_cache: Arc<std::sync::RwLock<std::collections::HashSet<String>>> =
        Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));
    {
        match repos.cors_repo.get_allowed_origins().await {
            Ok(origins) => {
                let mut cache = cors_origins_cache.write().unwrap();
                for origin in origins {
                    cache.insert(origin);
                }
                info!(count = cache.len(), "CORS origins loaded from database");
            }
            Err(e) => {
                tracing::warn!("Failed to load CORS origins: {}", e);
            }
        }
    }
    // Spawn background task to refresh CORS origins every 60 seconds
    {
        let cache = cors_origins_cache.clone();
        let cors_repo_bg = CorsOriginRepository::new(&pg_pool);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                match cors_repo_bg.get_allowed_origins().await {
                    Ok(origins) => {
                        let mut c = cache.write().unwrap();
                        c.clear();
                        for origin in origins {
                            c.insert(origin);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to refresh CORS origins: {}", e);
                    }
                }
            }
        });
    }

    // Sync code-defined roles to database (always, not just in dev mode)
    {
        let role_sync = fc_platform::service::RoleSyncService::new(
            fc_platform::repository::RoleRepository::new(&pg_pool)
        );
        if let Err(e) = role_sync.sync_code_defined_roles().await {
            tracing::warn!("Role sync failed: {}", e);
        }
    }

    // Initialize auth services (load or generate RSA keys, build services)
    let auth_init_config = fc_platform::shared::server_setup::AuthInitConfig {
        issuer: jwt_issuer,
        ..fc_platform::shared::server_setup::AuthInitConfig::from_env("http://localhost:3000")
    };
    let auth_services = fc_platform::shared::server_setup::init_auth_services(&repos, auth_init_config)?;
    info!("Auth services initialized");

    // Create AppState
    let app_state = AppState {
        auth_service: auth_services.auth.clone(),
        authz_service: auth_services.authz.clone(),
    };

    // Create UnitOfWork for atomic commits with events and audit logs
    let unit_of_work = Arc::new(PgUnitOfWork::new(pg_pool.clone()));

    // Build platform API router via shared builder (handles ~38 state structs)
    let routes = fc_platform::shared::server_setup::build_platform_routes(
        &repos,
        &auth_services,
        &unit_of_work,
        fc_platform::shared::server_setup::PlatformRoutesConfig {
            event_dispatch: None,
            session_cookie_secure: false,
            static_dir: std::env::var("FC_STATIC_DIR").ok(),
            oidc_login_external_base_url: std::env::var("FC_EXTERNAL_BASE_URL").ok(),
            well_known_external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
                .unwrap_or_else(|_| format!("http://localhost:{}", api_port)),
            password_reset_external_base_url: std::env::var("FC_EXTERNAL_BASE_URL")
                .unwrap_or_else(|_| format!("http://localhost:{}", api_port)),
        },
    );
    let (app, _openapi) = routes.build();

    // Add middleware layers
    let app = app
        .layer(AuthLayer::new(app_state))
        .layer(TraceLayer::new_for_http())
        .layer({
            let cache = cors_origins_cache.clone();
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _parts| {
                    let origin_str = match origin.to_str() {
                        Ok(s) => s,
                        Err(_) => return false,
                    };
                    let origins = cache.read().unwrap();
                    // Check exact match first
                    if origins.contains(origin_str) {
                        return true;
                    }
                    // Check wildcard patterns (e.g., https://*.example.com)
                    for pattern in origins.iter() {
                        if pattern.contains('*') {
                            let regex_str = format!(
                                "^{}$",
                                regex::escape(pattern).replace(r"\*", "[a-zA-Z0-9-]+")
                            );
                            if let Ok(re) = regex::Regex::new(&regex_str) {
                                if re.is_match(origin_str) {
                                    return true;
                                }
                            }
                        }
                    }
                    false
                }))
                .allow_methods([
                    Method::GET, Method::POST, Method::PUT, Method::PATCH,
                    Method::DELETE, Method::OPTIONS, Method::HEAD,
                ])
                .allow_headers([
                    http_header::AUTHORIZATION,
                    http_header::CONTENT_TYPE,
                    http_header::ACCEPT,
                    http_header::ORIGIN,
                    http_header::HeaderName::from_static("x-requested-with"),
                    http_header::HeaderName::from_static("x-client-id"),
                ])
                .allow_credentials(true)
                .max_age(std::time::Duration::from_secs(86400))
        });

    // SPA serving is now handled by PlatformRoutes::build() via the static_dir field.

    // Start API server
    let api_addr = format!("0.0.0.0:{}", api_port);
    info!("API server listening on http://{}", api_addr);

    let api_listener = TcpListener::bind(&api_addr).await?;
    let api_task = tokio::spawn(async move {
        axum::serve(api_listener, app).await.unwrap();
    });

    // Start metrics server
    let metrics_addr = format!("0.0.0.0:{}", metrics_port);
    info!("Metrics server listening on http://{}/metrics", metrics_addr);

    let metrics_app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler));

    let metrics_listener = TcpListener::bind(&metrics_addr).await?;
    let metrics_task = tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_app).await.unwrap();
    });

    info!("FlowCatalyst Platform Server started");
    info!("Press Ctrl+C to shutdown");

    // Wait for shutdown
    fc_platform::shared::server_setup::wait_for_shutdown_signal().await;
    info!("Shutdown signal received...");

    api_task.abort();
    metrics_task.abort();

    info!("FlowCatalyst Platform Server shutdown complete");
    Ok(())
}

async fn metrics_handler() -> &'static str {
    "# HELP fc_platform_up Platform is up\n# TYPE fc_platform_up gauge\nfc_platform_up 1\n"
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "UP",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn ready_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "READY"
    }))
}


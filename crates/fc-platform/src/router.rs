//! Centralized Platform Router Builder
//!
//! Eliminates duplicated route wiring across binary crates (fc-server,
//! fc-platform-server, fc-dev). Each binary still constructs the state
//! objects and adds its own middleware/static-file layers on top.

use axum::{routing::get, response::{Json, IntoResponse}, Router};
use utoipa::openapi::{ObjectBuilder, schema::Type};
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use crate::api::{
    // OpenApiRouter routes
    events_router, admin_events_router, EventsState,
    event_types_router, EventTypesState,
    dispatch_jobs_router, DispatchJobsState,
    filter_options_router, FilterOptionsState,
    clients_router, ClientsState,
    principals_router, PrincipalsState,
    roles_router, RolesState,
    subscriptions_router, SubscriptionsState,
    oauth_clients_router, OAuthClientsState,
    audit_logs_router, AuditLogsState,
    monitoring_router, MonitoringState,
    auth_router, AuthState,
    // Plain Router routes
    bff_roles_router, BffRolesState,
    bff_event_types_router, BffEventTypesState,
    debug_events_router, debug_dispatch_jobs_router, DebugState,
    anchor_domains_router, client_auth_configs_router, idp_role_mappings_router, AuthConfigState,
    applications_router, ApplicationsState,
    dispatch_pools_router, DispatchPoolsState,
    service_accounts_router, ServiceAccountsState,
    connections_router, ConnectionsState,
    cors_router, CorsState,
    identity_providers_router, IdentityProvidersState,
    email_domain_mappings_router, EmailDomainMappingsState,
    admin_platform_config_router, PlatformConfigState,
    config_access_router, ConfigAccessState,
    login_attempts_router, LoginAttemptsState,
    me_router, MeState,
    sdk_events_batch_router, SdkEventsState,
    sdk_dispatch_jobs_batch_router, SdkDispatchJobsState,
    oidc_login_router, OidcLoginApiState,
    oauth_router, OAuthState,
    well_known_router, WellKnownState,
    client_selection_router, ClientSelectionState,
    application_roles_sdk_router, ApplicationRolesSdkState,
    sdk_sync_router, SdkSyncState,
    sdk_audit_batch_router, SdkAuditBatchState,
    platform_config_router,
    public_router, PublicApiState,
    password_reset_router, PasswordResetApiState,
    dispatch_process_router, DispatchProcessState,
};
use crate::usecase::UnitOfWork;

// =============================================================================
// Route path constants
// =============================================================================

// BFF routes
pub const PATH_BFF_EVENTS: &str = "/bff/events";
pub const PATH_BFF_DISPATCH_JOBS: &str = "/bff/dispatch-jobs";
pub const PATH_BFF_FILTER_OPTIONS: &str = "/bff/filter-options";
pub const PATH_BFF_ROLES: &str = "/bff/roles";
pub const PATH_BFF_EVENT_TYPES: &str = "/bff/event-types";
pub const PATH_BFF_DEBUG_EVENTS: &str = "/bff/debug/events";
pub const PATH_BFF_DEBUG_DISPATCH_JOBS: &str = "/bff/debug/dispatch-jobs";

// API routes (single programmable surface; gated by permissions, not URL tier)
pub const PATH_API_EVENTS: &str = "/api/events";
pub const PATH_API_EVENT_TYPES: &str = "/api/event-types";
pub const PATH_API_CLIENTS: &str = "/api/clients";
pub const PATH_API_PRINCIPALS: &str = "/api/principals";
pub const PATH_API_ROLES: &str = "/api/roles";
pub const PATH_API_SUBSCRIPTIONS: &str = "/api/subscriptions";
pub const PATH_API_OAUTH_CLIENTS: &str = "/api/oauth-clients";
// Audit logs admin CRUD reuses the same prefix as batch ingest; the two routers
// occupy non-overlapping sub-paths, so both nest at `/api/audit-logs`.
pub const PATH_API_ANCHOR_DOMAINS: &str = "/api/anchor-domains";
pub const PATH_API_AUTH_CONFIGS: &str = "/api/auth-configs";
pub const PATH_API_IDP_ROLE_MAPPINGS: &str = "/api/idp-role-mappings";
pub const PATH_API_DISPATCH_JOBS: &str = "/api/dispatch-jobs";
pub const PATH_API_DISPATCH_POOLS: &str = "/api/dispatch-pools";
pub const PATH_API_SERVICE_ACCOUNTS: &str = "/api/service-accounts";
pub const PATH_API_CONNECTIONS: &str = "/api/connections";
pub const PATH_API_CORS: &str = "/api/platform/cors";
pub const PATH_API_IDENTITY_PROVIDERS: &str = "/api/identity-providers";
pub const PATH_API_EMAIL_DOMAIN_MAPPINGS: &str = "/api/email-domain-mappings";
// Admin config reuses `/api/config`; shared with platform_config_router on
// non-overlapping sub-paths.
pub const PATH_API_CONFIG_ACCESS: &str = "/api/config-access";
pub const PATH_API_LOGIN_ATTEMPTS: &str = "/api/login-attempts";

// Monitoring
pub const PATH_MONITORING: &str = "/api/monitoring";

// Auth routes
pub const PATH_AUTH: &str = "/auth";
/// User-facing "me" routes (my clients, my applications, etc.). Mounted under
/// `/api/me` to match the TypeScript platform — note this is distinct from the
/// OIDC session-user endpoint `/auth/me` served by `auth_router`.
pub const PATH_API_ME: &str = "/api/me";
pub const PATH_AUTH_CLIENT: &str = "/auth/client";
pub const PATH_AUTH_PASSWORD_RESET: &str = "/auth/password-reset";

// OAuth / OIDC
pub const PATH_OAUTH: &str = "/oauth";
pub const PATH_WELL_KNOWN: &str = "/.well-known";

// NOTE: the legacy `/api/sdk/*` tier was consolidated into `/api/*`. Batch
// ingest endpoints (events, dispatch-jobs) now live under their resource's
// main router; all other SDK-tier CRUD was duplicative and has been removed.

// Dispatch processing (internal callback from message router)
pub const PATH_API_DISPATCH: &str = "/api/dispatch";

// Public / shared API routes
pub const PATH_API_APPLICATIONS: &str = "/api/applications";
pub const PATH_API_AUDIT_LOGS: &str = "/api/audit-logs";
pub const PATH_API_CONFIG: &str = "/api/config";
pub const PATH_API_PUBLIC: &str = "/api/public";

// Health
pub const PATH_HEALTH: &str = "/health";

// Swagger
pub const PATH_SWAGGER_UI: &str = "/swagger-ui";
pub const PATH_OPENAPI_SPEC: &str = "/q/openapi";

// =============================================================================
// PlatformRoutes
// =============================================================================

/// Holds all pre-constructed API state structs and assembles the full
/// platform router. Binaries create this after building repos/services,
/// call `build()`, then layer on middleware and static files.
pub struct PlatformRoutes<U: UnitOfWork + Clone + 'static> {
    // -- OpenApiRouter routes (collected in Swagger) --
    pub events: EventsState,
    pub event_types: EventTypesState,
    pub dispatch_jobs: DispatchJobsState,
    pub filter_options: FilterOptionsState,
    pub clients: ClientsState,
    pub principals: PrincipalsState,
    pub roles: RolesState,
    pub subscriptions: SubscriptionsState,
    pub oauth_clients: OAuthClientsState,
    pub audit_logs: AuditLogsState,
    pub monitoring: MonitoringState,
    pub auth: AuthState,

    // -- Plain Router routes (NOT in Swagger) --
    pub bff_roles: BffRolesState,
    pub bff_event_types: BffEventTypesState,
    pub debug: DebugState,
    pub auth_config: AuthConfigState,
    pub applications: ApplicationsState<U>,
    pub dispatch_pools: DispatchPoolsState<U>,
    pub service_accounts: ServiceAccountsState<U>,
    pub connections: ConnectionsState,
    pub cors: CorsState,
    pub identity_providers: IdentityProvidersState,
    pub email_domain_mappings: EmailDomainMappingsState,
    pub platform_config: PlatformConfigState,
    pub config_access: ConfigAccessState,
    pub login_attempts: LoginAttemptsState,
    pub me: MeState,
    pub sdk_events: SdkEventsState,
    pub sdk_dispatch_jobs: SdkDispatchJobsState,
    pub oidc_login: OidcLoginApiState,
    pub oauth: OAuthState,
    pub well_known: WellKnownState,
    pub client_selection: ClientSelectionState,
    pub application_roles_sdk: ApplicationRolesSdkState,
    pub sdk_sync: SdkSyncState,
    pub sdk_audit_batch: SdkAuditBatchState,
    pub public: PublicApiState,
    pub password_reset: PasswordResetApiState,
    /// Optional — dispatch processing endpoint state. None when dispatch processing
    /// is not needed (e.g., tests or standalone platform server without router).
    pub dispatch_process: Option<DispatchProcessState>,

    /// Optional static directory for SPA serving. When set, serves:
    /// - `/assets/*` with immutable cache headers (Vite hashed assets)
    /// - SPA fallback (index.html) for unmatched GET requests
    /// - Explicit SPA routes for paths that conflict with API nests (e.g., /auth/login)
    pub static_dir: Option<String>,
}

impl<U: UnitOfWork + Clone + 'static> PlatformRoutes<U> {
    /// Assemble the full platform router and OpenAPI spec.
    ///
    /// The returned `Router` includes all API routes, the health endpoint,
    /// Swagger UI, and SPA serving (if `static_dir` is set).
    /// It does **not** include auth middleware, CORS, or tracing layers.
    pub fn build(self) -> (Router, utoipa::openapi::OpenApi) {
        // 1. OpenApiRouter routes (auto-collected in Swagger spec)
        let (router, mut openapi) = OpenApiRouter::new()
            .nest(PATH_API_EVENTS, admin_events_router(self.events.clone()).into())
            .nest(PATH_BFF_EVENTS, events_router(self.events))
            .nest(PATH_API_EVENT_TYPES, event_types_router(self.event_types))
            .nest(PATH_BFF_DISPATCH_JOBS, dispatch_jobs_router(self.dispatch_jobs))
            .nest(PATH_BFF_FILTER_OPTIONS, filter_options_router(self.filter_options))
            .nest(PATH_API_CLIENTS, clients_router(self.clients))
            .nest(PATH_API_PRINCIPALS, principals_router(self.principals))
            .nest(PATH_API_ROLES, roles_router(self.roles))
            .nest(PATH_API_SUBSCRIPTIONS, subscriptions_router(self.subscriptions))
            .nest(PATH_API_OAUTH_CLIENTS, oauth_clients_router(self.oauth_clients))
            .nest(PATH_API_AUDIT_LOGS, audit_logs_router(self.audit_logs))
            .nest(PATH_MONITORING, monitoring_router(self.monitoring))
            .nest(PATH_AUTH, auth_router(self.auth))
            .split_for_parts();

        // 2. Add PaginationParams schema (referenced via #[serde(flatten)], not auto-collected)
        if let Some(components) = openapi.components.as_mut() {
            components.schemas.insert(
                "PaginationParams".to_string(),
                ObjectBuilder::new()
                    .property("page", ObjectBuilder::new().schema_type(Type::Integer))
                    .property("limit", ObjectBuilder::new().schema_type(Type::Integer))
                    .into(),
            );
        }

        // 3. Set OpenAPI metadata
        openapi.info.title = "FlowCatalyst Platform API".to_string();
        openapi.info.version = "1.0.0".to_string();
        openapi.info.description =
            Some("REST APIs for events, subscriptions, and administration".to_string());

        // 4. Merge plain Router routes (not in Swagger)
        let app = Router::new()
            .merge(router)
            // BFF
            .nest(PATH_BFF_ROLES, bff_roles_router(self.bff_roles).into())
            .nest(PATH_BFF_EVENT_TYPES, bff_event_types_router(self.bff_event_types).into())
            .nest(PATH_BFF_DEBUG_EVENTS, debug_events_router(self.debug.clone()))
            .nest(PATH_BFF_DEBUG_DISPATCH_JOBS, debug_dispatch_jobs_router(self.debug))
            // API — auth config
            .nest(PATH_API_ANCHOR_DOMAINS, anchor_domains_router(self.auth_config.clone()))
            .nest(PATH_API_AUTH_CONFIGS, client_auth_configs_router(self.auth_config.clone()))
            .nest(PATH_API_IDP_ROLE_MAPPINGS, idp_role_mappings_router(self.auth_config))
            // API — domain aggregates
            .nest(PATH_API_APPLICATIONS, applications_router(self.applications))
            .nest(PATH_API_DISPATCH_POOLS, dispatch_pools_router(self.dispatch_pools))
            .nest(PATH_API_SERVICE_ACCOUNTS, service_accounts_router(self.service_accounts))
            .nest(PATH_API_CONNECTIONS, connections_router(self.connections).into())
            .nest(PATH_API_CORS, cors_router(self.cors))
            .nest(PATH_API_IDENTITY_PROVIDERS, identity_providers_router(self.identity_providers))
            .nest(PATH_API_EMAIL_DOMAIN_MAPPINGS, email_domain_mappings_router(self.email_domain_mappings).into())
            .nest(PATH_API_CONFIG, admin_platform_config_router(self.platform_config).into())
            .nest(PATH_API_CONFIG_ACCESS, config_access_router(self.config_access).into())
            .nest(PATH_API_LOGIN_ATTEMPTS, login_attempts_router(self.login_attempts))
            // Auth
            .nest(PATH_API_ME, me_router(self.me))
            .nest(PATH_AUTH, oidc_login_router(self.oidc_login))
            .nest(PATH_OAUTH, oauth_router(self.oauth))
            .nest(PATH_WELL_KNOWN, well_known_router(self.well_known))
            .nest(PATH_AUTH_CLIENT, client_selection_router(self.client_selection))
            .nest(PATH_AUTH_PASSWORD_RESET, password_reset_router(self.password_reset))
            // Batch ingest endpoints (merged into resource routers)
            .nest(PATH_API_EVENTS, sdk_events_batch_router(self.sdk_events))
            .nest(PATH_API_DISPATCH_JOBS, sdk_dispatch_jobs_batch_router(self.sdk_dispatch_jobs))
            // Shared API
            .nest(PATH_API_APPLICATIONS, application_roles_sdk_router(self.application_roles_sdk))
            .nest(PATH_API_APPLICATIONS, sdk_sync_router(self.sdk_sync))
            .nest(PATH_API_AUDIT_LOGS, sdk_audit_batch_router(self.sdk_audit_batch))
            .nest(PATH_API_CONFIG, platform_config_router())
            // Public
            .nest(PATH_API_PUBLIC, public_router(self.public));

        // Dispatch processing (optional — only when message router callback is needed)
        let app = if let Some(dispatch_process) = self.dispatch_process {
            app.nest(PATH_API_DISPATCH, dispatch_process_router(dispatch_process))
        } else {
            app
        };

        let app = app
            // Health
            .route(PATH_HEALTH, get(health_handler))
            // Swagger UI
            .merge(SwaggerUi::new(PATH_SWAGGER_UI).url(PATH_OPENAPI_SPEC, openapi.clone()));

        // SPA serving (if static_dir is configured)
        let app = if let Some(ref static_dir) = self.static_dir {
            let index_path = std::path::PathBuf::from(static_dir).join("index.html");
            if index_path.exists() {
                use tower_http::services::{ServeDir, ServeFile};
                use tower_http::set_header::SetResponseHeaderLayer;
                use axum::http::header::CACHE_CONTROL;
                use axum::http::HeaderValue;

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
                            Ok(html) => axum::response::Html(html).into_response(),
                            Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                        }
                    }
                });

                app
                    .route("/auth/login", spa_handler.clone())
                    .route("/auth/forgot-password", spa_handler.clone())
                    .route("/auth/reset-password", spa_handler)
                    .nest_service("/assets", assets_service)
                    .fallback_service(
                        ServeDir::new(static_dir)
                            .fallback(ServeFile::new(index_path))
                    )
            } else {
                tracing::warn!(dir = %static_dir, "Static dir set but index.html not found");
                app
            }
        } else {
            // No static_dir — don't add a root handler. The binary can add its own
            // (fc-dev uses embedded assets, fc-server/fc-platform-server may redirect to Swagger).
            app
        };

        (app, openapi)
    }
}

// =============================================================================
// Health handler (simple inline version matching the binary crates)
// =============================================================================

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "UP",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

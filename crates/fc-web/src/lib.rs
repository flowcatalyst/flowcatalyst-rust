//! FlowCatalyst server-rendered admin UI (Topcoat trial).
//!
//! Mounted as the axum app's fallback service: pages live under `/ui/*`,
//! Topcoat's own assets and runtime endpoints under `/_topcoat/*`, and every
//! other path falls through to the existing Vue SPA service handed to
//! [`service`].
//!
//! Authentication is done here, from the `fc_session` cookie (or a Bearer
//! header), not taken from the axum `AuthLayer` extension: in fc-dev the
//! fallback sits outside that layer, and Topcoat's runtime WebSocket
//! re-renders drop request extensions. See [`auth`].

mod app;
mod assets;
pub mod auth;
mod ui;

use std::sync::Arc;

use bytes::Bytes;
use fc_platform::api::AppState;
use fc_platform::auth::auth_api::AuthState;
use fc_platform::auth::login_backoff::BackoffPolicy;
use fc_platform::auth::session_cookie::SessionCookieConfig;
use fc_platform::repository::Repositories;
use fc_platform::shared::server_setup::AuthServices;
use fc_platform::{
    AnchorDomainRepository, ApplicationRepository, AuditLogRepository, ClientRepository,
    EmailDomainMappingRepository, EventTypeRepository, IdentityProviderRepository, PgUnitOfWork,
    PlatformConfigRepository, PrincipalRepository,
};
use topcoat::asset::RouterBuilderAssetExt;
use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::router::tower::{TowerRoute, TowerService};
use topcoat::router::{Compression, Router, RouterBuilderDiscoverExt};
use topcoat::runtime::RouterBuilderRuntimeExt;

pub use topcoat::router::request::Request as WebRequest;

/// Everything the UI reads or writes through. Registered as Topcoat app
/// context; pages reach it with [`deps`].
pub struct WebDeps {
    pub(crate) app_state: AppState,
    pub(crate) auth_state: AuthState,
    pub(crate) unit_of_work: Arc<PgUnitOfWork>,
    pub(crate) audit_log_repo: Arc<AuditLogRepository>,
    pub(crate) principal_repo: Arc<PrincipalRepository>,
    pub(crate) event_type_repo: Arc<EventTypeRepository>,
    pub(crate) anchor_domain_repo: Arc<AnchorDomainRepository>,
    pub(crate) edm_repo: Arc<EmailDomainMappingRepository>,
    pub(crate) idp_repo: Arc<IdentityProviderRepository>,
    pub(crate) platform_config_repo: Arc<PlatformConfigRepository>,
    pub(crate) application_repo: Arc<ApplicationRepository>,
    pub(crate) client_repo: Arc<ClientRepository>,
    pub(crate) subscription_repo: Arc<fc_platform::SubscriptionRepository>,
    pub(crate) service_account_repo: Arc<fc_platform::ServiceAccountRepository>,
    pub(crate) connection_repo: Arc<fc_platform::ConnectionRepository>,
    pub(crate) dispatch_pool_repo: Arc<fc_platform::DispatchPoolRepository>,
    pub(crate) oauth_client_repo: Arc<fc_platform::OAuthClientRepository>,
    pub(crate) application_client_config_repo: Arc<fc_platform::ApplicationClientConfigRepository>,
    pub(crate) event_repo: Arc<fc_platform::EventRepository>,
    pub(crate) dispatch_job_repo: Arc<fc_platform::DispatchJobRepository>,
}

impl WebDeps {
    /// Build from the same pieces every binary already has, the way
    /// `build_platform_routes` builds the API states.
    /// `session_cookie_secure` mirrors `PlatformRoutesConfig::session_cookie_secure`.
    pub fn new(
        repos: &Repositories,
        auth: &AuthServices,
        unit_of_work: Arc<PgUnitOfWork>,
        session_cookie_secure: bool,
    ) -> Self {
        let auth_state = AuthState {
            auth_service: auth.auth.clone(),
            principal_repo: repos.principal_repo.clone(),
            role_repo: repos.role_repo.clone(),
            password_service: auth.password.clone(),
            refresh_token_repo: repos.refresh_token_repo.clone(),
            email_domain_mapping_repo: repos.edm_repo.clone(),
            identity_provider_repo: repos.idp_repo.clone(),
            login_attempt_repo: repos.login_attempt_repo.clone(),
            backoff_policy: Arc::new(BackoffPolicy::from_env()),
            // Same cookie as password login through `/auth/login`.
            session_cookie: SessionCookieConfig::password_login(session_cookie_secure),
        };
        Self {
            app_state: AppState {
                auth_service: auth.auth.clone(),
                authz_service: auth.authz.clone(),
            },
            auth_state,
            unit_of_work,
            audit_log_repo: repos.audit_log_repo.clone(),
            principal_repo: repos.principal_repo.clone(),
            event_type_repo: repos.event_type_repo.clone(),
            anchor_domain_repo: repos.anchor_domain_repo.clone(),
            edm_repo: repos.edm_repo.clone(),
            idp_repo: repos.idp_repo.clone(),
            platform_config_repo: repos.platform_config_repo.clone(),
            application_repo: repos.application_repo.clone(),
            client_repo: repos.client_repo.clone(),
            subscription_repo: repos.subscription_repo.clone(),
            service_account_repo: repos.service_account_repo.clone(),
            connection_repo: repos.connection_repo.clone(),
            dispatch_pool_repo: repos.dispatch_pool_repo.clone(),
            oauth_client_repo: repos.oauth_client_repo.clone(),
            application_client_config_repo: repos.application_client_config_repo.clone(),
            event_repo: repos.event_repo.clone(),
            dispatch_job_repo: repos.dispatch_job_repo.clone(),
        }
    }
}

/// The shared dependencies, from any handler or component.
pub(crate) fn deps(cx: &topcoat::context::Cx) -> &WebDeps {
    topcoat::context::app_context::<Arc<WebDeps>>(cx)
}

/// The UI as a tower service, for axum's `fallback_service`. Requests no
/// UI route claims go to `spa`.
pub fn service<S, ResBody>(deps: WebDeps, spa: S) -> TowerService
where
    S: tower::Service<WebRequest, Response = http::Response<ResBody>>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
    S::Future: Send,
    ResBody: http_body::Body<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let mut builder = Router::builder().discover();
    if let Some(bundle) = assets::load() {
        builder = builder.assets(bundle);
    }
    let router: Router = builder
        .app_context(Arc::new(deps))
        .cookies()
        .runtime()
        // axum owns response compression.
        .compression(Compression::off())
        // Everything else is the Vue app. `/{*rest}` needs a segment, so the
        // root is registered on its own.
        .route(TowerRoute::any("/", spa.clone()))
        .route(TowerRoute::any("/{*rest}", spa))
        .build();
    TowerService::new(router)
}

/// Tell `topcoat dev` (if it launched us) that the server is listening, so
/// open pages hot-reload. A no-op unless `TOPCOAT_DEV_URL` is set.
pub async fn notify_dev_ready(addr: std::net::SocketAddr) {
    topcoat::dev::notify_ready(Some(addr)).await;
}

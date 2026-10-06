//! What every route module is built from.
//!
//! [`PlatformContext`] is built once per process (by fc-server, fc-dev, the
//! tests) from the repositories, the auth services, the unit of work and
//! the binary's [`PlatformRoutesConfig`]. It also holds the services more
//! than one module's state must *share* — one instance, not one per module:
//! a cache (application access, secret resolver, JWKS, outbound
//! credentials), a rate-limit bucket (`/auth`, `/oauth`), or a service whose
//! identity matters (two-factor login, the reset emailer, the portal plane).
//!
//! Each route module exposes `pub fn routes(ctx: &PlatformContext) ->
//! AggregateRoutes`, builds its own `*State` from the context, and returns
//! its routes at their full paths; `router::build` merges them.

use fc_platform_core::shared::id::ApplicationId;
use std::sync::Arc;

use axum::Router;
use tracing::warn;
use utoipa_axum::router::OpenApiRouter;

use crate::auth::jwks_cache::JwksCache;
use crate::auth::login_backoff::BackoffPolicy;
use crate::auth::password_reset_api::PasswordResetEmailer;
use crate::auth::session_cookie::SessionCookieConfig;
use crate::dispatch_job::signing_guard::SigningGuard;
use crate::mfa::notify::Notifier;
use crate::mfa::notify::PlatformName;
use crate::mfa::MfaRepository;
use crate::mfa::MfaService;
use crate::mfa::MfaTokenIssuer;
use crate::mfa::TwoFactorLogin;
use crate::mfa::TwoFactorPolicy;
use crate::portal::PortalDeps;
use crate::portal::PortalState;
use crate::repository::Repositories;
use crate::service_account::outbound_credentials::OutboundCredentialsResolver;
use crate::shared::authorization_service::ApplicationAccessService;
use crate::shared::email_service;
use crate::shared::email_service::EmailService;
use crate::shared::encryption_service::EncryptionService;
use crate::shared::rate_limit_middleware::{IpRateLimiterState, RateLimitConfig};
use crate::shared::rate_limit_store::RateLimitPolicies;
use crate::shared::rate_limit_store::RateLimitStore;
use crate::shared::secret_ref::SecretResolver;
use crate::shared::server_setup::AuthServices;
use crate::usecase::PgUnitOfWork;

/// Per-binary configuration for the points where binaries diverge.
pub struct PlatformRoutesConfig {
    /// Distributed rate-limit store. Built by the binary (async) so the
    /// Redis-or-Postgres choice happens once at startup and is logged.
    /// Use `NoopRateLimitStore` in tests.
    pub rate_limit_store: Arc<dyn RateLimitStore>,
    /// Per-bucket policies, loaded from env via `RateLimitPolicies::from_env`.
    pub rate_limit_policies: Arc<RateLimitPolicies>,
    /// `Secure` flag for the OIDC session cookie. `true` in production.
    pub session_cookie_secure: bool,
    /// `SameSite` policy for the session cookie (`Lax`, `Strict`, or `None`).
    /// Defaults to `Lax`.
    pub session_cookie_same_site: String,
    /// Session token expiry in seconds. Defaults to 86400 (24h).
    pub session_token_expiry_secs: i64,
    /// Optional static asset directory for SPA serving.
    pub static_dir: Option<String>,
    /// External base URL for the OIDC login flow (used for absolute redirect
    /// URLs). Binary pre-resolves from env (usually `FC_EXTERNAL_BASE_URL`).
    pub oidc_login_external_base_url: Option<String>,
    /// External base URL for the `.well-known` endpoints (issuer, JWKS).
    pub well_known_external_base_url: String,
    /// External base URL for password-reset email links.
    pub password_reset_external_base_url: String,
}

impl PlatformRoutesConfig {
    /// Default `SameSite` policy when not configured.
    pub const DEFAULT_SAME_SITE: &'static str = "Lax";
    /// Default session token expiry (24 hours) when not configured.
    pub const DEFAULT_SESSION_EXPIRY_SECS: i64 = 86400;
}

/// The dependencies every route module builds its state from.
pub struct PlatformContext {
    pub repos: Repositories,
    pub auth: AuthServices,
    pub unit_of_work: Arc<PgUnitOfWork>,
    pub config: PlatformRoutesConfig,
    /// The seeded `code='platform'` application row.
    pub platform_application_id: ApplicationId,

    /// `FLOWCATALYST_APP_KEY`'s encryption service (`None` when unset).
    pub encryption: Option<Arc<EncryptionService>>,
    /// Opens stored secrets wherever they are used: `encrypted:` values and
    /// secret-manager references (`aws-sm://…`, `env://…`), one cache.
    pub secret_resolver: Arc<SecretResolver>,
    pub email_service: Arc<dyn EmailService>,
    /// Password reset emailer — shared between user-initiated
    /// `/auth/password-reset/request` and the admin-initiated
    /// `/api/principals/{id}/send-password-reset` and create-user paths.
    pub password_reset_emailer: Arc<PasswordResetEmailer>,
    /// One instance so every `/{appCode}` route shares the scope cache, and
    /// the application-access endpoint can drop a principal's entry when it
    /// changes.
    pub app_access: Arc<ApplicationAccessService>,
    /// The one ingest signing guard: dispatch-job (S5) and event (S6)
    /// ingest on every route.
    pub signing_guard: Arc<SigningGuard>,
    /// Every handler that sets or clears the OIDC session cookie shares it.
    pub session_cookie: SessionCookieConfig,
    pub backoff_policy: Arc<BackoffPolicy>,
    /// Two-factor authentication (Go's mfa + twofa + mfatoken + notify).
    pub two_factor: Arc<TwoFactorLogin>,
    /// The portal identity plane (Go wire_routes.go: portalusersapi.State,
    /// portalauth.State, bridge.PortalBridge and the token endpoint's portal
    /// repos).
    pub portal: PortalState,
    /// The OIDC login flow's key cache, shared by `/auth/oidc/*` and the
    /// portal's OIDC login.
    pub jwks_cache: Arc<JwksCache>,
    /// One outbound-credentials resolver for every delivery the platform
    /// signs (Java OutboundCredentials, one-minute cache per application).
    pub outbound_credentials: Arc<OutboundCredentialsResolver>,
    /// Per-IP limiter shared by every `/auth` route group, so a high-volume
    /// OAuth client doesn't starve the login flow (and vice versa). Composes
    /// with the per-account backoff in `auth::login_backoff`.
    pub auth_ip_limit: IpRateLimiterState,
    /// Per-IP limiter for `/oauth/*`.
    pub oauth_ip_limit: IpRateLimiterState,
}

impl PlatformContext {
    pub fn new(
        repos: &Repositories,
        auth: &AuthServices,
        unit_of_work: &Arc<PgUnitOfWork>,
        config: PlatformRoutesConfig,
        platform_application_id: ApplicationId,
    ) -> Self {
        let email_service: Arc<dyn EmailService> = Arc::from(email_service::create_email_service());
        let password_reset_emailer = Arc::new(PasswordResetEmailer {
            password_reset_repo: repos.password_reset_repo.clone(),
            email_service: email_service.clone(),
            unit_of_work: unit_of_work.clone(),
            external_base_url: config.password_reset_external_base_url.clone(),
            brand: Some(repos.platform_config_repo.clone()),
        });
        let session_cookie = SessionCookieConfig {
            name: "fc_session".to_string(),
            secure: config.session_cookie_secure,
            same_site: SessionCookieConfig::parse_same_site(&config.session_cookie_same_site),
            ttl: time::Duration::seconds(config.session_token_expiry_secs),
        };
        let encryption = EncryptionService::from_env().map(Arc::new);
        if encryption.is_none() {
            warn!("FLOWCATALYST_APP_KEY not set — stored secrets can be neither written nor read");
        }
        let secret_resolver = Arc::new(SecretResolver::platform(encryption.clone()));
        let backoff_policy = Arc::new(BackoffPolicy::from_env());
        let platform_name = PlatformName {
            configs: Some(repos.platform_config_repo.clone()),
        };
        let two_factor = Arc::new(TwoFactorLogin {
            mfa: Arc::new(MfaService {
                repo: Arc::new(MfaRepository::new(&repos.pool)),
                encryption: encryption.clone(),
                email: email_service.clone(),
                issuer: platform_name.clone(),
            }),
            tokens: Arc::new(MfaTokenIssuer::new(&auth.auth, auth.auth.issuer())),
            policy: TwoFactorPolicy {
                mappings: repos.edm_repo.clone(),
                identity_providers: repos.idp_repo.clone(),
            },
            notifier: Notifier {
                email: email_service.clone(),
                name: platform_name,
            },
            auth_service: auth.auth.clone(),
            principal_repo: repos.principal_repo.clone(),
            role_repo: repos.role_repo.clone(),
            login_attempt_repo: repos.login_attempt_repo.clone(),
            audit_log_repo: repos.audit_log_repo.clone(),
            backoff_policy: backoff_policy.clone(),
            session_cookie: SessionCookieConfig::password_login(config.session_cookie_secure),
            rate_limit_store: config.rate_limit_store.clone(),
            rate_limit_policies: config.rate_limit_policies.clone(),
        });
        let portal = PortalState::new(PortalDeps {
            pool: repos.pool.clone(),
            clients: repos.client_repo.clone(),
            oauth_clients: repos.oauth_client_repo.clone(),
            identity_providers: repos.idp_repo.clone(),
            auth_codes: repos.auth_code_repo.clone(),
            password_service: auth.password.clone(),
            unit_of_work: unit_of_work.clone(),
            email_service: email_service.clone(),
            encryption_service: encryption.clone(),
            rate_limit_store: config.rate_limit_store.clone(),
            external_base_url: config.password_reset_external_base_url.clone(),
        });
        let outbound_credentials = Arc::new(
            OutboundCredentialsResolver::new(
                repos.service_account_repo.clone(),
                encryption.clone(),
            )
            .with_secret_resolver(secret_resolver.clone()),
        );
        Self {
            signing_guard: Arc::new(SigningGuard::new(
                repos.subscription_repo.clone(),
                repos.connection_repo.clone(),
                repos.service_account_repo.clone(),
                repos.application_repo.clone(),
                repos.principal_repo.clone(),
            )),
            app_access: Arc::new(ApplicationAccessService::new(
                repos.principal_repo.clone(),
                repos.application_repo.clone(),
            )),
            jwks_cache: Arc::new(JwksCache::default()),
            auth_ip_limit: IpRateLimiterState::new(&RateLimitConfig::auth_default_from_env()),
            oauth_ip_limit: IpRateLimiterState::new(
                &RateLimitConfig::oauth_token_default_from_env(),
            ),
            repos: repos.clone(),
            auth: auth.clone(),
            unit_of_work: unit_of_work.clone(),
            config,
            platform_application_id,
            encryption,
            secret_resolver,
            email_service,
            password_reset_emailer,
            session_cookie,
            backoff_policy,
            two_factor,
            portal,
            outbound_credentials,
        }
    }
}

/// One route module's routes, at their full paths: the ones documented in
/// the OpenAPI document and the plain ones (routed, not documented).
pub struct AggregateRoutes {
    pub documented: OpenApiRouter,
    pub plain: Router,
}

impl Default for AggregateRoutes {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateRoutes {
    pub fn new() -> Self {
        Self {
            documented: OpenApiRouter::new(),
            plain: Router::new(),
        }
    }

    /// Another module's routes after these, documented and plain alike.
    pub fn merge(self, other: AggregateRoutes) -> Self {
        Self {
            documented: self.documented.merge(other.documented),
            plain: self.plain.merge(other.plain),
        }
    }
}

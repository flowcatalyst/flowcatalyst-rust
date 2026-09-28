//! Portal identity plane — Go's `internal/platform/portalidentity`,
//! `internal/platform/portalauth` and the portal hooks of the OIDC bridge,
//! the reset-token flows and the token endpoint.
//!
//! Portal end-users are a SEPARATE identity population from `iam_principals`:
//! one identity per (client, email) context, holding its own password (or
//! signing in through the IdP that owns its email domain), granted per portal
//! app. The plane has:
//!
//! - an admin surface, `/api/portal-users` and `/api/portal-apps`
//!   ([`api`]), gated by the client-delegable portal permissions
//!   ([`can_read_portal_users`], [`can_write_portal_users`]);
//! - a login surface, `/portal/*` ([`login_api`]), independent of the
//!   employee one: it never reads or writes `fc_session`, and issues
//!   authorization codes whose subject is the `ptu_…` identity id;
//! - the portal branches of shared endpoints: the reset-token confirm and
//!   validate ([`password`]), `/oauth/token` ([`token`]) and the OIDC
//!   callback ([`oidc`]).

pub use fc_platform_iam::portal::*;

pub mod api;
mod email;
pub mod login_api;
pub mod oidc;
pub mod operations;
pub mod password;
pub mod token;

use std::sync::Arc;

use crate::auth::authorization_code_repository::AuthorizationCodeRepository;
use fc_platform_core::shared::email_service::EmailService;
use fc_platform_core::shared::encryption_service::EncryptionService;
use fc_platform_core::shared::error::Result;
use fc_platform_core::shared::rate_limit_store::{RateLimitPolicy, RateLimitStore};
use fc_platform_core::usecase::PgUnitOfWork;
use fc_platform_iam::auth::password_service::PasswordService;
use fc_platform_iam::{
    auth::oauth_client_repository::OAuthClientRepository, client::repository::ClientRepository,
    identity_provider::repository::IdentityProviderRepository,
};
use repository::{
    PortalAppRepository, PortalFlowRepository, PortalIdentityRepository, PortalOAuthClientReader,
    PortalOidcStateRepository, PortalResetTokenRepository,
};

/// Everything the portal plane's handlers and hooks need.
#[derive(Clone)]
pub struct PortalState {
    pub identities: Arc<PortalIdentityRepository>,
    pub apps: Arc<PortalAppRepository>,
    pub portal_oauth: Arc<PortalOAuthClientReader>,
    pub flows: Arc<PortalFlowRepository>,
    pub oidc_states: Arc<PortalOidcStateRepository>,
    pub passwords: Arc<password::PortalPasswords>,
    pub clients: Arc<ClientRepository>,
    pub oauth_clients: Arc<OAuthClientRepository>,
    pub identity_providers: Arc<IdentityProviderRepository>,
    pub auth_codes: Arc<AuthorizationCodeRepository>,
    pub password_service: Arc<PasswordService>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    /// Hashes generated OAuth client secrets. `None` when
    /// `FLOWCATALYST_APP_KEY` is unset: a CONFIDENTIAL portal app then
    /// cannot be created.
    pub encryption_service: Option<Arc<EncryptionService>>,
    /// The per-(client, email) brute-force ceiling of the password login and
    /// the reset request (Go `ratelimit.BucketPortalLogin`).
    pub rate_limit_store: Arc<dyn RateLimitStore>,
    pub portal_login_policy: RateLimitPolicy,
    /// The SPA route `/portal/authorize` bounces to (Go default
    /// `/portal/login`).
    pub login_page_path: String,
}

/// Construction inputs for [`PortalState`] that are not repositories built
/// from the pool.
pub struct PortalDeps {
    pub pool: sqlx::PgPool,
    pub clients: Arc<ClientRepository>,
    pub oauth_clients: Arc<OAuthClientRepository>,
    pub identity_providers: Arc<IdentityProviderRepository>,
    pub auth_codes: Arc<AuthorizationCodeRepository>,
    pub password_service: Arc<PasswordService>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    pub email_service: Arc<dyn EmailService>,
    pub encryption_service: Option<Arc<EncryptionService>>,
    pub rate_limit_store: Arc<dyn RateLimitStore>,
    /// Base for invite/reset links.
    pub external_base_url: String,
}

impl PortalState {
    pub fn new(deps: PortalDeps) -> Self {
        let identities = Arc::new(PortalIdentityRepository::new(&deps.pool));
        let passwords = Arc::new(password::PortalPasswords {
            tokens: Arc::new(PortalResetTokenRepository::new(&deps.pool)),
            identities: identities.clone(),
            email_service: deps.email_service,
            brand: Some(Arc::new(
                fc_platform_iam::platform_config::repository::PlatformConfigRepository::new(
                    &deps.pool,
                ),
            )),
            password_service: deps.password_service.clone(),
            external_base_url: deps.external_base_url,
        });
        Self {
            identities,
            apps: Arc::new(PortalAppRepository::new(&deps.pool)),
            portal_oauth: Arc::new(PortalOAuthClientReader::new(&deps.pool)),
            flows: Arc::new(PortalFlowRepository::new(&deps.pool)),
            oidc_states: Arc::new(PortalOidcStateRepository::new(&deps.pool)),
            passwords,
            clients: deps.clients,
            oauth_clients: deps.oauth_clients,
            identity_providers: deps.identity_providers,
            auth_codes: deps.auth_codes,
            password_service: deps.password_service,
            unit_of_work: deps.unit_of_work,
            encryption_service: deps.encryption_service,
            rate_limit_store: deps.rate_limit_store,
            portal_login_policy: portal_login_policy_from_env(),
            login_page_path: "/portal/login".to_string(),
        }
    }

    /// The OIDC IdP that owns the email domain, if any (Go
    /// `identityprovider.OIDCProviderForDomain`): domain ownership alone
    /// decides SSO vs password, on every login surface.
    pub async fn oidc_provider_for_domain(
        &self,
        domain: &str,
    ) -> Result<Option<fc_platform_iam::identity_provider::entity::IdentityProvider>> {
        if domain.is_empty() {
            return Ok(None);
        }
        let idps = self.identity_providers.find_all().await?;
        Ok(idps.into_iter().find(|idp| {
            idp.r#type == fc_platform_iam::identity_provider::entity::IdentityProviderType::Oidc
                && idp
                    .allowed_email_domains
                    .iter()
                    .any(|d| d.eq_ignore_ascii_case(domain))
        }))
    }
}

/// Go `ratelimit.Policies.PortalLogin`: `FC_RL_PORTAL_LOGIN_PER_15MIN`
/// (default 10) per (client, email) per 15 minutes.
pub fn portal_login_policy_from_env() -> RateLimitPolicy {
    let limit = std::env::var("FC_RL_PORTAL_LOGIN_PER_15MIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    RateLimitPolicy::new(std::time::Duration::from_secs(15 * 60), limit)
}

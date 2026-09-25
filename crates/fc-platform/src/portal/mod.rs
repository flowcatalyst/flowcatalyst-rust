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

pub mod api;
mod email;
pub mod entity;
pub mod login_api;
pub mod oidc;
pub mod operations;
pub mod password;
pub mod policy;
pub mod repository;
pub mod token;

use std::sync::Arc;

pub use entity::{is_portal_subject, trimmed_or_none, PortalApp, PortalIdentity};

use crate::auth::authorization_code_repository::AuthorizationCodeRepository;
use crate::auth::password_service::PasswordService;
use crate::role::entity::permissions;
use crate::shared::authorization_service::AuthContext;
use crate::shared::email_service::EmailService;
use crate::shared::encryption_service::EncryptionService;
use crate::shared::error::{PlatformError, Result};
use crate::shared::rate_limit_store::{RateLimitPolicy, RateLimitStore};
use crate::usecase::PgUnitOfWork;
use crate::{ClientRepository, IdentityProviderRepository, OAuthClientRepository};
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
    ) -> Result<Option<crate::IdentityProvider>> {
        if domain.is_empty() {
            return Ok(None);
        }
        let idps = self.identity_providers.find_all().await?;
        Ok(idps.into_iter().find(|idp| {
            idp.r#type == crate::identity_provider::entity::IdentityProviderType::Oidc
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

// ── The portal flags on OAuth clients (Go auth/api + auth/operations) ────

/// Go `validatePlaneFlags`: a portal app can only be linked to a portal
/// client. (Rust has no `apiAccess` flag, so Go's portal + apiAccess
/// conflict cannot arise.)
pub fn validate_oauth_client_plane(
    client: &crate::OAuthClient,
) -> std::result::Result<(), crate::usecase::UseCaseError> {
    let is_portal = client
        .portal_client_id
        .as_deref()
        .is_some_and(|p| !p.is_empty());
    if client
        .portal_app_id
        .as_deref()
        .is_some_and(|a| !a.is_empty())
        && !is_portal
    {
        return Err(crate::usecase::UseCaseError::validation(
            "PORTAL_APP_REQUIRES_PORTAL_CLIENT",
            "a portal app can only be linked to a portal client (portalClientId)",
        ));
    }
    Ok(())
}

/// Go `authapi.resolvePortalApp`: a linked portal app is authoritative for
/// the portal owner — `portalAppId` (when non-empty) must name an existing
/// app, and `portalClientId` becomes that app's client; a conflicting
/// explicit `portalClientId` is refused.
pub async fn resolve_oauth_client_portal_app(
    apps: &PortalAppRepository,
    portal_app_id: Option<&str>,
    portal_client_id: &mut Option<String>,
) -> Result<()> {
    let Some(app_id) = portal_app_id.map(str::trim).filter(|a| !a.is_empty()) else {
        return Ok(());
    };
    let app = apps
        .find_by_id(app_id)
        .await?
        .ok_or_else(|| PlatformError::from(operations::not_found("PortalApp", app_id)))?;
    if portal_client_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|pc| !pc.is_empty() && pc != app.client_id)
    {
        return Err(PlatformError::from(
            crate::usecase::UseCaseError::validation(
                "PORTAL_APP_CLIENT_MISMATCH",
                "portalAppId belongs to a different client than portalClientId",
            ),
        ));
    }
    *portal_client_id = Some(app.client_id);
    Ok(())
}

// ── Permission checks (Go shared/auth CanReadPortalUsers /
// CanManagePortalUsers) ─────────────────────────────────────────────────────
//
// The plane is CLIENT-delegable: anchors pass everywhere; a client-scoped
// caller (a client administrator in the platform UI, or the portal
// application's confined service account) needs access to the target client
// AND the portal-user permission, so a client manages its own portal
// population and nobody else's.

fn scope_forbidden() -> PlatformError {
    PlatformError::forbidden_code("SCOPE_FORBIDDEN", "no access to this client")
}

/// Listing a client's portal identities (and its portal apps).
pub fn can_read_portal_users(ctx: &AuthContext, client_id: &str) -> Result<()> {
    if ctx.is_anchor() {
        return Ok(());
    }
    if !ctx.can_access_client(client_id) {
        return Err(scope_forbidden());
    }
    let any = [
        permissions::iam::PORTAL_USER_READ,
        permissions::iam::PORTAL_USER_MANAGE,
    ];
    if ctx.has_any_permission(&any) {
        Ok(())
    } else {
        Err(PlatformError::forbidden_code(
            "PERMISSION_REQUIRED",
            format!("one of: {}", any.join(", ")),
        ))
    }
}

/// Ensure/invite, suspension, deletion and app administration of a client's
/// portal identities (Go `CanManagePortalUsers`).
pub fn can_write_portal_users(ctx: &AuthContext, client_id: &str) -> Result<()> {
    if ctx.is_anchor() {
        return Ok(());
    }
    if !ctx.can_access_client(client_id) {
        return Err(scope_forbidden());
    }
    if ctx.has_permission(permissions::iam::PORTAL_USER_MANAGE) {
        Ok(())
    } else {
        Err(PlatformError::forbidden_code(
            "PERMISSION_REQUIRED",
            format!(
                "permission required: {}",
                permissions::iam::PORTAL_USER_MANAGE
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::entity::UserScope;
    use crate::shared::authorization_service::Credential;
    use std::collections::HashSet;

    fn ctx(scope: UserScope, clients: &[&str], perms: &[&str]) -> AuthContext {
        AuthContext {
            principal_id: "prn_1".into(),
            principal_type: crate::principal::entity::PrincipalType::User,
            scope,
            email: None,
            name: "x".into(),
            accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
            permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
            roles: vec![],
            credential: Credential::BearerToken,
        }
    }

    #[test]
    fn anchors_pass_without_a_permission() {
        let a = ctx(UserScope::Anchor, &["*"], &[]);
        assert!(can_read_portal_users(&a, "clt_1").is_ok());
        assert!(can_write_portal_users(&a, "clt_1").is_ok());
    }

    #[test]
    fn client_callers_need_reach_and_the_permission() {
        let manage = permissions::iam::PORTAL_USER_MANAGE;
        let view = permissions::iam::PORTAL_USER_READ;
        let other = ctx(UserScope::Client, &["clt_2"], &[manage]);
        assert!(can_write_portal_users(&other, "clt_1").is_err());
        let viewer = ctx(UserScope::Client, &["clt_1"], &[view]);
        assert!(can_read_portal_users(&viewer, "clt_1").is_ok());
        assert!(can_write_portal_users(&viewer, "clt_1").is_err());
        let manager = ctx(UserScope::Client, &["clt_1"], &[manage]);
        assert!(can_read_portal_users(&manager, "clt_1").is_ok());
        assert!(can_write_portal_users(&manager, "clt_1").is_ok());
    }
}

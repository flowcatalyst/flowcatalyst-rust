//! Router builder — assembles middleware + OIDC handlers into a `Router`
//! the consumer app can `.merge()` or `.nest()` into theirs.

use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

use crate::auth::{oauth::OAuthClient, TokenValidator};

use super::crypto::SessionCrypto;
use super::oidc::{callback_handler, login_handler, logout_handler, OidcState};
use super::rbac::RbacCatalogue;
use super::session::{CookieAttrs, CookieSessionStore, SharedSessionStore};
use super::state::{AuthRoutes, AuthState};
use crate::auth::AuthError;

/// Build options for [`auth_router`]. Mirrors `FlowcatalystAuthOptions` from
/// the TS SDK.
pub struct FlowcatalystAuthBuilder {
    token_validator: Arc<TokenValidator>,
    oauth_client: Arc<OAuthClient>,
    session_secret: Vec<String>,
    cookie_attrs: CookieAttrs,
    session_store: Option<SharedSessionStore>,
    rbac: Option<RbacCatalogue>,
    routes: AuthRoutes,
    session_max_age_ms: i64,
}

impl FlowcatalystAuthBuilder {
    /// Start a new builder. `token_validator` validates Bearer tokens AND
    /// access tokens received during the OIDC callback. `oauth_client` is
    /// used for the authorization-code exchange + refresh-token grant.
    pub fn new(token_validator: Arc<TokenValidator>, oauth_client: Arc<OAuthClient>) -> Self {
        Self {
            token_validator,
            oauth_client,
            session_secret: Vec::new(),
            cookie_attrs: CookieAttrs::default(),
            session_store: None,
            rbac: None,
            routes: AuthRoutes::default(),
            session_max_age_ms: 1000 * 60 * 60 * 8,
        }
    }

    /// One or more 32-byte secrets (base64url, base64, or hex). The first
    /// encrypts; any can decrypt. Required unless `session_store` is set.
    pub fn cookie_secret<I, S>(mut self, secrets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.session_secret = secrets.into_iter().map(Into::into).collect();
        self
    }

    pub fn cookie_attrs(mut self, attrs: CookieAttrs) -> Self {
        self.cookie_attrs = attrs;
        self
    }

    /// Provide a custom session backend. If unset, an AES-GCM cookie store
    /// is built from `cookie_secret`.
    pub fn session_store(mut self, store: SharedSessionStore) -> Self {
        self.session_store = Some(store);
        self
    }

    pub fn rbac(mut self, rbac: RbacCatalogue) -> Self {
        self.rbac = Some(rbac);
        self
    }

    pub fn routes(mut self, routes: AuthRoutes) -> Self {
        self.routes = routes;
        self
    }

    /// Max lifetime of a session in milliseconds. Default 8h.
    pub fn session_max_age_ms(mut self, ms: i64) -> Self {
        self.session_max_age_ms = ms;
        self
    }

    /// Finish building. Returns `(AuthState, Router)`:
    ///   - `AuthState` is what the auth middleware needs as state.
    ///   - `Router` exposes `/auth/login`, `/auth/callback`, `/auth/logout`.
    ///
    /// The caller merges the router into theirs and applies the middleware
    /// globally:
    /// ```ignore
    /// let (state, auth_router) = FlowcatalystAuthBuilder::new(tv, oc)
    ///     .cookie_secret([secret])
    ///     .rbac(rbac)
    ///     .build()?;
    ///
    /// let app = Router::new()
    ///     .merge(auth_router)
    ///     .route("/api/me", get(me_handler))
    ///     .layer(axum::middleware::from_fn_with_state(
    ///         state.clone(),
    ///         fc_sdk::auth::axum::fc_auth_middleware,
    ///     ))
    ///     .layer(axum::Extension(state));
    /// ```
    pub fn build(self) -> Result<(AuthState, Router), AuthError> {
        let session_store: SharedSessionStore = if let Some(s) = self.session_store {
            s
        } else {
            if self.session_secret.is_empty() {
                return Err(AuthError::Config(
                    "either `cookie_secret(...)` or `session_store(...)` is required".into(),
                ));
            }
            let crypto = SessionCrypto::new(self.session_secret.iter())?;
            Arc::new(CookieSessionStore::new(crypto, self.cookie_attrs.clone()))
        };

        let state_secret = if self.session_secret.is_empty() {
            // No session secret was supplied (custom store). Derive a runtime
            // secret for the short-lived state cookie — it only lives 10
            // minutes and is single-use, so a per-process key is fine.
            vec![super::crypto::generate_session_secret()]
        } else {
            self.session_secret.clone()
        };
        let state_crypto = Arc::new(SessionCrypto::new(state_secret.iter())?);

        let auth = AuthState {
            token_validator: self.token_validator,
            oauth_client: self.oauth_client,
            session_store,
            rbac: self.rbac,
            routes: self.routes.clone(),
            session_max_age_ms: self.session_max_age_ms,
        };
        let oidc_state = OidcState {
            auth: auth.clone(),
            state_crypto,
        };

        let router = Router::new()
            .route(&self.routes.login, get(login_handler))
            .route(&self.routes.callback, get(callback_handler))
            .route(&self.routes.logout, post(logout_handler))
            .with_state(oidc_state);

        Ok((auth, router))
    }
}

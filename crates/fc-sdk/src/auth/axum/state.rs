//! Shared state held by the Axum auth integration. Stored in the router's
//! Extensions so handlers + middleware can reach it without threading it
//! through every layer.

use std::sync::Arc;

use crate::auth::oauth::OAuthClient;
use crate::auth::TokenValidator;

use super::rbac::RbacCatalogue;
use super::session::SharedSessionStore;

/// Where on the host the plugin mounts its OIDC routes.
#[derive(Debug, Clone)]
pub struct AuthRoutes {
    pub login: String,
    pub callback: String,
    pub logout: String,
    pub return_to_param: String,
}

impl Default for AuthRoutes {
    fn default() -> Self {
        Self {
            login: "/auth/login".into(),
            callback: "/auth/callback".into(),
            logout: "/auth/logout".into(),
            return_to_param: "returnTo".into(),
        }
    }
}

/// Shared per-app auth configuration. Held by Axum middleware + handlers
/// via [`axum::Extension`].
#[derive(Clone)]
pub struct AuthState {
    pub token_validator: Arc<TokenValidator>,
    pub oauth_client: Arc<OAuthClient>,
    pub session_store: SharedSessionStore,
    pub rbac: Option<RbacCatalogue>,
    pub routes: AuthRoutes,
    pub session_max_age_ms: i64,
}

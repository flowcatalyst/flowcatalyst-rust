//! Shared auth services initialization.
//!
//! Constructs `AuthService`, `AuthorizationService`, `PasswordService`,
//! `OidcSyncService`, and `OidcService` from configuration — the same
//! set of services that all three server binaries build.

use std::sync::Arc;

use crate::repository::Repositories;
use crate::service::{
    AuthConfig, AuthService, AuthorizationService, OidcService, OidcSyncService, PasswordService,
};

/// Bundle of auth-related services every binary needs.
///
/// All fields are `Arc`-wrapped so they can be cheaply cloned into
/// handler state structs.
#[derive(Clone)]
pub struct AuthServices {
    pub auth: Arc<AuthService>,
    pub authz: Arc<AuthorizationService>,
    pub password: Arc<PasswordService>,
    pub oidc_sync: Arc<OidcSyncService>,
    pub oidc: Arc<OidcService>,
}

/// Configuration needed to build the auth services.
///
/// Binaries resolve these from env vars (with whatever alias/default
/// handling they need) and pass them in.
pub struct AuthInitConfig {
    /// JWT issuer and audience (external base URL per OIDC spec).
    pub issuer: String,
    /// Optional on-disk RSA private key path. When both paths are
    /// `None`, keys are generated in-memory (dev mode).
    pub private_key_path: Option<String>,
    /// Optional on-disk RSA public key path.
    pub public_key_path: Option<String>,
    /// Optional previous public key PEM (for JWT key rotation).
    pub previous_public_key: Option<String>,
    pub access_token_expiry_secs: i64,
    pub session_token_expiry_secs: i64,
    pub refresh_token_expiry_secs: i64,
}

impl AuthInitConfig {
    /// Load an `AuthInitConfig` from environment variables.
    ///
    /// Reads the keys every binary already reads:
    /// - `FC_JWT_ISSUER` / `FC_EXTERNAL_BASE_URL` / `EXTERNAL_BASE_URL`
    ///   (falling back to `default_issuer` if none set)
    /// - `FC_JWT_PRIVATE_KEY_PATH` / `FC_JWT_PUBLIC_KEY_PATH`
    /// - `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS` (path whose contents become
    ///   the previous public key) or `FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS`
    /// - `FC_ACCESS_TOKEN_EXPIRY_SECS` / `OIDC_ACCESS_TOKEN_TTL` (default 3600)
    /// - `FC_SESSION_TOKEN_EXPIRY_SECS` / `OIDC_SESSION_TTL` (default 86400)
    /// - `FC_REFRESH_TOKEN_EXPIRY_SECS` / `OIDC_REFRESH_TOKEN_TTL` (default 30 days)
    pub fn from_env(default_issuer: &str) -> Self {
        let issuer = std::env::var("FC_JWT_ISSUER")
            .or_else(|_| std::env::var("FC_EXTERNAL_BASE_URL"))
            .or_else(|_| std::env::var("EXTERNAL_BASE_URL"))
            .unwrap_or_else(|_| default_issuer.to_string());

        let private_key_path = std::env::var("FC_JWT_PRIVATE_KEY_PATH").ok();
        let public_key_path = std::env::var("FC_JWT_PUBLIC_KEY_PATH").ok();
        let previous_public_key = std::env::var("FC_JWT_PUBLIC_KEY_PATH_PREVIOUS")
            .ok()
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .or_else(|| std::env::var("FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS").ok());

        let access_token_expiry_secs = env_or_alias_i64(
            "FC_ACCESS_TOKEN_EXPIRY_SECS",
            "OIDC_ACCESS_TOKEN_TTL",
            3600,
        );
        let session_token_expiry_secs = env_or_alias_i64(
            "FC_SESSION_TOKEN_EXPIRY_SECS",
            "OIDC_SESSION_TTL",
            86400,
        );
        let refresh_token_expiry_secs = env_or_alias_i64(
            "FC_REFRESH_TOKEN_EXPIRY_SECS",
            "OIDC_REFRESH_TOKEN_TTL",
            86400 * 30,
        );

        Self {
            issuer,
            private_key_path,
            public_key_path,
            previous_public_key,
            access_token_expiry_secs,
            session_token_expiry_secs,
            refresh_token_expiry_secs,
        }
    }
}

/// Build the full auth service bundle from configuration + repos.
///
/// Loads or generates RSA keys, then wires up the `AuthService`,
/// `AuthorizationService`, `PasswordService`, `OidcSyncService`,
/// and `OidcService`.
pub fn init_auth_services(
    repos: &Repositories,
    config: AuthInitConfig,
) -> anyhow::Result<AuthServices> {
    let (private_key, public_key) = AuthConfig::load_or_generate_rsa_keys(
        config.private_key_path.as_deref(),
        config.public_key_path.as_deref(),
    )?;

    let auth_config = AuthConfig {
        rsa_private_key: Some(private_key),
        rsa_public_key: Some(public_key),
        rsa_public_key_previous: config.previous_public_key,
        secret_key: String::new(),
        audience: config.issuer.clone(),
        issuer: config.issuer,
        access_token_expiry_secs: config.access_token_expiry_secs,
        session_token_expiry_secs: config.session_token_expiry_secs,
        refresh_token_expiry_secs: config.refresh_token_expiry_secs,
    };

    let auth = Arc::new(AuthService::new(auth_config));
    let authz = Arc::new(AuthorizationService::new(repos.role_repo.clone()));
    let password = Arc::new(PasswordService::default());
    let oidc_sync = Arc::new(OidcSyncService::new(
        repos.principal_repo.clone(),
        repos.idp_role_mapping_repo.clone(),
    ));
    let oidc = Arc::new(OidcService::new());

    Ok(AuthServices {
        auth,
        authz,
        password,
        oidc_sync,
        oidc,
    })
}

fn env_or_alias_i64(primary: &str, alias: &str, default: i64) -> i64 {
    std::env::var(primary)
        .or_else(|_| std::env::var(alias))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

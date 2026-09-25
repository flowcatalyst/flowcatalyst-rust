//! Shared auth services initialization.
//!
//! Constructs `AuthService`, `AuthorizationService`, `PasswordService`,
//! and `OidcSyncService` from configuration — the same
//! set of services that all three server binaries build.

use std::sync::Arc;

use crate::repository::Repositories;
use crate::service::{
    AuthConfig, AuthService, AuthorizationService, OidcSyncService, PasswordService,
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
    /// Access-token lifetime when none is configured (Go: 1h).
    pub const DEFAULT_ACCESS_TOKEN_TTL_SECS: i64 = 60 * 60;
    /// Session lifetime when none is configured (Go: 24h).
    pub const DEFAULT_SESSION_TTL_SECS: i64 = 24 * 60 * 60;
    /// Refresh-token family cap when none is configured (Go: 7 days).
    pub const DEFAULT_REFRESH_TOKEN_TTL_SECS: i64 = 7 * 24 * 60 * 60;

    /// Load an `AuthInitConfig` from environment variables, with Go's names
    /// (flowcatalyst-go `internal/server/envcfg.go`) and Rust's earlier ones:
    /// - issuer: `FC_JWT_ISSUER` / `FC_EXTERNAL_BASE_URL` / `EXTERNAL_BASE_URL`
    ///   (falling back to `default_issuer` if none set)
    /// - signing key: the file at `FC_JWT_PRIVATE_KEY_PATH` or Go's
    ///   `FC_JWT_SIGNING_KEY_PATH`, else the inline PEM in
    ///   `FLOWCATALYST_JWT_PRIVATE_KEY` / `FC_JWT_SIGNING_KEY_PEM`; the public
    ///   key comes from `FC_JWT_PUBLIC_KEY_PATH` or is derived, as Go derives it
    /// - previous public key (rotation, validation only): the file at
    ///   `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS`, else Go's
    ///   `FLOWCATALYST_JWT_PREVIOUS_PUBLIC_KEY` (or the older
    ///   `FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS`); a non-PEM value is ignored
    /// - access token: `FC_ACCESS_TOKEN_EXPIRY_SECS` / `FC_JWT_ACCESS_TOKEN_TTL_SECS`
    ///   / `OIDC_ACCESS_TOKEN_TTL` (default 1h)
    /// - session: `FC_SESSION_TOKEN_EXPIRY_SECS` / `OIDC_SESSION_TTL` (default 24h)
    /// - refresh-token family cap: `FC_REFRESH_TOKEN_EXPIRY_SECS` /
    ///   `OIDC_REFRESH_TOKEN_TTL` (default 7 days)
    ///
    /// As in Go, an unset, unparseable or non-positive lifetime means the default.
    pub fn from_env(default_issuer: &str) -> Self {
        use fc_common::config::env_first;

        let issuer = env_first(
            &["FC_JWT_ISSUER", "FC_EXTERNAL_BASE_URL", "EXTERNAL_BASE_URL"],
            default_issuer,
        );

        let non_empty = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let private_key_path =
            non_empty("FC_JWT_PRIVATE_KEY_PATH").or_else(|| non_empty("FC_JWT_SIGNING_KEY_PATH"));
        let public_key_path = non_empty("FC_JWT_PUBLIC_KEY_PATH");
        let previous_public_key = non_empty("FC_JWT_PUBLIC_KEY_PATH_PREVIOUS")
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .map(|pem| crate::auth::signing_keys::normalize_pem(&pem))
            .or_else(crate::auth::signing_keys::previous_public_key_from_env);

        Self {
            issuer,
            private_key_path,
            public_key_path,
            previous_public_key,
            access_token_expiry_secs: ttl_from_env(
                &[
                    "FC_ACCESS_TOKEN_EXPIRY_SECS",
                    "FC_JWT_ACCESS_TOKEN_TTL_SECS",
                    "OIDC_ACCESS_TOKEN_TTL",
                ],
                Self::DEFAULT_ACCESS_TOKEN_TTL_SECS,
            ),
            session_token_expiry_secs: ttl_from_env(
                &["FC_SESSION_TOKEN_EXPIRY_SECS", "OIDC_SESSION_TTL"],
                Self::DEFAULT_SESSION_TTL_SECS,
            ),
            refresh_token_expiry_secs: ttl_from_env(
                &["FC_REFRESH_TOKEN_EXPIRY_SECS", "OIDC_REFRESH_TOKEN_TTL"],
                Self::DEFAULT_REFRESH_TOKEN_TTL_SECS,
            ),
        }
    }
}

/// A lifetime in seconds: the first parseable value among `names`, or
/// `default` when there is none or it is not positive (Go `positiveOr`).
fn ttl_from_env(names: &[&str], default: i64) -> i64 {
    let v = fc_common::config::env_first_parse(names, 0i64);
    if v > 0 {
        v
    } else {
        default
    }
}

/// Build the full auth service bundle from configuration + repos.
///
/// Loads or generates RSA keys, then wires up the `AuthService`,
/// `AuthorizationService`, `PasswordService`, and `OidcSyncService`.
pub fn init_auth_services(
    repos: &Repositories,
    config: AuthInitConfig,
) -> anyhow::Result<AuthServices> {
    let (private_key, public_key) = AuthConfig::load_or_generate_rsa_keys(
        config.private_key_path.as_deref(),
        config.public_key_path.as_deref(),
    )?;

    let config_refresh_ttl = config.refresh_token_expiry_secs;
    let auth_config = AuthConfig {
        rsa_private_key: Some(private_key.clone()),
        rsa_public_key: Some(public_key.clone()),
        rsa_public_key_previous: config.previous_public_key,
        secret_key: String::new(),
        audience: config.issuer.clone(),
        issuer: config.issuer,
        access_token_expiry_secs: config.access_token_expiry_secs,
        session_token_expiry_secs: config.session_token_expiry_secs,
        refresh_token_expiry_secs: config.refresh_token_expiry_secs,
    };

    // Fail closed, as Go's `authservice.New` does: a configured RSA key that
    // doesn't load is a boot error, never a silent downgrade to HS256 with an
    // empty secret (which would make every token forgeable).
    let mut auth_service =
        AuthService::new_with_rsa(auth_config.clone(), &private_key, &public_key)
            .map_err(|e| anyhow::anyhow!("JWT signing key: {e}"))?;
    if let Some(previous) = auth_config.rsa_public_key_previous.as_deref() {
        auth_service
            .add_previous_rsa_key(previous)
            .map_err(|e| anyhow::anyhow!("FLOWCATALYST_JWT_PREVIOUS_PUBLIC_KEY: {e}"))?;
    }
    // Go stamps this lifetime on each freshly issued refresh token, and it is
    // the family's absolute cap (rotation carries the first deadline forward).
    crate::auth::refresh_token::set_refresh_token_ttl_secs(config_refresh_ttl);
    let auth = Arc::new(auth_service);
    let authz = Arc::new(
        AuthorizationService::new(repos.role_repo.clone())
            .with_session_principals(repos.principal_repo.clone()),
    );
    let password = Arc::new(PasswordService::default());
    let oidc_sync = Arc::new(OidcSyncService::new(
        repos.principal_repo.clone(),
        repos.idp_role_mapping_repo.clone(),
    ));

    Ok(AuthServices {
        auth,
        authz,
        password,
        oidc_sync,
    })
}

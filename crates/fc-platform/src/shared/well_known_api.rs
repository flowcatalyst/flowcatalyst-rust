//! Well-Known Endpoints
//!
//! Standard .well-known endpoints for OAuth 2.0 / OpenID Connect discovery.
//! - /.well-known/openid-configuration
//! - /.well-known/jwks.json

use axum::{
    routing::get,
    extract::State,
    Json, Router,
};
use utoipa::ToSchema;
use serde::Serialize;
use std::sync::Arc;

use crate::AuthService;

/// OpenID Connect Discovery Document
#[derive(Debug, Serialize, ToSchema)]
pub struct OpenIdConfiguration {
    /// Issuer identifier
    pub issuer: String,

    /// URL of the authorization endpoint
    pub authorization_endpoint: String,

    /// URL of the token endpoint
    pub token_endpoint: String,

    /// URL of the userinfo endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<String>,

    /// URL of the JWKS endpoint
    pub jwks_uri: String,

    /// Supported response types
    pub response_types_supported: Vec<String>,

    /// Supported subject types
    pub subject_types_supported: Vec<String>,

    /// Supported ID token signing algorithms
    pub id_token_signing_alg_values_supported: Vec<String>,

    /// Supported scopes
    pub scopes_supported: Vec<String>,

    /// Supported token endpoint auth methods
    pub token_endpoint_auth_methods_supported: Vec<String>,

    /// Supported grant types
    pub grant_types_supported: Vec<String>,

    /// Supported claims
    pub claims_supported: Vec<String>,

    /// Code challenge methods supported (PKCE)
    pub code_challenge_methods_supported: Vec<String>,
}

/// JSON Web Key Set (JWKS)
#[derive(Debug, Serialize, ToSchema)]
pub struct JwksResponse {
    /// Array of JWK keys
    pub keys: Vec<JwkKey>,
}

/// Individual JSON Web Key
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JwkKey {
    /// Key type (RSA, EC, etc.)
    pub kty: String,

    /// Key use (sig for signature)
    #[serde(rename = "use")]
    pub key_use: String,

    /// Key ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,

    /// Algorithm
    pub alg: String,

    /// RSA modulus (for RSA keys)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,

    /// RSA exponent (for RSA keys)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,
}

/// Well-Known service state
#[derive(Clone)]
pub struct WellKnownState {
    pub auth_service: Arc<AuthService>,
    pub external_base_url: String,
}

/// Get OpenID Connect discovery document
///
/// Standard OpenID Connect discovery endpoint providing metadata
/// about the authorization server.
#[utoipa::path(
    get,
    path = "/openid-configuration",
    tag = "well-known",
    responses(
        (status = 200, description = "OpenID configuration", body = OpenIdConfiguration)
    )
)]
pub async fn get_openid_configuration(
    State(state): State<WellKnownState>,
) -> Json<OpenIdConfiguration> {
    let base_url = &state.external_base_url;

    Json(OpenIdConfiguration {
        issuer: base_url.clone(),
        authorization_endpoint: format!("{}/oauth/authorize", base_url),
        token_endpoint: format!("{}/oauth/token", base_url),
        userinfo_endpoint: Some(format!("{}/oauth/userinfo", base_url)),
        jwks_uri: format!("{}/.well-known/jwks.json", base_url),
        response_types_supported: vec![
            "code".to_string(),
            "token".to_string(),
            "id_token".to_string(),
            "code token".to_string(),
            "code id_token".to_string(),
            "token id_token".to_string(),
            "code token id_token".to_string(),
        ],
        subject_types_supported: vec!["public".to_string()],
        id_token_signing_alg_values_supported: vec!["RS256".to_string()],
        scopes_supported: vec![
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
            "offline_access".to_string(),
        ],
        token_endpoint_auth_methods_supported: vec![
            "client_secret_basic".to_string(),
            "client_secret_post".to_string(),
        ],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
            "client_credentials".to_string(),
        ],
        claims_supported: vec![
            "sub".to_string(),
            "iss".to_string(),
            "aud".to_string(),
            "exp".to_string(),
            "iat".to_string(),
            "name".to_string(),
            "email".to_string(),
            "email_verified".to_string(),
            "clients".to_string(),
            "roles".to_string(),
            "scope".to_string(),
        ],
        code_challenge_methods_supported: vec![
            "S256".to_string(),
            "plain".to_string(),
        ],
    })
}

/// Get JSON Web Key Set (JWKS)
///
/// Returns the public keys used to verify JWT signatures.
/// Clients should cache this response and refresh periodically.
#[utoipa::path(
    get,
    path = "/jwks.json",
    tag = "well-known",
    responses(
        (status = 200, description = "JWKS", body = JwksResponse)
    )
)]
pub async fn get_jwks(
    State(state): State<WellKnownState>,
) -> Json<JwksResponse> {
    // Build JWKS from RSA components if available
    let keys = match (state.auth_service.key_id(), state.auth_service.rsa_components()) {
        (Some(kid), Some(components)) => {
            // RS256 mode - return the public key with full components
            vec![JwkKey {
                kty: "RSA".to_string(),
                key_use: "sig".to_string(),
                kid: Some(kid.to_string()),
                alg: "RS256".to_string(),
                n: Some(components.n.clone()),
                e: Some(components.e.clone()),
            }]
        }
        _ => {
            // HS256 mode - no public keys to expose
            vec![]
        }
    };

    Json(JwksResponse { keys })
}

/// Create the well-known router
pub fn well_known_router(state: WellKnownState) -> Router {
    Router::new()
        .route("/openid-configuration", get(get_openid_configuration))
        .route("/jwks.json", get(get_jwks))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openid_config_serialization() {
        let config = OpenIdConfiguration {
            issuer: "https://example.com".to_string(),
            authorization_endpoint: "https://example.com/oauth/authorize".to_string(),
            token_endpoint: "https://example.com/oauth/token".to_string(),
            userinfo_endpoint: None,
            jwks_uri: "https://example.com/.well-known/jwks.json".to_string(),
            response_types_supported: vec!["code".to_string()],
            subject_types_supported: vec!["public".to_string()],
            id_token_signing_alg_values_supported: vec!["RS256".to_string()],
            scopes_supported: vec!["openid".to_string()],
            token_endpoint_auth_methods_supported: vec!["client_secret_basic".to_string()],
            grant_types_supported: vec!["authorization_code".to_string()],
            claims_supported: vec!["sub".to_string()],
            code_challenge_methods_supported: vec!["S256".to_string()],
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("issuer"));
        assert!(json.contains("jwks_uri"));
    }

    #[test]
    fn test_jwks_serialization() {
        let jwks = JwksResponse {
            keys: vec![JwkKey {
                kty: "RSA".to_string(),
                key_use: "sig".to_string(),
                kid: Some("key-1".to_string()),
                alg: "RS256".to_string(),
                n: Some("modulus".to_string()),
                e: Some("AQAB".to_string()),
            }],
        };

        let json = serde_json::to_string(&jwks).unwrap();
        assert!(json.contains("\"keys\""));
        assert!(json.contains("\"kty\":\"RSA\""));
        assert!(json.contains("\"use\":\"sig\""));
    }
}

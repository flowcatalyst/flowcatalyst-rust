//! OIDC Authentication Service
//!
//! Handles authentication with external OIDC identity providers.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use tracing::info;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

use crate::{Principal, UserScope, ExternalIdentity};
use crate::shared::error::{PlatformError, Result};

/// OIDC provider discovery document
#[derive(Debug, Clone, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: String,
    pub scopes_supported: Option<Vec<String>>,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Option<Vec<String>>,
    pub subject_types_supported: Vec<String>,
    pub id_token_signing_alg_values_supported: Vec<String>,
}

/// JWKS (JSON Web Key Set)
#[derive(Debug, Clone, Deserialize)]
pub struct Jwks {
    pub keys: Vec<JwkKey>,
}

/// Individual JWK key
#[derive(Debug, Clone, Deserialize)]
pub struct JwkKey {
    pub kty: String,
    #[serde(default)]
    pub use_: Option<String>,
    #[serde(rename = "use")]
    pub key_use: Option<String>,
    pub kid: Option<String>,
    pub alg: Option<String>,
    pub n: Option<String>,
    pub e: Option<String>,
    pub x: Option<String>,
    pub y: Option<String>,
    pub crv: Option<String>,
}

/// Standard OIDC ID token claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdTokenClaims {
    /// Issuer
    pub iss: String,
    /// Subject (unique user ID from IDP)
    pub sub: String,
    /// Audience (client ID)
    pub aud: StringOrVec,
    /// Expiration
    pub exp: i64,
    /// Issued at
    pub iat: i64,
    /// Auth time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_time: Option<i64>,
    /// Nonce
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Email
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Email verified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    /// Name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Given name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    /// Family name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    /// Picture URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// Groups/roles (common extension)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    /// Roles (common extension)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,
}

/// Audience can be a string or array
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}

impl StringOrVec {
    pub fn contains(&self, value: &str) -> bool {
        match self {
            StringOrVec::String(s) => s == value,
            StringOrVec::Vec(v) => v.iter().any(|s| s == value),
        }
    }
}

/// OIDC token response
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// OIDC provider configuration
#[derive(Debug, Clone)]
pub struct OidcProviderConfig {
    pub provider_id: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub issuer_url: String,
    pub scopes: Vec<String>,
    pub redirect_uri: String,
}

/// Cached OIDC provider info
pub struct CachedProvider {
    pub config: OidcProviderConfig,
    pub discovery: OidcDiscovery,
    pub jwks: Jwks,
    pub last_refresh: DateTime<Utc>,
}

/// OIDC authentication service
pub struct OidcService {
    http_client: reqwest::Client,
    providers: Arc<RwLock<HashMap<String, CachedProvider>>>,
    jwks_cache_ttl_secs: u64,
}

impl OidcService {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            providers: Arc::new(RwLock::new(HashMap::new())),
            jwks_cache_ttl_secs: 3600, // 1 hour
        }
    }

    /// Register an OIDC provider
    pub async fn register_provider(&self, config: OidcProviderConfig) -> Result<()> {
        let discovery_url = format!("{}/.well-known/openid-configuration", config.issuer_url);

        info!("Fetching OIDC discovery document from {}", discovery_url);

        let discovery: OidcDiscovery = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to fetch OIDC discovery: {}", e),
            })?
            .json()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to parse OIDC discovery: {}", e),
            })?;

        info!("Fetching JWKS from {}", discovery.jwks_uri);

        let jwks: Jwks = self
            .http_client
            .get(&discovery.jwks_uri)
            .send()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to fetch JWKS: {}", e),
            })?
            .json()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to parse JWKS: {}", e),
            })?;

        let cached = CachedProvider {
            config: config.clone(),
            discovery,
            jwks,
            last_refresh: Utc::now(),
        };

        let mut providers = self.providers.write().await;
        providers.insert(config.provider_id.clone(), cached);

        info!("Registered OIDC provider: {}", config.provider_id);
        Ok(())
    }

    /// Build the authorization URL for a provider
    pub async fn get_authorization_url(
        &self,
        provider_id: &str,
        state: &str,
        nonce: Option<&str>,
    ) -> Result<String> {
        let providers = self.providers.read().await;
        let provider = providers.get(provider_id).ok_or_else(|| PlatformError::NotFound {
            entity_type: "OidcProvider".to_string(),
            id: provider_id.to_string(),
        })?;

        let scopes = provider.config.scopes.join(" ");
        let mut url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            provider.discovery.authorization_endpoint,
            urlencoding::encode(&provider.config.client_id),
            urlencoding::encode(&provider.config.redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state),
        );

        if let Some(n) = nonce {
            url.push_str(&format!("&nonce={}", urlencoding::encode(n)));
        }

        Ok(url)
    }

    /// Exchange authorization code for tokens
    pub async fn exchange_code(
        &self,
        provider_id: &str,
        code: &str,
    ) -> Result<TokenResponse> {
        let providers = self.providers.read().await;
        let provider = providers.get(provider_id).ok_or_else(|| PlatformError::NotFound {
            entity_type: "OidcProvider".to_string(),
            id: provider_id.to_string(),
        })?;

        let mut params = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &provider.config.redirect_uri),
            ("client_id", &provider.config.client_id),
        ];

        let client_secret;
        if let Some(ref secret) = provider.config.client_secret {
            client_secret = secret.clone();
            params.push(("client_secret", &client_secret));
        }

        let response = self
            .http_client
            .post(&provider.discovery.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Token exchange failed: {}", e),
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(PlatformError::Internal {
                message: format!("Token exchange failed: {}", error_text),
            });
        }

        response.json().await.map_err(|e| PlatformError::Internal {
            message: format!("Failed to parse token response: {}", e),
        })
    }

    /// Validate an ID token and extract claims
    pub async fn validate_id_token(
        &self,
        provider_id: &str,
        id_token: &str,
        expected_nonce: Option<&str>,
    ) -> Result<IdTokenClaims> {
        let providers = self.providers.read().await;
        let provider = providers.get(provider_id).ok_or_else(|| PlatformError::NotFound {
            entity_type: "OidcProvider".to_string(),
            id: provider_id.to_string(),
        })?;

        // Decode header to get kid
        let header = decode_header(id_token).map_err(|e| PlatformError::InvalidToken {
            message: format!("Invalid ID token header: {}", e),
        })?;

        // Find matching key
        let key = provider
            .jwks
            .keys
            .iter()
            .find(|k| {
                header.kid.as_ref().map_or(true, |kid| k.kid.as_ref() == Some(kid))
            })
            .ok_or_else(|| PlatformError::InvalidToken {
                message: "No matching key found in JWKS".to_string(),
            })?;

        // Build decoding key
        let decoding_key = match key.kty.as_str() {
            "RSA" => {
                let n = key.n.as_ref().ok_or_else(|| PlatformError::InvalidToken {
                    message: "Missing 'n' in RSA key".to_string(),
                })?;
                let e = key.e.as_ref().ok_or_else(|| PlatformError::InvalidToken {
                    message: "Missing 'e' in RSA key".to_string(),
                })?;
                DecodingKey::from_rsa_components(n, e).map_err(|e| PlatformError::InvalidToken {
                    message: format!("Invalid RSA key: {}", e),
                })?
            }
            _ => {
                return Err(PlatformError::InvalidToken {
                    message: format!("Unsupported key type: {}", key.kty),
                });
            }
        };

        // Build validation
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&provider.discovery.issuer]);
        validation.set_audience(&[&provider.config.client_id]);

        // Decode and validate
        let token_data = decode::<IdTokenClaims>(id_token, &decoding_key, &validation)
            .map_err(|e| PlatformError::InvalidToken {
                message: format!("Invalid ID token: {}", e),
            })?;

        let claims = token_data.claims;

        // Validate nonce if provided
        if let Some(expected) = expected_nonce {
            if claims.nonce.as_deref() != Some(expected) {
                return Err(PlatformError::InvalidToken {
                    message: "Nonce mismatch".to_string(),
                });
            }
        }

        // Validate audience
        if !claims.aud.contains(&provider.config.client_id) {
            return Err(PlatformError::InvalidToken {
                message: "Audience mismatch".to_string(),
            });
        }

        Ok(claims)
    }

    /// Get user info from the userinfo endpoint (optional, provides additional claims)
    pub async fn get_userinfo(
        &self,
        provider_id: &str,
        access_token: &str,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let providers = self.providers.read().await;
        let provider = providers.get(provider_id).ok_or_else(|| PlatformError::NotFound {
            entity_type: "OidcProvider".to_string(),
            id: provider_id.to_string(),
        })?;

        let userinfo_endpoint = provider
            .discovery
            .userinfo_endpoint
            .as_ref()
            .ok_or_else(|| PlatformError::Internal {
                message: "Provider does not support userinfo endpoint".to_string(),
            })?;

        let response = self
            .http_client
            .get(userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to fetch userinfo: {}", e),
            })?;

        if !response.status().is_success() {
            return Err(PlatformError::Internal {
                message: format!("Userinfo request failed: {}", response.status()),
            });
        }

        response.json().await.map_err(|e| PlatformError::Internal {
            message: format!("Failed to parse userinfo: {}", e),
        })
    }

    /// Refresh JWKS for a provider
    pub async fn refresh_jwks(&self, provider_id: &str) -> Result<()> {
        let providers = self.providers.read().await;
        let provider = providers.get(provider_id).ok_or_else(|| PlatformError::NotFound {
            entity_type: "OidcProvider".to_string(),
            id: provider_id.to_string(),
        })?;

        let jwks: Jwks = self
            .http_client
            .get(&provider.discovery.jwks_uri)
            .send()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to fetch JWKS: {}", e),
            })?
            .json()
            .await
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to parse JWKS: {}", e),
            })?;

        drop(providers);

        let mut providers = self.providers.write().await;
        if let Some(p) = providers.get_mut(provider_id) {
            p.jwks = jwks;
            p.last_refresh = Utc::now();
            info!("Refreshed JWKS for provider: {}", provider_id);
        }

        Ok(())
    }

    /// Check if provider's JWKS needs refresh
    pub async fn needs_jwks_refresh(&self, provider_id: &str) -> bool {
        let providers = self.providers.read().await;
        if let Some(p) = providers.get(provider_id) {
            let age = (Utc::now() - p.last_refresh).num_seconds() as u64;
            age > self.jwks_cache_ttl_secs
        } else {
            false
        }
    }

    /// List registered provider IDs
    pub async fn list_providers(&self) -> Vec<String> {
        let providers = self.providers.read().await;
        providers.keys().cloned().collect()
    }
}

impl Default for OidcService {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to map OIDC claims to platform principal
pub fn map_claims_to_principal(
    claims: &IdTokenClaims,
    provider_id: &str,
    scope: UserScope,
    client_id: Option<String>,
) -> Principal {
    let email = claims.email.clone().unwrap_or_else(|| format!("{}@{}", claims.sub, provider_id));
    let name = claims.name.clone().unwrap_or_else(|| {
        match (&claims.given_name, &claims.family_name) {
            (Some(g), Some(f)) => format!("{} {}", g, f),
            (Some(g), None) => g.clone(),
            (None, Some(f)) => f.clone(),
            _ => email.clone(),
        }
    });

    let mut principal = Principal::new_user(&email, scope);
    principal.name = name;

    if let Some(cid) = client_id {
        principal = principal.with_client_id(cid);
    }

    // Store external IDP reference
    principal.external_identity = Some(ExternalIdentity {
        provider_id: provider_id.to_string(),
        external_id: claims.sub.clone(),
    });

    principal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_or_vec() {
        let single: StringOrVec = serde_json::from_str("\"client123\"").unwrap();
        assert!(single.contains("client123"));
        assert!(!single.contains("other"));

        let multi: StringOrVec = serde_json::from_str("[\"client1\", \"client2\"]").unwrap();
        assert!(multi.contains("client1"));
        assert!(multi.contains("client2"));
        assert!(!multi.contains("client3"));
    }
}

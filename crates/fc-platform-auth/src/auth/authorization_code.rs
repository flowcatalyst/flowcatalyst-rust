//! Authorization Code Domain Model
//!
//! OAuth2 authorization codes for the authorization code flow.
//! Codes are short-lived (10 minutes), single-use, and bound to PKCE.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use fc_platform_core::shared::id::ClientId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use fc_platform_core::shared::enum_str::UnknownEnumValue;

/// PKCE code challenge method (RFC 7636 §4.2). `plain` parses so it can be
/// named in an error, but it is refused everywhere (as in the Go platform):
/// `/oauth/authorize` rejects it and a code bound to it never verifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PkceMethod {
    #[serde(rename = "S256")]
    S256,
    #[serde(rename = "plain")]
    Plain,
}

fc_platform_core::shared::enum_str::str_enum!(PkceMethod, "PKCE code challenge method", {
    S256 => "S256",
    Plain => "plain",
});

/// A PKCE binding: the challenge and how it was derived. A challenge without
/// a method can't be verified, so the two travel together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pkce {
    pub challenge: String,
    pub method: PkceMethod,
}

impl Pkce {
    /// Builds the binding from the wire's two optional parameters. No
    /// challenge means no PKCE. A challenge without a method is S256 (as in
    /// the Go port), not RFC 7636's `plain` default: the stronger reading of
    /// an ambiguous request, and what every current client sends anyway.
    pub fn from_parts(
        challenge: Option<String>,
        method: Option<&str>,
    ) -> Result<Option<Self>, UnknownEnumValue> {
        let Some(challenge) = challenge else {
            return Ok(None);
        };
        let method = match method {
            None | Some("") => PkceMethod::S256,
            Some(m) => m.parse()?,
        };
        Ok(Some(Self { challenge, method }))
    }

    /// Whether `verifier` produces this challenge. Only S256 is supported:
    /// a `plain` binding never verifies. Constant-time compare.
    pub fn verify(&self, verifier: &str) -> bool {
        match self.method {
            PkceMethod::S256 => {
                let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                computed.as_bytes().ct_eq(self.challenge.as_bytes()).into()
            }
            PkceMethod::Plain => false,
        }
    }
}

/// Authorization code for OAuth2 authorization code flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationCode {
    /// The authorization code value (64 char random string).
    pub code: String,

    /// OAuth client that initiated this authorization.
    pub client_id: String,

    /// The authenticated principal.
    pub principal_id: String,

    /// Redirect URI used in the authorization request.
    /// Must match exactly during token exchange.
    pub redirect_uri: String,

    /// Requested scopes.
    pub scope: Option<String>,

    /// PKCE binding (required for public clients).
    pub pkce: Option<Pkce>,

    /// OIDC nonce for replay protection.
    pub nonce: Option<String>,

    /// Client-provided state for CSRF protection.
    pub state: Option<String>,

    /// Client context for the authorization.
    pub context_client_id: Option<ClientId>,

    /// When this code was created.
    pub created_at: DateTime<Utc>,

    /// When this code expires.
    pub expires_at: DateTime<Utc>,

    /// Whether this code has been used (single-use enforcement).
    pub used: bool,
}

/// What a new authorization code is bound to, by field so the three
/// adjacent strings cannot be passed in the wrong order.
#[derive(Debug, Clone)]
pub struct NewAuthorizationCode {
    /// The authorization code value.
    pub code: String,
    /// The OAuth client's public id (not a tenant client).
    pub client_id: String,
    /// The authenticated subject: a principal id, or a portal identity's.
    pub principal_id: String,
    /// The redirect URI of the authorization request.
    pub redirect_uri: String,
}

impl AuthorizationCode {
    /// Default expiration time for authorization codes (10 minutes)
    const DEFAULT_EXPIRY_MINUTES: i64 = 10;

    /// Create a new authorization code.
    pub fn new(new: NewAuthorizationCode) -> Self {
        let NewAuthorizationCode {
            code,
            client_id,
            principal_id,
            redirect_uri,
        } = new;
        let now = Utc::now();
        Self {
            code,
            client_id,
            principal_id,
            redirect_uri,
            scope: None,
            pkce: None,
            nonce: None,
            state: None,
            context_client_id: None,
            created_at: now,
            expires_at: now + Duration::minutes(Self::DEFAULT_EXPIRY_MINUTES),
            used: false,
        }
    }

    /// Bind a PKCE challenge.
    pub fn with_pkce(mut self, pkce: Option<Pkce>) -> Self {
        self.pkce = pkce;
        self
    }

    /// Check if this code is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if this code is valid (not used and not expired).
    pub fn is_valid(&self) -> bool {
        !self.used && !self.is_expired()
    }

    /// Mark this code as used.
    pub fn mark_used(&mut self) {
        self.used = true;
    }
}

// Note: Conversion from oauth_oidc_payloads is handled in AuthorizationCodeRepository::from_model

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_code() {
        let code = AuthorizationCode::new(NewAuthorizationCode {
            code: "test-code".to_string(),
            client_id: "client-123".to_string(),
            principal_id: "principal-456".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
        });

        assert_eq!(code.code, "test-code");
        assert_eq!(code.client_id, "client-123");
        assert!(!code.used);
        assert!(code.is_valid());
    }

    #[test]
    fn test_code_with_pkce() {
        let code = AuthorizationCode::new(NewAuthorizationCode {
            code: "test-code".to_string(),
            client_id: "client-123".to_string(),
            principal_id: "principal-456".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
        })
        .with_pkce(Some(Pkce {
            challenge: "challenge".to_string(),
            method: PkceMethod::S256,
        }));

        assert_eq!(
            code.pkce,
            Some(Pkce {
                challenge: "challenge".to_string(),
                method: PkceMethod::S256
            })
        );
    }

    #[test]
    fn pkce_from_parts() {
        assert_eq!(Pkce::from_parts(None, Some("S256")), Ok(None));
        // A challenge without a method binds S256 rather than being dropped.
        assert_eq!(
            Pkce::from_parts(Some("c".to_string()), None)
                .unwrap()
                .unwrap()
                .method,
            PkceMethod::S256
        );
        assert_eq!(
            Pkce::from_parts(Some("c".to_string()), Some("plain"))
                .unwrap()
                .unwrap()
                .method,
            PkceMethod::Plain
        );
        assert!(Pkce::from_parts(Some("c".to_string()), Some("s256")).is_err());
    }

    #[test]
    fn pkce_verify() {
        // RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let s256 = Pkce {
            challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
            method: PkceMethod::S256,
        };
        assert!(s256.verify(verifier));
        assert!(!s256.verify("wrong"));
        // `plain` is refused: even the verifier itself doesn't match.
        let plain = Pkce {
            challenge: verifier.to_string(),
            method: PkceMethod::Plain,
        };
        assert!(!plain.verify(verifier));
    }

    #[test]
    fn test_mark_used() {
        let mut code = AuthorizationCode::new(NewAuthorizationCode {
            code: "test-code".to_string(),
            client_id: "client-123".to_string(),
            principal_id: "principal-456".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
        });

        assert!(code.is_valid());
        code.mark_used();
        assert!(!code.is_valid());
    }
}

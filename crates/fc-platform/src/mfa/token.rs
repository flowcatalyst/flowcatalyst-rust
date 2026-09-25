//! The short-lived, single-purpose tokens that carry a half-authenticated
//! user from `/auth/login` to the `/auth/2fa/*` routes (Go
//! `auth/mfatoken/mfatoken.go`):
//!
//! - `mfa_pending`: the password verified; a 2FA challenge is owed before a
//!   session is issued.
//! - `mfa_enroll`: the password verified, the domain requires 2FA and the
//!   user has no factor yet; they enrol before a session is issued.
//!
//! HS256 with a secret derived from the RSA signing key, as Go: stable
//! across instances (and across Go and Rust at cutover), and never usable as
//! a session, which is RS256-only.

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::AuthService;

/// A token's single allowed use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Pending,
    Enroll,
}

impl Purpose {
    fn claim(self) -> &'static str {
        match self {
            Purpose::Pending => "mfa_pending",
            Purpose::Enroll => "mfa_enroll",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    prp: String,
    iat: i64,
    nbf: i64,
    exp: i64,
}

/// Mints and checks 2FA step tokens.
pub struct MfaTokenIssuer {
    secret: [u8; 32],
    issuer: String,
}

impl MfaTokenIssuer {
    /// Go: `SHA-256("fc-mfa-token-v1|" || D)`.
    pub fn new(auth: &AuthService, issuer: impl Into<String>) -> Self {
        Self {
            secret: auth.derived_secret(b"fc-mfa-token-v1|"),
            issuer: issuer.into(),
        }
    }

    pub fn mint(&self, subject: &str, purpose: Purpose, ttl_secs: i64) -> Option<String> {
        if subject.is_empty() {
            return None;
        }
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            iss: self.issuer.clone(),
            sub: subject.to_string(),
            prp: purpose.claim().to_string(),
            iat: now,
            nbf: now,
            exp: now + ttl_secs,
        };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .ok()
    }

    /// The subject of a valid, unexpired token of this issuer and purpose.
    pub fn parse(&self, token: &str, want: Purpose) -> Option<String> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.validate_nbf = true;
        validation.validate_aud = false;
        validation.leeway = 0;
        let data =
            decode::<Claims>(token, &DecodingKey::from_secret(&self.secret), &validation).ok()?;
        (data.claims.prp == want.claim() && !data.claims.sub.is_empty()).then_some(data.claims.sub)
    }
}

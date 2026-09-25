//! `auth: platform` and versioned calls (Java
//! `fnhost/http/BearerAuthenticator.java` over the server's
//! `shared/auth/JwtVerifier.java` and `TokenClaims.java`): a bearer JWT
//! verified locally, RS256 only, against the keys [`JwksKeySource`] holds,
//! with the platform authenticator's own rules and rejection messages.

use std::collections::BTreeSet;
use std::sync::Arc;

use base64::Engine;
use chrono::{DateTime, Utc};
use fc_function_abi::Principal;
use rsa::{Pkcs1v15Sign, RsaPublicKey};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::jwks::{JwksKeySource, BASE64_URL};
use crate::clock::SharedClock;

/// `token_use` of a token that identifies a user but grants no API access.
const TOKEN_USE_IDENTITY: &str = "identity";

/// The `clients` / `applications` entry meaning every one.
pub const SCOPE_WILDCARD: &str = "*";

/// The claims read off a verified token (Java `TokenClaims`, minus `email`
/// and `name`, which the host never passes on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenClaims {
    /// `sub`.
    pub subject: String,
    /// `type`.
    pub principal_type: Option<String>,
    /// `tier` (`ANCHOR` / `PARTNER` / `CLIENT`).
    pub tier: Option<String>,
    /// Client ids (from `"{id}:{label}"` pairs), or `*` for every client.
    pub clients: Vec<String>,
    pub roles: Vec<String>,
    /// Application ids; a `*` entry sets `all_applications` instead.
    pub applications: Vec<String>,
    /// `all_applications`, or `applications` holding `*`.
    pub all_applications: bool,
    /// `scope`, split on whitespace.
    pub permissions: Vec<String>,
}

impl TokenClaims {
    /// Every claim onto the function's [`Principal`] (Java
    /// `FnHttpServer.principalFrom`, spec `function-caller-claims.md` §3);
    /// an absent `type` is `unknown`.
    pub fn principal(&self) -> Principal {
        Principal {
            id: self.subject.clone(),
            principal_type: self
                .principal_type
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
            tier: self.tier.clone(),
            clients: self.clients.clone(),
            roles: self.roles.clone(),
            applications: self.applications.clone(),
            all_applications: self.all_applications,
            permissions: self.permissions.iter().cloned().collect::<BTreeSet<_>>(),
        }
    }
}

pub struct BearerAuthenticator {
    keys: Arc<JwksKeySource>,
    clock: SharedClock,
}

impl BearerAuthenticator {
    pub fn new(keys: Arc<JwksKeySource>, clock: SharedClock) -> Self {
        Self { keys, clock }
    }

    pub fn key_source(&self) -> &Arc<JwksKeySource> {
        &self.keys
    }

    /// `Err` is the reason the 401 body's `message` carries.
    pub async fn authenticate(&self, authorization: Option<&str>) -> Result<TokenClaims, String> {
        let Some(token) = authorization.and_then(|h| h.strip_prefix("Bearer ")) else {
            return Err("missing bearer token".to_owned());
        };
        let parsed = Jwt::parse(token).map_err(|e| format!("token is malformed: {e}"))?;
        self.keys.ensure_known(parsed.kid.as_deref()).await;
        let Some(issuer) = self.keys.issuer() else {
            return Err("platform issuer could not be discovered".to_owned());
        };
        let keys = self.keys.keys();
        if keys.is_empty() {
            return Err("no verification keys available".to_owned());
        }
        verify(&parsed, &issuer, &keys, self.clock.now())
    }
}

/// A compact JWS, split and its header read (nimbus `SignedJWT.parse`).
struct Jwt {
    alg: String,
    kid: Option<String>,
    signing_input: String,
    payload: String,
    signature: String,
}

impl Jwt {
    fn parse(token: &str) -> Result<Self, String> {
        let parts: Vec<&str> = token.split('.').collect();
        let [header, payload, signature] = parts.as_slice() else {
            return Err("Invalid serialized JWS object: Missing part delimiters".to_owned());
        };
        let header = decode_object(header).map_err(|e| format!("Invalid JWS header: {e}"))?;
        let alg = match header.get("alg") {
            Some(Value::String(alg)) => alg.clone(),
            _ => return Err("Invalid JWS header: Missing \"alg\" in header JSON object".to_owned()),
        };
        let kid = match header.get("kid") {
            Some(Value::String(kid)) => Some(kid.clone()),
            _ => None,
        };
        Ok(Self {
            alg,
            kid,
            signing_input: format!("{}.{}", parts[0], parts[1]),
            payload: (*payload).to_owned(),
            signature: (*signature).to_owned(),
        })
    }
}

fn decode_object(part: &str) -> Result<Map<String, Value>, String> {
    let bytes = BASE64_URL
        .decode(part)
        .map_err(|_| "not base64url".to_owned())?;
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err("not a JSON object".to_owned()),
    }
}

/// `DigestInfo` for SHA-256 (RFC 8017 §9.2 note 1).
const SHA256_DIGEST_INFO: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

/// Java `JwtVerifier.verify` with RS256 keys and `audience = issuer`.
fn verify(
    jwt: &Jwt,
    issuer: &str,
    keys: &[RsaPublicKey],
    now: DateTime<Utc>,
) -> Result<TokenClaims, String> {
    if jwt.alg != "RS256" {
        return Err(format!(
            "token signature is invalid: unexpected signing method {}",
            jwt.alg
        ));
    }
    let signature = BASE64_URL
        .decode(&jwt.signature)
        .map_err(|_| "token signature is invalid".to_owned())?;
    let hashed = Sha256::digest(jwt.signing_input.as_bytes());
    let scheme = || Pkcs1v15Sign {
        hash_len: Some(32),
        prefix: SHA256_DIGEST_INFO.to_vec().into_boxed_slice(),
    };
    if !keys
        .iter()
        .any(|key| key.verify(scheme(), &hashed, &signature).is_ok())
    {
        return Err("token signature is invalid".to_owned());
    }
    let claims = decode_object(&jwt.payload)
        .map_err(|e| format!("token is malformed: Payload of JWS object is {e}"))?;
    let now_ms = now.timestamp_millis();
    if let Some(exp) = numeric_date_ms(&claims, "exp")? {
        if now_ms >= exp {
            return Err("token has invalid claims: token is expired".to_owned());
        }
    }
    if let Some(nbf) = numeric_date_ms(&claims, "nbf")? {
        if now_ms < nbf {
            return Err("token has invalid claims: token is not valid yet".to_owned());
        }
    }
    let token_issuer = match claims.get("iss") {
        None | Some(Value::Null) => None,
        Some(Value::String(iss)) => Some(iss.as_str()),
        Some(_) => {
            return Err("token is malformed: Unexpected type of JSON object member iss".to_owned())
        }
    };
    if token_issuer != Some(issuer) {
        return Err("sessiontoken: issuer not accepted".to_owned());
    }
    let audiences: Vec<&str> = match claims.get("aud") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(aud)) => vec![aud.as_str()],
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        Some(_) => {
            return Err("token is malformed: Unexpected type of JSON object member aud".to_owned())
        }
    };
    if !audiences.is_empty() && !audiences.contains(&issuer) {
        return Err("sessiontoken: audience not accepted (not a platform token)".to_owned());
    }
    let subject = match claims.get("sub") {
        Some(Value::String(sub)) if !sub.is_empty() => sub.clone(),
        _ => return Err("sessiontoken: token is missing sub claim".to_owned()),
    };
    let string = |name: &str| match claims.get(name) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let list = |name: &str| match claims.get(name) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    // An identity token (issued to relying parties without API access, and
    // to portal identities) is not an API credential: the platform refuses
    // it as a bearer, and so must a function endpoint, or a relying party
    // could replay a user's identity token at an `auth: platform` endpoint.
    if string("token_use").as_deref() == Some(TOKEN_USE_IDENTITY) {
        return Err("an identity token is not an API credential".to_owned());
    }
    let permissions = match string("scope") {
        Some(scope) if !crate::java::is_blank(&scope) => scope
            .split([' ', '\t', '\n', '\u{000B}', '\u{000C}', '\r'])
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    let applications = scope_ids(list("applications"));
    Ok(TokenClaims {
        subject,
        principal_type: string("type"),
        tier: string("tier"),
        clients: scope_ids(list("clients")),
        roles: list("roles"),
        all_applications: matches!(claims.get("all_applications"), Some(Value::Bool(true)))
            || applications.iter().any(|a| a == SCOPE_WILDCARD),
        applications: applications
            .into_iter()
            .filter(|a| a != SCOPE_WILDCARD)
            .collect(),
        permissions,
    })
}

/// `clients` / `applications` arrive as `"{id}:{label}"` pairs (or the `*`
/// wildcard); the reach check and the function see bare ids, as the
/// platform's own authenticator does. A bare id (an older token) is kept.
fn scope_ids(entries: Vec<String>) -> Vec<String> {
    entries
        .into_iter()
        .map(|entry| match entry.split_once(':') {
            Some((id, _label)) => id.to_owned(),
            None => entry,
        })
        .collect()
}

/// A NumericDate claim in milliseconds; a non-number is malformed.
fn numeric_date_ms(claims: &Map<String, Value>, name: &str) -> Result<Option<i64>, String> {
    match claims.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .map(|seconds| seconds.saturating_mul(1000))),
        Some(_) => Err(format!(
            "token is malformed: Unexpected type of JSON object member {name}"
        )),
    }
}

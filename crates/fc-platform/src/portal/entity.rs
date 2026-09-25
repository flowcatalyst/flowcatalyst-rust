//! Portal identity plane — domain types (Go `internal/platform/portalidentity/
//! {entity,app}.go`, `internal/platform/portalauth/flow.go`).
//!
//! Portal end-users are a SEPARATE population from `iam_principals`: one
//! identity per (client, email) context, granted per portal app. Nothing here
//! knows about SQL; the rows live in `portal/repository.rs`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Prefix of a portal identity id (Go `tsid.PortalUser`): `ptu_…`.
pub const PORTAL_USER_PREFIX: &str = "ptu";
/// Prefix of a portal app id (Go `tsid.PortalApp`): `pta_…`.
pub const PORTAL_APP_PREFIX: &str = "pta";

/// Whether `subject` is a portal identity id rather than a principal id. Reset
/// tokens and authorization codes key both populations in one column; the
/// TSID prefix tells them apart (Go `portalSubjectPrefix`).
pub fn is_portal_subject(subject: &str) -> bool {
    subject.starts_with("ptu_")
}

/// The identity state. Only ACTIVE may authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IdentityStatus {
    Active,
    Disabled,
}

impl IdentityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Disabled => "DISABLED",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ACTIVE" => Some(Self::Active),
            "DISABLED" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// How an identity (or an app grant) came to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IdentitySource {
    /// Created by the portal backend via `/api/portal-users`.
    Invite,
    /// Created by a first SSO login through the portal plane.
    Jit,
    /// An app grant added directly (`POST /api/portal-users/{id}/apps`).
    Admin,
}

impl IdentitySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Invite => "INVITE",
            Self::Jit => "JIT",
            Self::Admin => "ADMIN",
        }
    }

    /// Stored values are Go's strings; an unknown value reads as INVITE,
    /// which is what Go's ensure writes for anything that is not JIT.
    pub fn parse(s: &str) -> Self {
        match s {
            "JIT" => Self::Jit,
            "ADMIN" => Self::Admin,
            _ => Self::Invite,
        }
    }
}

/// The admin-facing lifecycle of an identity, derived (never stored) from
/// status, credentials, logins and the invite dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessState {
    Invited,
    InviteExpired,
    Active,
    Suspended,
}

impl AccessState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Invited => "INVITED",
            Self::InviteExpired => "INVITE_EXPIRED",
            Self::Active => "ACTIVE",
            Self::Suspended => "SUSPENDED",
        }
    }
}

/// One identity's access to one portal app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppGrant {
    pub app_id: String,
    pub source: IdentitySource,
    pub granted_at: DateTime<Utc>,
}

/// One portal end-user identity in one client's portal context.
#[derive(Debug, Clone)]
pub struct PortalIdentity {
    pub id: String,
    pub client_id: String,
    /// Stored lower-cased.
    pub email: String,
    /// Empty when unknown (stored NULL).
    pub name: String,
    /// `None` until the invite completes (or forever, for SSO-only).
    pub password_hash: Option<String>,
    pub status: IdentityStatus,
    pub source: IdentitySource,
    pub last_login_at: Option<DateTime<Utc>>,
    /// Latest invite; infrastructure bookkeeping written by the invite path,
    /// not by `persist`. A `None` expiry with `invited_at` set is an SSO
    /// invite.
    pub invited_at: Option<DateTime<Utc>>,
    pub invite_expires_at: Option<DateTime<Utc>>,
    /// The portal apps this identity may sign in to. `persist` inserts any
    /// grant here that is missing and deletes only the grants revoked since
    /// load — never "whatever isn't in apps".
    pub apps: Vec<AppGrant>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    revoked: Vec<String>,
}

impl PortalIdentity {
    /// A fresh ACTIVE identity; the email is normalised to lower case.
    pub fn new(client_id: &str, email: &str, name: &str, source: IdentitySource) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate_with_prefix(PORTAL_USER_PREFIX),
            client_id: client_id.to_string(),
            email: normalize_email(email),
            name: name.to_string(),
            password_hash: None,
            status: IdentityStatus::Active,
            source,
            last_login_at: None,
            invited_at: None,
            invite_expires_at: None,
            apps: Vec::new(),
            created_at: now,
            updated_at: now,
            revoked: Vec::new(),
        }
    }

    /// Rebuild from stored columns (the repository's row mapping).
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        id: String,
        client_id: String,
        email: String,
        name: Option<String>,
        password_hash: Option<String>,
        status: IdentityStatus,
        source: IdentitySource,
        last_login_at: Option<DateTime<Utc>>,
        invited_at: Option<DateTime<Utc>>,
        invite_expires_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            client_id,
            email,
            name: name.unwrap_or_default(),
            password_hash,
            status,
            source,
            last_login_at,
            invited_at,
            invite_expires_at,
            apps: Vec::new(),
            created_at,
            updated_at,
            revoked: Vec::new(),
        }
    }

    pub fn has_password(&self) -> bool {
        self.password_hash.as_deref().is_some_and(|h| !h.is_empty())
    }

    pub fn can_sign_in_with_password(&self) -> bool {
        self.status == IdentityStatus::Active && self.has_password()
    }

    pub fn has_app(&self, app_id: &str) -> bool {
        self.apps.iter().any(|g| g.app_id == app_id)
    }

    /// Add the app grant, reporting whether it was new.
    pub fn grant(&mut self, app_id: &str, source: IdentitySource) -> bool {
        if app_id.is_empty() || self.has_app(app_id) {
            return false;
        }
        self.revoked.retain(|id| id != app_id);
        self.apps.push(AppGrant {
            app_id: app_id.to_string(),
            source,
            granted_at: Utc::now(),
        });
        true
    }

    /// Remove the app grant, reporting whether one existed. The removal is
    /// recorded so `persist` deletes exactly this grant row.
    pub fn revoke(&mut self, app_id: &str) -> bool {
        let before = self.apps.len();
        self.apps.retain(|g| g.app_id != app_id);
        if self.apps.len() == before {
            return false;
        }
        self.revoked.push(app_id.to_string());
        true
    }

    /// App ids revoked since load (what `persist` deletes).
    pub fn revoked_apps(&self) -> &[String] {
        &self.revoked
    }

    /// The admin-facing lifecycle at `now` (Go `Identity.State`).
    pub fn state(&self, now: DateTime<Utc>) -> AccessState {
        if self.status == IdentityStatus::Disabled {
            return AccessState::Suspended;
        }
        if self.has_password() || self.last_login_at.is_some() || self.source == IdentitySource::Jit
        {
            return AccessState::Active;
        }
        match self.invite_expires_at {
            Some(expires) if now >= expires => AccessState::InviteExpired,
            _ => AccessState::Invited,
        }
    }
}

impl crate::usecase::HasId for PortalIdentity {
    fn id(&self) -> &str {
        &self.id
    }
}

/// Lower-case and trim an email (the `(client_id, email)` key assumes it).
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// The domain of an email (after the last `@`), lower-cased, or empty when
/// the address is not well formed (Go `portalauth.emailDomainOf`).
pub fn email_domain_of(email: &str) -> String {
    let email = email.trim();
    match email.rfind('@') {
        Some(at) if at > 0 && at < email.len() - 1 => email[at + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// One portal a client runs. The portal app identifies itself on the admin
/// API by `code`; OAuth clients link to it (`oauth_clients.portal_app_id`).
#[derive(Debug, Clone)]
pub struct PortalApp {
    pub id: String,
    pub client_id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PortalApp {
    /// A fresh active app; the code is normalised.
    pub fn new(client_id: &str, code: &str, name: &str) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate_with_prefix(PORTAL_APP_PREFIX),
            client_id: client_id.to_string(),
            code: normalize_app_code(code),
            name: name.trim().to_string(),
            description: None,
            active: true,
            created_at: now,
            updated_at: now,
        }
    }
}

impl crate::usecase::HasId for PortalApp {
    fn id(&self) -> &str {
        &self.id
    }
}

/// Trim and lower-case a code so callers match it case-insensitively.
pub fn normalize_app_code(code: &str) -> String {
    code.trim().to_lowercase()
}

/// Whether a (normalised) code is well formed: `^[a-z0-9][a-z0-9_-]{0,99}$`.
pub fn valid_app_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    if bytes.is_empty() || bytes.len() > 100 {
        return false;
    }
    let first_ok = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    first_ok
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
}

/// Trimmed, or `None` when empty (Go `trimmedOrNil`).
pub fn trimmed_or_none(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|t| !t.is_empty()).map(String::from)
}

/// The summary of an OAuth client fronting an app.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedOAuthClient {
    pub id: String,
    pub client_id: String,
    pub client_name: String,
}

/// A portal-flagged OAuth client as the portal plane reads it: enough to
/// validate `/portal/authorize` and derive portal origins.
#[derive(Debug, Clone)]
pub struct PortalOAuthClient {
    pub id: String,
    pub client_id: String,
    pub client_name: String,
    pub active: bool,
    pub pkce_required: bool,
    pub portal_client_id: Option<String>,
    pub portal_app_id: Option<String>,
    pub redirect_uris: Vec<String>,
}

/// How long the portal login page may sit open (Go `flowTTL`).
pub const FLOW_TTL_MINUTES: i64 = 15;

/// One parked `/portal/authorize` request: the validated OAuth chain waiting
/// for the user to authenticate. Short-TTL; consumed when a code is issued.
#[derive(Debug, Clone)]
pub struct LoginFlow {
    pub id: String,
    pub oauth_client_id: String,
    pub portal_client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub state: String,
    pub nonce: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl LoginFlow {
    /// A flow with a fresh random id (32 random bytes, URL-safe base64) and
    /// the default TTL.
    pub fn new(
        oauth_client_id: &str,
        portal_client_id: &str,
        redirect_uri: &str,
        state: &str,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: random_token(32),
            oauth_client_id: oauth_client_id.to_string(),
            portal_client_id: portal_client_id.to_string(),
            redirect_uri: redirect_uri.to_string(),
            scope: None,
            state: state.to_string(),
            nonce: None,
            code_challenge: None,
            code_challenge_method: None,
            created_at: now,
            expires_at: now + Duration::minutes(FLOW_TTL_MINUTES),
        }
    }
}

/// `n` random bytes as unpadded URL-safe base64 (Go `randomToken`).
pub fn random_token(n: usize) -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = vec![0u8; n];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Go's `jsontime` layout: microsecond ISO-8601 in UTC.
pub fn micros(at: &DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident() -> PortalIdentity {
        PortalIdentity::new("clt_1", "  Pat@Example.COM ", "Pat", IdentitySource::Invite)
    }

    #[test]
    fn new_identity_is_active_and_lower_cased() {
        let i = ident();
        assert!(i.id.starts_with("ptu_") && i.id.len() == 17);
        assert_eq!(i.email, "pat@example.com");
        assert_eq!(i.status, IdentityStatus::Active);
        assert!(!i.can_sign_in_with_password());
    }

    #[test]
    fn state_derivation_follows_go() {
        let now = Utc::now();
        let mut i = ident();
        assert_eq!(i.state(now), AccessState::Invited);
        i.invite_expires_at = Some(now - Duration::hours(1));
        assert_eq!(i.state(now), AccessState::InviteExpired);
        i.password_hash = Some("h".into());
        assert_eq!(i.state(now), AccessState::Active);
        i.status = IdentityStatus::Disabled;
        assert_eq!(i.state(now), AccessState::Suspended);
        let jit = PortalIdentity::new("clt_1", "a@b.c", "", IdentitySource::Jit);
        assert_eq!(jit.state(now), AccessState::Active);
    }

    #[test]
    fn grant_and_revoke_track_removals() {
        let mut i = ident();
        assert!(i.grant("pta_1", IdentitySource::Admin));
        assert!(!i.grant("pta_1", IdentitySource::Admin));
        assert!(i.revoke("pta_1"));
        assert_eq!(i.revoked_apps(), ["pta_1".to_string()]);
        assert!(!i.revoke("pta_1"));
        assert!(i.grant("pta_1", IdentitySource::Admin));
        assert!(i.revoked_apps().is_empty());
    }

    #[test]
    fn app_codes() {
        assert!(valid_app_code("customer-portal_1"));
        assert!(valid_app_code("0x"));
        assert!(!valid_app_code("-leading-dash"));
        assert!(!valid_app_code(""));
        assert!(!valid_app_code("Upper"));
        assert!(!valid_app_code(&"a".repeat(101)));
        assert_eq!(normalize_app_code("  SUPPLIERS "), "suppliers");
    }

    #[test]
    fn email_domains() {
        assert_eq!(email_domain_of("a@Example.com"), "example.com");
        assert_eq!(email_domain_of("@example.com"), "");
        assert_eq!(email_domain_of("a@"), "");
        assert_eq!(email_domain_of("nope"), "");
        assert!(is_portal_subject("ptu_0ABC"));
        assert!(!is_portal_subject("prn_0ABC"));
    }
}

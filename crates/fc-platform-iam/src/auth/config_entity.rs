//! Authentication Configuration Entities — matches TypeScript domain

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::AnchorDomainId;
use fc_platform_core::shared::id::ClientAuthConfigId;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::IdpRoleMappingId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum AuthProvider {
    #[default]
    Internal,
    Oidc,
}

fc_platform_core::shared::enum_str::str_enum!(AuthProvider, "auth provider", {
    Internal => "INTERNAL",
    Oidc => "OIDC",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum AuthConfigType {
    Anchor,
    Partner,
    #[default]
    Client,
}

fc_platform_core::shared::enum_str::str_enum!(AuthConfigType, "auth config type", {
    Anchor => "ANCHOR",
    Partner => "PARTNER",
    Client => "CLIENT",
});

/// AnchorDomain — matches TypeScript AnchorDomain interface
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorDomain {
    pub id: AnchorDomainId,
    pub domain: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AnchorDomain {
    pub fn new(domain: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: AnchorDomainId::generate(),
            domain: domain.into().to_lowercase(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn matches_email(&self, email: &str) -> bool {
        email.to_lowercase().ends_with(&format!("@{}", self.domain))
    }
}

/// ClientAuthConfig — matches TypeScript ClientAuthConfig interface
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAuthConfig {
    pub id: ClientAuthConfigId,
    pub email_domain: String,
    pub config_type: AuthConfigType,
    pub primary_client_id: Option<ClientId>,
    pub additional_client_ids: Vec<ClientId>,
    pub granted_client_ids: Vec<ClientId>,
    pub auth_provider: AuthProvider,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_multi_tenant: bool,
    pub oidc_issuer_pattern: Option<String>,
    pub oidc_client_secret_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClientAuthConfig {
    pub fn new_internal(email_domain: impl Into<String>, config_type: AuthConfigType) -> Self {
        let now = Utc::now();
        Self {
            id: ClientAuthConfigId::generate(),
            email_domain: email_domain.into().to_lowercase(),
            config_type,
            primary_client_id: None,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            auth_provider: AuthProvider::Internal,
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_multi_tenant: false,
            oidc_issuer_pattern: None,
            oidc_client_secret_ref: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn new_partner(email_domain: impl Into<String>) -> Self {
        Self::new_internal(email_domain, AuthConfigType::Partner)
    }

    pub fn new_client(email_domain: impl Into<String>, client_id: ClientId) -> Self {
        let mut config = Self::new_internal(email_domain, AuthConfigType::Client);
        config.primary_client_id = Some(client_id);
        config
    }

    pub fn with_oidc(
        mut self,
        issuer_url: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        self.auth_provider = AuthProvider::Oidc;
        self.oidc_issuer_url = Some(issuer_url.into());
        self.oidc_client_id = Some(client_id.into());
        self
    }

    /// Check if the given OIDC issuer URL is valid for this config
    pub fn is_valid_issuer(&self, issuer: &str) -> bool {
        if let Some(ref issuer_url) = self.oidc_issuer_url {
            if issuer_url == issuer {
                return true;
            }
        }
        if let Some(ref pattern) = self.oidc_issuer_pattern {
            return issuer_matches_pattern(pattern, issuer);
        }
        false
    }

    /// Get all accessible client IDs for this config
    pub fn accessible_clients(&self) -> Vec<ClientId> {
        let mut clients = Vec::new();
        if let Some(ref primary) = self.primary_client_id {
            clients.push(primary.clone());
        }
        clients.extend(self.additional_client_ids.iter().cloned());
        clients.extend(self.granted_client_ids.iter().cloned());
        clients
    }
}

/// Whether a multi-tenant issuer pattern must match the WHOLE issuer
/// (`FC_OIDC_ISSUER_PATTERN_STRICT`, default false; same switch as Go's).
/// Off, a pattern is searched for inside the issuer, as before; an
/// unanchored pattern then also matches an issuer that merely CONTAINS a
/// match (`https://evil.example/?x=login.microsoftonline.com/...`), so a
/// warning names it once. Off by default because an operator may have
/// registered a prefix pattern that relies on the partial match.
pub fn oidc_issuer_pattern_strict() -> bool {
    static STRICT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *STRICT.get_or_init(|| env_flag("FC_OIDC_ISSUER_PATTERN_STRICT"))
}

/// Whether an upstream id_token carrying `email_verified: false` is refused
/// (`FC_OIDC_REJECT_UNVERIFIED_EMAIL`, default false; same switch as Go's).
/// Off by default: some IdPs send `false` for accounts the tenant trusts.
pub fn oidc_reject_unverified_email() -> bool {
    static REJECT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REJECT.get_or_init(|| env_flag("FC_OIDC_REJECT_UNVERIFIED_EMAIL"))
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Whether `issuer` satisfies the multi-tenant issuer `pattern`.
pub fn issuer_matches_pattern(pattern: &str, issuer: &str) -> bool {
    issuer_matches_pattern_with(pattern, issuer, oidc_issuer_pattern_strict())
}

fn issuer_matches_pattern_with(pattern: &str, issuer: &str, strict: bool) -> bool {
    if strict {
        // Full match: the pattern must cover the whole issuer.
        return regex::Regex::new(&format!("^(?:{pattern})$")).is_ok_and(|re| re.is_match(issuer));
    }
    let Ok(re) = regex::Regex::new(pattern) else {
        return false;
    };
    if !(pattern.starts_with('^') && pattern.ends_with('$') && !pattern.ends_with("\\$")) {
        warn_unanchored_once(pattern);
    }
    re.is_match(issuer)
}

fn warn_unanchored_once(pattern: &str) {
    static WARNED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    let seen = WARNED.get_or_init(Default::default);
    if let Ok(mut seen) = seen.lock() {
        if seen.insert(pattern.to_string()) {
            tracing::warn!(
                pattern,
                "OIDC issuer pattern is not anchored with ^...$; any issuer that merely CONTAINS a match is accepted. Anchor it or set FC_OIDC_ISSUER_PATTERN_STRICT=true"
            );
        }
    }
}

/// IDP Role Mapping — maps external IDP role names to internal platform roles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdpRoleMapping {
    pub id: IdpRoleMappingId,
    pub idp_type: String,
    pub idp_role_name: String,
    pub platform_role_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl IdpRoleMapping {
    pub fn new(
        idp_type: impl Into<String>,
        idp_role_name: impl Into<String>,
        platform_role_name: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: IdpRoleMappingId::generate(),
            idp_type: idp_type.into(),
            idp_role_name: idp_role_name.into(),
            platform_role_name: platform_role_name.into(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod issuer_pattern_tests {
    use super::issuer_matches_pattern_with as m;

    const ENTRA: &str = r"https://login\.microsoftonline\.com/[^/]+/v2\.0";

    /// B3: an unanchored pattern is satisfied by a substring today (the
    /// default keeps that); strict makes it a full match, and an anchored
    /// pattern behaves the same either way.
    #[test]
    fn strict_makes_the_pattern_a_full_match() {
        let hostile = "https://evil.example/?x=https://login.microsoftonline.com/t/v2.0";
        let real = "https://login.microsoftonline.com/tenant-a/v2.0";
        assert!(
            m(ENTRA, hostile, false),
            "the default keeps the partial match"
        );
        assert!(!m(ENTRA, hostile, true));
        assert!(m(ENTRA, real, true) && m(ENTRA, real, false));
        let anchored = format!("^{ENTRA}$");
        assert!(!m(&anchored, hostile, false) && !m(&anchored, hostile, true));
        assert!(m(&anchored, real, false) && m(&anchored, real, true));
        // Alternation stays inside the group in strict mode.
        assert!(!m("a|b", "xbx", true));
        assert!(m("a|b", "b", true));
        assert!(
            !m("(", "(", false) && !m("(", "(", true),
            "a bad pattern matches nothing"
        );
    }
}

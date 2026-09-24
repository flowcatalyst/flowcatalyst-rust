//! Principal Entity
//!
//! Unified model for users and service accounts.
//! Multi-tenant with UserScope determining client access.

use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Principal type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum PrincipalType {
    /// Human user
    #[default]
    User,
    /// Machine service account
    Service,
}

crate::shared::enum_str::str_enum!(PrincipalType, "principal type", {
    User => "USER",
    Service => "SERVICE",
});

/// User scope determines client access level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum UserScope {
    /// Platform admin - access to all clients
    Anchor,
    /// Partner user - access to multiple assigned clients
    Partner,
    /// Client user - access to single home client
    #[default]
    Client,
}

impl UserScope {
    /// Check if this scope has access to all clients
    pub fn is_anchor(&self) -> bool {
        matches!(self, Self::Anchor)
    }

    /// Check if this scope can access a specific client
    pub fn can_access_client(
        &self,
        client_id: &str,
        home_client_id: Option<&str>,
        assigned_clients: &[String],
    ) -> bool {
        match self {
            Self::Anchor => true,
            Self::Partner => assigned_clients.iter().any(|c| c == client_id),
            Self::Client => home_client_id == Some(client_id),
        }
    }
}

crate::shared::enum_str::str_enum!(UserScope, "user scope", {
    Anchor => "ANCHOR",
    Partner => "PARTNER",
    Client => "CLIENT",
});

/// User identity for human users
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIdentity {
    /// Email address (unique)
    pub email: String,

    /// Email verified
    #[serde(default)]
    pub email_verified: bool,

    /// First name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,

    /// Last name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,

    /// Profile picture URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,

    /// Phone number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,

    /// External IDP subject ID (for federated auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,

    /// IDP provider name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Password hash (for embedded auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,

    /// Last login time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<DateTime<Utc>>,
}

impl UserIdentity {
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            email_verified: false,
            first_name: None,
            last_name: None,
            picture_url: None,
            phone: None,
            external_id: None,
            provider: None,
            password_hash: None,
            last_login_at: None,
        }
    }

    pub fn with_name(
        mut self,
        first_name: impl Into<String>,
        last_name: impl Into<String>,
    ) -> Self {
        self.first_name = Some(first_name.into());
        self.last_name = Some(last_name.into());
        self
    }

    pub fn display_name(&self) -> String {
        match (&self.first_name, &self.last_name) {
            (Some(first), Some(last)) => format!("{} {}", first, last),
            (Some(first), None) => first.clone(),
            (None, Some(last)) => last.clone(),
            (None, None) => self.email.clone(),
        }
    }
}

/// Principal entity - unified user/service account
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    /// TSID as Crockford Base32 string
    pub id: String,

    /// Principal type (user or service)
    #[serde(rename = "type")]
    #[serde(default)]
    pub principal_type: PrincipalType,

    /// User scope (for users only)
    #[serde(default)]
    pub scope: UserScope,

    /// Home client ID (for CLIENT scope users)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Application ID (for service accounts created by an app)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,

    /// Display name
    pub name: String,

    /// Whether the principal is active
    #[serde(default = "default_active")]
    pub active: bool,

    /// User identity (for USER type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_identity: Option<UserIdentity>,

    /// Service account ID reference (for SERVICE type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,

    /// Assigned roles (loaded from iam_principal_roles junction table)
    #[serde(default)]
    pub roles: Vec<RoleAssignment>,

    /// Assigned client IDs (loaded from iam_client_access_grants)
    #[serde(default)]
    pub assigned_clients: Vec<String>,

    /// Client ID → identifier mapping (for JWT "id:identifier" claims)
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub client_identifier_map: std::collections::HashMap<String, String>,

    /// Accessible application IDs (loaded from iam_principal_application_access)
    #[serde(default)]
    pub accessible_application_ids: Vec<String>,

    /// Audit fields
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    /// External identity for OIDC-authenticated users
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<ExternalIdentity>,
}

/// External identity reference for OIDC-authenticated users
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIdentity {
    /// OIDC provider ID
    pub provider_id: String,
    /// Subject ID from the external IDP
    pub external_id: String,
}

fn default_active() -> bool {
    true
}

impl Principal {
    /// Create a new user principal
    pub fn new_user(email: impl Into<String>, scope: UserScope) -> Self {
        let email = email.into();
        let identity = UserIdentity::new(&email);
        let now = Utc::now();

        Self {
            id: crate::shared::tsid::generate(crate::EntityType::Principal),
            principal_type: PrincipalType::User,
            scope,
            client_id: None,
            application_id: None,
            name: identity.display_name(),
            active: true,
            user_identity: Some(identity),
            service_account_id: None,
            roles: vec![],
            assigned_clients: vec![],
            client_identifier_map: std::collections::HashMap::new(),
            accessible_application_ids: vec![],
            created_at: now,
            updated_at: now,
            external_identity: None,
        }
    }

    /// Create a new service principal
    pub fn new_service(service_account_id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::Principal),
            principal_type: PrincipalType::Service,
            scope: UserScope::Anchor,
            client_id: None,
            application_id: None,
            name: name.into(),
            active: true,
            user_identity: None,
            service_account_id: Some(service_account_id.into()),
            roles: vec![],
            assigned_clients: vec![],
            client_identifier_map: std::collections::HashMap::new(),
            accessible_application_ids: vec![],
            created_at: now,
            updated_at: now,
            external_identity: None,
        }
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn with_application_id(mut self, application_id: impl Into<String>) -> Self {
        self.application_id = Some(application_id.into());
        self
    }

    pub fn assign_role(&mut self, role: impl Into<String>) {
        self.roles.push(RoleAssignment::new(role));
        self.updated_at = Utc::now();
    }

    pub fn assign_role_with_source(&mut self, role: impl Into<String>, source: AssignmentSource) {
        self.roles.push(RoleAssignment::with_source(role, source));
        self.updated_at = Utc::now();
    }

    pub fn assign_role_for_client(
        &mut self,
        role: impl Into<String>,
        client_id: impl Into<String>,
    ) {
        self.roles.push(RoleAssignment::for_client(role, client_id));
        self.updated_at = Utc::now();
    }

    /// Remove all roles from a specific source (e.g. `IdpSync`)
    pub fn remove_roles_by_source(&mut self, source: AssignmentSource) -> usize {
        let original_count = self.roles.len();
        self.roles.retain(|r| !r.has_source(source));
        let removed = original_count - self.roles.len();
        if removed > 0 {
            self.updated_at = Utc::now();
        }
        removed
    }

    /// Update last login timestamp
    pub fn update_last_login(&mut self) {
        if let Some(ref mut identity) = self.user_identity {
            identity.last_login_at = Some(Utc::now());
        }
        self.updated_at = Utc::now();
    }

    pub fn grant_client_access(&mut self, client_id: impl Into<String>) {
        let id = client_id.into();
        if !self.assigned_clients.contains(&id) {
            self.assigned_clients.push(id);
            self.updated_at = Utc::now();
        }
    }

    pub fn revoke_client_access(&mut self, client_id: &str) {
        self.assigned_clients.retain(|c| c != client_id);
        self.updated_at = Utc::now();
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r.role == role)
    }

    pub fn can_access_client(&self, client_id: &str) -> bool {
        self.scope
            .can_access_client(client_id, self.client_id.as_deref(), &self.assigned_clients)
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = Utc::now();
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.updated_at = Utc::now();
    }

    pub fn is_user(&self) -> bool {
        self.principal_type == PrincipalType::User
    }

    pub fn is_service(&self) -> bool {
        self.principal_type == PrincipalType::Service
    }

    pub fn email(&self) -> Option<&str> {
        self.user_identity.as_ref().map(|i| i.email.as_str())
    }
}

/// Client access grant — tracks which principals have access to which clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccessGrant {
    pub id: String,
    pub principal_id: String,
    pub client_id: String,
    pub granted_by: String,
    pub granted_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClientAccessGrant {
    pub fn new(
        principal_id: impl Into<String>,
        client_id: impl Into<String>,
        granted_by: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::Principal),
            principal_id: principal_id.into(),
            client_id: client_id.into(),
            granted_by: granted_by.into(),
            granted_at: now,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ── PrincipalType / UserScope enum roundtrips ─────────────────────────

    #[test]
    fn principal_type_roundtrip_rejects_unknown() {
        assert_eq!(PrincipalType::from_str("USER"), Ok(PrincipalType::User));
        assert_eq!(
            PrincipalType::from_str("SERVICE"),
            Ok(PrincipalType::Service)
        );
        // Unknown values are rejected (X-06)
        assert!(PrincipalType::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn user_scope_roundtrip_rejects_unknown() {
        assert_eq!(UserScope::from_str("ANCHOR"), Ok(UserScope::Anchor));
        assert_eq!(UserScope::from_str("PARTNER"), Ok(UserScope::Partner));
        assert_eq!(UserScope::from_str("CLIENT"), Ok(UserScope::Client));
        // Unknown values are rejected (X-06)
        assert!(UserScope::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn user_scope_is_anchor_helper() {
        assert!(UserScope::Anchor.is_anchor());
        assert!(!UserScope::Partner.is_anchor());
        assert!(!UserScope::Client.is_anchor());
    }

    // ── UserScope::can_access_client — core authorization invariant ───────

    #[test]
    fn anchor_scope_can_access_any_client() {
        let scope = UserScope::Anchor;
        assert!(scope.can_access_client("clt_any", None, &[]));
        assert!(scope.can_access_client("clt_foo", Some("clt_bar"), &[]));
    }

    #[test]
    fn partner_scope_requires_assigned_clients_membership() {
        let scope = UserScope::Partner;
        let assigned = vec!["clt_a".to_string(), "clt_b".to_string()];
        assert!(scope.can_access_client("clt_a", None, &assigned));
        assert!(scope.can_access_client("clt_b", None, &assigned));
        assert!(!scope.can_access_client("clt_other", None, &assigned));
        // home_client_id is ignored for Partner
        assert!(!scope.can_access_client("clt_home", Some("clt_home"), &assigned));
    }

    #[test]
    fn client_scope_matches_only_home_client() {
        let scope = UserScope::Client;
        assert!(scope.can_access_client("clt_home", Some("clt_home"), &[]));
        assert!(!scope.can_access_client("clt_other", Some("clt_home"), &[]));
        assert!(!scope.can_access_client("clt_x", None, &[]));
        // assigned_clients is ignored for Client scope
        assert!(!scope.can_access_client("clt_x", Some("clt_home"), &["clt_x".to_string()]));
    }

    // ── UserIdentity ──────────────────────────────────────────────────────

    #[test]
    fn user_identity_display_name_uses_first_last_when_both_present() {
        let id = UserIdentity::new("alice@example.com").with_name("Alice", "Smith");
        assert_eq!(id.display_name(), "Alice Smith");
    }

    #[test]
    fn user_identity_display_name_handles_partial_names() {
        let mut id = UserIdentity::new("bob@example.com");
        id.first_name = Some("Bob".to_string());
        assert_eq!(id.display_name(), "Bob");

        id.first_name = None;
        id.last_name = Some("Jones".to_string());
        assert_eq!(id.display_name(), "Jones");
    }

    #[test]
    fn user_identity_display_name_falls_back_to_email() {
        let id = UserIdentity::new("eve@example.com");
        assert_eq!(id.display_name(), "eve@example.com");
    }

    // ── Principal constructors ────────────────────────────────────────────

    #[test]
    fn new_user_sets_user_type_and_identity() {
        let p = Principal::new_user("alice@example.com", UserScope::Anchor);
        assert_eq!(p.principal_type, PrincipalType::User);
        assert_eq!(p.scope, UserScope::Anchor);
        assert!(p.active);
        assert!(p.user_identity.is_some());
        assert!(p.service_account_id.is_none());
        assert_eq!(p.email(), Some("alice@example.com"));
        assert!(p.is_user());
        assert!(!p.is_service());
    }

    #[test]
    fn new_service_sets_service_type_and_anchor_scope() {
        let p = Principal::new_service("svc_123", "Outbox Processor");
        assert_eq!(p.principal_type, PrincipalType::Service);
        assert_eq!(p.scope, UserScope::Anchor);
        assert!(p.user_identity.is_none());
        assert_eq!(p.service_account_id, Some("svc_123".to_string()));
        assert_eq!(p.name, "Outbox Processor");
        assert!(p.is_service());
        assert!(!p.is_user());
        assert!(p.email().is_none());
    }

    // ── Role assignment state changes ─────────────────────────────────────

    #[test]
    fn assign_role_appends_and_updates_timestamp() {
        let mut p = Principal::new_user("a@b.com", UserScope::Client);
        let before = p.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        p.assign_role("admin");
        assert_eq!(p.roles.len(), 1);
        assert_eq!(p.roles[0].role, "admin");
        assert!(p.updated_at > before);
    }

    #[test]
    fn has_role_returns_true_for_assigned_role() {
        let mut p = Principal::new_user("a@b.com", UserScope::Client);
        p.assign_role("editor");
        p.assign_role("viewer");
        assert!(p.has_role("editor"));
        assert!(p.has_role("viewer"));
        assert!(!p.has_role("admin"));
    }

    #[test]
    fn remove_roles_by_source_only_removes_matching_source() {
        let mut p = Principal::new_user("a@b.com", UserScope::Client);
        p.assign_role_with_source("role-idp-1", AssignmentSource::IdpSync);
        p.assign_role_with_source("role-idp-2", AssignmentSource::IdpSync);
        p.assign_role_with_source("role-manual", AssignmentSource::AdminAssigned);

        let removed = p.remove_roles_by_source(AssignmentSource::IdpSync);
        assert_eq!(removed, 2);
        assert_eq!(p.roles.len(), 1);
        assert_eq!(p.roles[0].role, "role-manual");
    }

    #[test]
    fn remove_roles_by_source_is_noop_when_no_match() {
        let mut p = Principal::new_user("a@b.com", UserScope::Client);
        p.assign_role_with_source("role-manual", AssignmentSource::AdminAssigned);
        let before = p.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));

        let removed = p.remove_roles_by_source(AssignmentSource::IdpSync);
        assert_eq!(removed, 0);
        // Timestamp should NOT update when nothing changed
        assert_eq!(p.updated_at, before);
    }

    // ── Client access grants ──────────────────────────────────────────────

    #[test]
    fn grant_client_access_is_idempotent() {
        let mut p = Principal::new_user("a@b.com", UserScope::Partner);
        p.grant_client_access("clt_1");
        p.grant_client_access("clt_1"); // duplicate
        p.grant_client_access("clt_2");
        assert_eq!(p.assigned_clients.len(), 2);
        assert!(p.assigned_clients.contains(&"clt_1".to_string()));
        assert!(p.assigned_clients.contains(&"clt_2".to_string()));
    }

    #[test]
    fn revoke_client_access_removes_only_that_client() {
        let mut p = Principal::new_user("a@b.com", UserScope::Partner);
        p.grant_client_access("clt_1");
        p.grant_client_access("clt_2");
        p.revoke_client_access("clt_1");
        assert_eq!(p.assigned_clients, vec!["clt_2".to_string()]);
    }

    // ── can_access_client delegation ──────────────────────────────────────

    #[test]
    fn principal_can_access_client_uses_its_scope() {
        // Client-scoped user with home client
        let p = Principal::new_user("u@c.com", UserScope::Client).with_client_id("clt_home");
        assert!(p.can_access_client("clt_home"));
        assert!(!p.can_access_client("clt_other"));

        // Partner-scoped user with assigned clients
        let mut partner = Principal::new_user("p@co.com", UserScope::Partner);
        partner.grant_client_access("clt_a");
        assert!(partner.can_access_client("clt_a"));
        assert!(!partner.can_access_client("clt_b"));

        // Anchor can access everything
        let anchor = Principal::new_user("admin@fc.com", UserScope::Anchor);
        assert!(anchor.can_access_client("clt_any"));
    }

    // ── Activate / deactivate ─────────────────────────────────────────────

    #[test]
    fn deactivate_and_activate_flip_active_and_bump_updated_at() {
        let mut p = Principal::new_user("a@b.com", UserScope::Client);
        assert!(p.active);
        let t0 = p.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));

        p.deactivate();
        assert!(!p.active);
        assert!(p.updated_at > t0);

        let t1 = p.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        p.activate();
        assert!(p.active);
        assert!(p.updated_at > t1);
    }
}

//! `Principal` — Rust analogue of the TS SDK's principal object.
//!
//! Wraps [`AuthContext`] (which already provides `is_anchor`, `has_role`,
//! `has_client_access`) and adds the permission/role aggregate helpers that
//! match the Fastify plugin's API surface 1:1.

use std::collections::HashSet;
use std::sync::Arc;

use crate::auth::AuthContext;

use super::rbac::RbacCatalogue;

/// How this principal was authenticated for the current request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMechanism {
    /// Bearer token from the `Authorization` header.
    Bearer,
    /// Session cookie (browser flow).
    Session,
}

/// Per-request principal. Built once by the auth middleware, then extracted
/// from request extensions by handlers.
#[derive(Debug, Clone)]
pub struct Principal {
    auth: AuthContext,
    mechanism: AuthMechanism,
    permissions: Arc<HashSet<String>>,
}

impl Principal {
    /// Build from an [`AuthContext`] + optional [`RbacCatalogue`]. The
    /// catalogue is used once at construction; per-request checks are O(1).
    pub fn from_auth(
        auth: AuthContext,
        mechanism: AuthMechanism,
        rbac: Option<&RbacCatalogue>,
    ) -> Self {
        let permissions: HashSet<String> = rbac
            .map(|c| c.resolve(&auth.claims.roles).into_iter().collect())
            .unwrap_or_default();
        Self {
            auth,
            mechanism,
            permissions: Arc::new(permissions),
        }
    }

    /// Borrow the underlying [`AuthContext`].
    pub fn auth(&self) -> &AuthContext {
        &self.auth
    }

    pub fn mechanism(&self) -> AuthMechanism {
        self.mechanism
    }

    pub fn id(&self) -> &str {
        self.auth.principal_id()
    }

    pub fn name(&self) -> &str {
        self.auth.name()
    }

    pub fn email(&self) -> Option<&str> {
        self.auth.email()
    }

    /// The tenancy tier (`ANCHOR` | `PARTNER` | `CLIENT`).
    pub fn scope(&self) -> &str {
        self.auth.claims.tier()
    }

    pub fn principal_type(&self) -> &str {
        &self.auth.claims.principal_type
    }

    pub fn clients(&self) -> &[String] {
        self.auth.client_ids()
    }

    pub fn roles(&self) -> &[String] {
        self.auth.roles()
    }

    pub fn applications(&self) -> &[String] {
        &self.auth.claims.applications
    }

    pub fn bearer_token(&self) -> &str {
        self.auth.bearer_token()
    }

    // ─── helpers matching the TS Fastify SDK ────────────────────────

    pub fn has_role(&self, role: &str) -> bool {
        self.auth.has_role(role)
    }

    /// ALL — every role must be present.
    pub fn has_roles<I, S>(&self, roles: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        roles.into_iter().all(|r| self.auth.has_role(r.as_ref()))
    }

    /// ANY — at least one role must be present.
    pub fn has_any_role<I, S>(&self, roles: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        roles.into_iter().any(|r| self.auth.has_role(r.as_ref()))
    }

    /// ALL — every permission must be granted (wildcards honoured).
    pub fn has_permission_to<I, S>(&self, permissions: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        permissions
            .into_iter()
            .all(|p| RbacCatalogue::matches_any(&self.permissions, p.as_ref()))
    }

    /// ANY — at least one permission must be granted.
    pub fn has_any_permission_to<I, S>(&self, permissions: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        permissions
            .into_iter()
            .any(|p| RbacCatalogue::matches_any(&self.permissions, p.as_ref()))
    }

    pub fn is_anchor(&self) -> bool {
        self.auth.is_anchor()
    }

    pub fn can_access_client(&self, client_id: &str) -> bool {
        self.auth.has_client_access(client_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AccessTokenClaims;
    use crate::auth::axum::rbac::RbacBuilder;

    fn make_principal(roles: &[&str], catalog: Option<&RbacCatalogue>) -> Principal {
        let claims = AccessTokenClaims {
            sub: "prn_x".into(),
            iss: "fc".into(),
            aud: "fc".into(),
            exp: 9_999_999_999,
            iat: 1_000_000_000,
            nbf: 1_000_000_000,
            jti: "jti".into(),
            principal_type: "USER".into(),
            scope: "CLIENT".into(),
            email: None,
            name: "Tester".into(),
            clients: vec!["clt_a".into()],
            roles: roles.iter().map(|s| s.to_string()).collect(),
            applications: vec![],
            ..Default::default()
        };
        Principal::from_auth(
            AuthContext::new(claims, "tok".into()),
            AuthMechanism::Bearer,
            catalog,
        )
    }

    #[test]
    fn has_roles_is_all() {
        let p = make_principal(&["a", "b"], None);
        assert!(p.has_roles(["a", "b"]));
        assert!(!p.has_roles(["a", "z"]));
    }

    #[test]
    fn has_any_role_is_any() {
        let p = make_principal(&["a"], None);
        assert!(p.has_any_role(["x", "a"]));
        assert!(!p.has_any_role(["x", "y"]));
    }

    #[test]
    fn permission_wildcard_works() {
        let rbac = RbacBuilder::new()
            .role("admin")
            .grants(["billing:*"])
            .build();
        let p = make_principal(&["admin"], Some(&rbac));
        assert!(p.has_permission_to(["billing:read"]));
        assert!(p.has_permission_to(["billing:invoice:export"]));
        assert!(!p.has_permission_to(["ticket:read"]));
    }

    #[test]
    fn has_permission_to_is_all() {
        let rbac = RbacBuilder::new()
            .role("admin")
            .grants(["billing:read", "billing:write"])
            .build();
        let p = make_principal(&["admin"], Some(&rbac));
        assert!(p.has_permission_to(["billing:read", "billing:write"]));
        assert!(!p.has_permission_to(["billing:read", "ticket:read"]));
    }

    #[test]
    fn has_any_permission_to_is_any() {
        let rbac = RbacBuilder::new()
            .role("a")
            .grants(["x"])
            .build();
        let p = make_principal(&["a"], Some(&rbac));
        assert!(p.has_any_permission_to(["y", "x"]));
        assert!(!p.has_any_permission_to(["y", "z"]));
    }

    #[test]
    fn permission_set_empty_without_rbac() {
        let p = make_principal(&["a"], None);
        assert!(!p.has_permission_to(["any"]));
    }
}

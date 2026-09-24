use std::collections::BTreeSet;

/// Who a request came from. Mirrors the sealed interface Java
/// `function-api/src/main/java/io/flowcatalyst/function/Caller.java`: one case
/// per `auth` mode an endpoint declares.
///
/// | `auth` | `Caller` |
/// |---|---|
/// | `webhook` | [`Caller::Platform`]: a verified delivery (subscription, dispatch job or scheduled job) |
/// | `platform` | [`Caller::Principal`]: a platform bearer token verified against the platform's JWKS |
/// | `none` | [`Caller::Anonymous`]: the host checked nothing |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// A verified webhook delivery. Nothing distinguishes two deliveries at
    /// this level; parse the body with [`crate::Webhook`] to learn which
    /// subscription or job fired.
    Platform,
    /// An authenticated platform principal.
    Principal(Principal),
    /// The host performed no authentication.
    Anonymous,
}

impl Caller {
    /// The principal, when the caller is one.
    pub fn principal(&self) -> Option<&Principal> {
        match self {
            Caller::Principal(p) => Some(p),
            Caller::Platform | Caller::Anonymous => None,
        }
    }
}

/// An authenticated platform principal: a user or service-account bearer
/// token the host verified before the call reached the function. Mirrors Java
/// `Caller.Principal` (`docs/spec/function-caller-claims.md` §1 in the Java
/// repo).
///
/// Every field comes straight off the verified token. `email` and `name` are
/// deliberately not carried: a function has no business with them, and they
/// are PII the token happens to hold.
///
/// The methods restate the platform's own authorisation rules exactly, so a
/// function's check answers as the platform's would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// The principal's id (JWT `sub`).
    pub id: String,
    /// The principal's kind (e.g. `user`, `service-account`). A raw string, as
    /// in Java, which carries it verbatim from the token rather than as an
    /// enum; kept lenient so a new kind never makes a request undecodable.
    pub principal_type: String,
    /// The tenancy tier (`ANCHOR` / `PARTNER` / `CLIENT`), `None` when absent.
    /// A raw string, as in Java (`isAnchor` is `"ANCHOR".equals(tier)`), so an
    /// unrecognised tier is simply "not anchor" rather than an error.
    pub tier: Option<String>,
    /// Tenant ids this principal can access, verbatim from the token.
    pub clients: Vec<String>,
    /// Assigned role codes.
    pub roles: Vec<String>,
    /// Explicit application ids; ignored when [`Self::all_applications`].
    pub applications: Vec<String>,
    /// Access to every application, present and future.
    pub all_applications: bool,
    /// Flattened permission codes (`*` wildcard segments allowed). A set, as
    /// in Java; the Extism envelope writes it sorted.
    pub permissions: BTreeSet<String>,
}

impl Principal {
    /// Whether a held permission satisfies `required` (Java `hasPermission`,
    /// a copy of the platform's `Permission.grants`).
    pub fn has_permission(&self, required: &str) -> bool {
        self.permissions
            .iter()
            .any(|held| permission_matches(held, required))
    }

    /// Any of `required` is held (Java `hasAnyPermission`). False for none.
    pub fn has_any_permission(&self, required: &[&str]) -> bool {
        required.iter().any(|r| self.has_permission(r))
    }

    /// Every one of `required` is held (Java `hasAllPermissions`). True for none.
    pub fn has_all_permissions(&self, required: &[&str]) -> bool {
        required.iter().all(|r| self.has_permission(r))
    }

    /// The role code is assigned, compared verbatim.
    pub fn has_role(&self, code: &str) -> bool {
        self.roles.iter().any(|r| r == code)
    }

    /// The tier is exactly `ANCHOR` (Java's copy of `AuthContext.isAnchor`).
    pub fn is_anchor(&self) -> bool {
        self.tier.as_deref() == Some("ANCHOR")
    }

    /// An anchor always; otherwise the id must be in [`Self::clients`]
    /// (Java's copy of `AuthContext.canAccessClient`).
    pub fn can_access_client(&self, client_id: &str) -> bool {
        self.is_anchor() || self.clients.iter().any(|c| c == client_id)
    }

    /// [`Self::all_applications`], or the application **id** is in
    /// [`Self::applications`] (Java's copy of `AuthContext.canAccessApplication`).
    pub fn can_access_application(&self, application_id: &str) -> bool {
        self.all_applications || self.applications.iter().any(|a| a == application_id)
    }

    /// The one client this principal is scoped to, when unambiguous: exactly
    /// one entry in [`Self::clients`] that is not the anchor wildcard `*`.
    pub fn client_id(&self) -> Option<&str> {
        match self.clients.as_slice() {
            [only] if only != "*" => Some(only),
            _ => None,
        }
    }
}

/// Whether a held permission pattern satisfies a required code: equal
/// strings, or the same number of `:`-separated segments with every held
/// segment either `*` or equal to the required one. A `*` in the required code
/// grants nothing.
///
/// Java's `Caller.Principal.matches`, itself a verbatim copy of the Java
/// platform's `Permission.matches`. It accepts any segment count, where the
/// Rust platform's `matches_pattern` only handles four; the two agree on every
/// four-segment code the platform issues (pinned by `tests/caller_agreement.rs`).
pub fn permission_matches(held: &str, required: &str) -> bool {
    if held == required {
        return true;
    }
    let mut h = held.split(':');
    let mut r = required.split(':');
    loop {
        match (h.next(), r.next()) {
            (None, None) => return true,
            (Some(hs), Some(rs)) if hs == "*" || hs == rs => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(
        tier: Option<&str>,
        clients: &[&str],
        roles: &[&str],
        applications: &[&str],
        all_applications: bool,
        permissions: &[&str],
    ) -> Principal {
        let owned = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Principal {
            id: "id".into(),
            principal_type: "user".into(),
            tier: tier.map(str::to_string),
            clients: owned(clients),
            roles: owned(roles),
            applications: owned(applications),
            all_applications,
            permissions: permissions.iter().map(|s| s.to_string()).collect(),
        }
    }

    // CallerTest.platformAndAnonymousAreSharedInstances: unit variants are the
    // shared instance by construction.
    #[test]
    fn platform_and_anonymous_are_values() {
        assert_eq!(Caller::Platform, Caller::Platform);
        assert_eq!(Caller::Anonymous, Caller::Anonymous);
        assert_ne!(Caller::Platform, Caller::Anonymous);
        assert!(Caller::Platform.principal().is_none());
    }

    // CallerTest.principalPermissionsAreIndependentOfTheSetPassedIn /
    // principalPermissionsAreUnmodifiable / principalRequiresIdAndType: owned
    // values and non-optional `String`s make these compile-time facts in Rust.

    // CallerTest P3: clientId()
    #[test]
    fn client_id_is_empty_when_no_clients() {
        assert_eq!(principal(None, &[], &[], &[], false, &[]).client_id(), None);
    }

    #[test]
    fn client_id_is_the_one_client_when_exactly_one() {
        let p = principal(Some("CLIENT"), &["clt_1"], &[], &[], false, &[]);
        assert_eq!(p.client_id(), Some("clt_1"));
    }

    #[test]
    fn client_id_is_empty_when_two_clients() {
        let p = principal(Some("PARTNER"), &["clt_1", "clt_2"], &[], &[], false, &[]);
        assert_eq!(p.client_id(), None);
    }

    #[test]
    fn client_id_is_empty_when_the_one_entry_is_the_anchor_wildcard() {
        let p = principal(Some("ANCHOR"), &["*"], &[], &[], false, &[]);
        assert_eq!(p.client_id(), None);
    }

    #[test]
    fn is_anchor_only_when_tier_is_anchor() {
        assert!(principal(Some("ANCHOR"), &[], &[], &[], false, &[]).is_anchor());
        assert!(!principal(Some("CLIENT"), &[], &[], &[], false, &[]).is_anchor());
        assert!(!principal(None, &[], &[], &[], false, &[]).is_anchor());
        assert!(!principal(Some("anchor"), &[], &[], &[], false, &[]).is_anchor());
    }

    #[test]
    fn can_access_client_is_true_for_anchor_regardless_of_clients() {
        let anchor = principal(Some("ANCHOR"), &[], &[], &[], false, &[]);
        assert!(anchor.can_access_client("anything"));
    }

    #[test]
    fn can_access_client_checks_the_list_for_non_anchors() {
        let p = principal(Some("CLIENT"), &["clt_1"], &[], &[], false, &[]);
        assert!(p.can_access_client("clt_1"));
        assert!(!p.can_access_client("clt_2"));
    }

    #[test]
    fn can_access_application_is_true_when_all_applications() {
        let p = principal(Some("CLIENT"), &[], &[], &[], true, &[]);
        assert!(p.can_access_application("anything"));
    }

    #[test]
    fn can_access_application_checks_the_list_otherwise() {
        let p = principal(Some("CLIENT"), &[], &[], &["app_1"], false, &[]);
        assert!(p.can_access_application("app_1"));
        assert!(!p.can_access_application("app_2"));
    }

    #[test]
    fn has_role_checks_the_role_list() {
        let p = principal(Some("CLIENT"), &[], &["admin"], &[], false, &[]);
        assert!(p.has_role("admin"));
        assert!(!p.has_role("viewer"));
    }

    #[test]
    fn has_any_permission_is_true_when_at_least_one_matches() {
        let p = principal(Some("CLIENT"), &[], &[], &[], false, &["a:b:c:read"]);
        assert!(p.has_any_permission(&["a:b:c:write", "a:b:c:read"]));
        assert!(!p.has_any_permission(&["a:b:c:write", "a:b:c:delete"]));
        assert!(!p.has_any_permission(&[]));
    }

    #[test]
    fn has_all_permissions_requires_every_one() {
        let p = principal(
            Some("CLIENT"),
            &[],
            &[],
            &[],
            false,
            &["a:b:c:read", "a:b:c:write"],
        );
        assert!(p.has_all_permissions(&["a:b:c:read", "a:b:c:write"]));
        assert!(!p.has_all_permissions(&["a:b:c:read", "a:b:c:delete"]));
        assert!(p.has_all_permissions(&[]));
    }

    // CallerTest.hasPermissionMatchesSegmentwise
    #[test]
    fn has_permission_matches_segmentwise() {
        let rows = [
            ("a:b:c:d", "a:b:c:d", true),
            ("a:*:c:d", "a:b:c:d", true),
            ("a:b:*:d", "a:b:c:d", true),
            ("*:*:*:*", "a:b:c:d", true),
            ("a:b:c:d", "a:b:c:e", false),
            ("a:b:c", "a:b:c:d", false),
            ("a:b:c:d:e", "a:b:c:d", false),
        ];
        for (held, required, expected) in rows {
            let p = principal(Some("CLIENT"), &[], &[], &[], false, &[held]);
            assert_eq!(
                p.has_permission(required),
                expected,
                "held={held} required={required}"
            );
        }
    }

    // CallerTest.hasPermissionIsFalseForNullRequired has no Rust equivalent:
    // `&str` cannot be null.

    // CallerTest.principalCarriesNoEmailOrName (P5): the struct literal in
    // `principal()` above names every field, so adding one (email, name, ...)
    // fails to compile here until this list is deliberately extended.
}

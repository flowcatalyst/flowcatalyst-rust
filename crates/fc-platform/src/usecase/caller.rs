//! Who a use case runs for, with what authority.
//!
//! Every [`ExecutionContext`](super::ExecutionContext) carries a [`Caller`],
//! so a use case's `authorize` can decide whether *this caller* may act on
//! *this target* (client reach, application scope, anchor-only operations,
//! ownership, role and permission ceilings) without the HTTP handler that
//! called it. There are exactly two kinds:
//!
//! - a **principal**: an authenticated request, taken whole from its
//!   [`AuthContext`] (principal id and type, tier, accessible clients,
//!   resolved permissions, credential kind), plus the application scope when
//!   the handler has resolved it;
//! - the **system**: the platform acting on its own account (startup sync,
//!   bootstrap, schedulers, login and password-reset flows that run before a
//!   principal is authenticated). It is explicit, [`Caller::system`], never
//!   a default: it reaches every client and application and holds every
//!   permission.
//!
//! The rule text of each check lives in
//! [`checks`](crate::shared::authorization_service::checks) and
//! [`caller_reach`](crate::shared::caller_reach), which take any
//! [`Authority`]: an [`AuthContext`] in a handler, a [`Caller`] in a use case.

use std::sync::OnceLock;

use crate::shared::authorization_service::{ApplicationScope, AuthContext, Authority, Credential};
use crate::{PrincipalType, UserScope};

/// The authority a use case runs with. See the module docs.
#[derive(Debug, Clone)]
pub struct Caller {
    kind: CallerKind,
    /// The application scope, when resolved (it costs a query, so a handler
    /// attaches it only where a use case checks it). `None` on a principal
    /// is "unresolved", which reaches no application: a check that needs it
    /// fails closed.
    applications: Option<ApplicationScope>,
}

#[derive(Debug, Clone)]
enum CallerKind {
    Principal(Box<AuthContext>),
    System,
}

/// How the caller authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerCredential {
    /// A principal's credential (bearer token or session cookie).
    Principal(Credential),
    /// No credential: the platform itself.
    System,
}

impl Caller {
    /// An authenticated principal, as its request's [`AuthContext`] says.
    pub fn from_auth(auth: &AuthContext) -> Caller {
        Caller {
            kind: CallerKind::Principal(Box::new(auth.clone())),
            applications: None,
        }
    }

    /// The platform acting on its own account: every client, every
    /// application, every permission. Only platform-internal paths build it.
    pub fn system() -> Caller {
        Caller {
            kind: CallerKind::System,
            applications: Some(ApplicationScope::All),
        }
    }

    /// Attach the application scope the handler resolved for this principal
    /// (`ApplicationAccessService::scope_for`). No effect on the system
    /// caller, which reaches every application.
    pub fn with_application_scope(mut self, scope: ApplicationScope) -> Caller {
        if !self.is_system() {
            self.applications = Some(scope);
        }
        self
    }

    /// Anchor tier (the system caller counts as anchor).
    pub fn is_anchor(&self) -> bool {
        Authority::is_anchor(self)
    }

    /// Whether the caller holds client `client_id` (or every client).
    pub fn can_access_client(&self, client_id: &str) -> bool {
        Authority::can_access_client(self, client_id)
    }

    /// Whether the caller holds `permission` (the system caller holds all).
    pub fn has_permission(&self, permission: &str) -> bool {
        Authority::has_permission(self, permission)
    }

    /// Whether this is the platform itself rather than a principal.
    pub fn is_system(&self) -> bool {
        matches!(self.kind, CallerKind::System)
    }

    /// The authenticated principal's context; `None` for the system caller.
    /// Helpers that take `Option<&AuthContext>` (the role ceiling, signing
    /// reach) read `None` as "no principal to bound".
    pub fn auth(&self) -> Option<&AuthContext> {
        match &self.kind {
            CallerKind::Principal(auth) => Some(auth),
            CallerKind::System => None,
        }
    }

    /// The acting principal's id; `None` for the system caller.
    pub fn principal_id(&self) -> Option<&str> {
        self.auth().map(|a| a.principal_id.as_str())
    }

    /// The acting principal's type; `None` for the system caller.
    pub fn principal_type(&self) -> Option<PrincipalType> {
        self.auth().map(|a| a.principal_type)
    }

    /// The caller's tier; the system caller is anchor.
    pub fn scope(&self) -> UserScope {
        self.auth().map_or(UserScope::Anchor, |a| a.scope)
    }

    /// How the caller authenticated.
    pub fn credential(&self) -> CallerCredential {
        match &self.kind {
            CallerKind::Principal(auth) => CallerCredential::Principal(auth.credential),
            CallerKind::System => CallerCredential::System,
        }
    }

    /// Whether the caller reaches application `application_id`: the system
    /// always, a principal as its resolved scope says (an unresolved scope
    /// reaches none).
    pub fn allows_application(&self, application_id: &str) -> bool {
        self.applications
            .as_ref()
            .is_some_and(|scope| scope.allows(application_id))
    }

    /// The resolved application scope, if any (always `All` for the system
    /// caller).
    pub fn application_scope(&self) -> Option<&ApplicationScope> {
        self.applications.as_ref()
    }
}

/// The system caller's client list: every client.
fn all_clients() -> &'static [String] {
    static ALL: OnceLock<Vec<String>> = OnceLock::new();
    ALL.get_or_init(|| vec!["*".to_string()])
}

impl Authority for Caller {
    fn is_anchor(&self) -> bool {
        self.auth().is_none_or(|a| a.is_anchor())
    }

    fn can_access_client(&self, client_id: &str) -> bool {
        self.auth().is_none_or(|a| a.can_access_client(client_id))
    }

    fn has_permission(&self, permission: &str) -> bool {
        self.auth().is_none_or(|a| a.has_permission(permission))
    }

    fn accessible_clients(&self) -> &[String] {
        match &self.kind {
            CallerKind::Principal(auth) => &auth.accessible_clients,
            CallerKind::System => all_clients(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::authorization_service::checks;
    use std::collections::HashSet;

    fn client_user() -> AuthContext {
        AuthContext {
            principal_id: "prn_1".into(),
            principal_type: PrincipalType::User,
            scope: UserScope::Client,
            email: None,
            name: "Test".into(),
            accessible_clients: vec!["clt_a".into()],
            permissions: HashSet::from(["platform:iam:user:view".to_string()]),
            roles: vec![],
            credential: Credential::SessionCookie,
        }
    }

    #[test]
    fn a_principal_caller_is_its_auth_context() {
        let caller = Caller::from_auth(&client_user());
        assert!(!caller.is_system());
        assert_eq!(caller.principal_id(), Some("prn_1"));
        assert_eq!(caller.principal_type(), Some(PrincipalType::User));
        assert_eq!(caller.scope(), UserScope::Client);
        assert_eq!(
            caller.credential(),
            CallerCredential::Principal(Credential::SessionCookie)
        );
        assert!(!Authority::is_anchor(&caller));
        assert!(Authority::can_access_client(&caller, "clt_a"));
        assert!(!Authority::can_access_client(&caller, "clt_b"));
        assert!(Authority::has_permission(&caller, "platform:iam:user:view"));
        assert!(!Authority::has_permission(
            &caller,
            "platform:iam:user:create"
        ));
        // The same rule answers the same way for either form.
        assert_eq!(
            format!("{:?}", checks::can_read_principals(&caller)),
            format!("{:?}", checks::can_read_principals(&client_user()))
        );
        assert!(checks::require_anchor_scope(&caller).is_err());
    }

    #[test]
    fn an_unresolved_application_scope_reaches_nothing() {
        let caller = Caller::from_auth(&client_user());
        assert!(caller.application_scope().is_none());
        assert!(!caller.allows_application("app_1"));
        let scoped = caller
            .with_application_scope(ApplicationScope::Only(HashSet::from(["app_1".to_string()])));
        assert!(scoped.allows_application("app_1"));
        assert!(!scoped.allows_application("app_2"));
    }

    #[test]
    fn the_system_caller_reaches_everything() {
        let system = Caller::system();
        assert!(system.is_system());
        assert!(system.auth().is_none());
        assert_eq!(system.principal_id(), None);
        assert_eq!(system.scope(), UserScope::Anchor);
        assert_eq!(system.credential(), CallerCredential::System);
        assert!(Authority::is_anchor(&system));
        assert!(Authority::can_access_client(&system, "clt_any"));
        assert!(Authority::has_permission(
            &system,
            "platform:iam:user:create"
        ));
        assert!(system.allows_application("app_any"));
        assert_eq!(system.accessible_clients(), ["*".to_string()]);
        assert!(checks::require_anchor_scope(&system).is_ok());
        assert!(checks::can_update_clients(&system).is_ok());
        // An application scope can't narrow it.
        let still = system.with_application_scope(ApplicationScope::Only(HashSet::new()));
        assert!(still.allows_application("app_any"));
    }
}

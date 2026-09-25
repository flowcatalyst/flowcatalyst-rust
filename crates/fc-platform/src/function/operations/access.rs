//! Who may reach what (Java `function/operations/Access.java` and the
//! `Checks.canAccessScope` / `checkScopeAccess` / `checkApplicationAccess`
//! it builds on).
//!
//! A function is reachable when the caller reaches its owner (the platform:
//! anchor scope; a client: access to that client) **and** its owning
//! application. A domain is reachable when the caller reaches its owner.
//! Anything that exists but is out of reach answers exactly as if it did not
//! exist, `404 <Resource>_NOT_FOUND`, never a 403: a 403 would confirm the
//! address. The read handlers and the write use cases share
//! [`Caller::can_reach`], so the two can never disagree.

use crate::function::domain_repository::FunctionDomainRepository;
use crate::function::entity::{Function, FunctionDomain};
use crate::function::repository::{FunctionRepository, OwnerReach};
use crate::function::{FunctionAddress, FunctionOwner, Hostname};
use crate::shared::authorization_service::{ApplicationScope, AuthContext};
use crate::usecase::UseCaseError;

/// The authenticated caller, with the application scope its principal
/// resolves to (Java's `AuthContext` carries both).
#[derive(Debug, Clone)]
pub struct Caller {
    pub auth: AuthContext,
    pub applications: ApplicationScope,
}

impl Caller {
    pub fn new(auth: AuthContext, applications: ApplicationScope) -> Caller {
        Caller { auth, applications }
    }

    /// Java `Checks.canAccessScope`: a client id needs access to that
    /// client; none (the platform) needs anchor scope.
    pub fn can_access_scope(&self, client_id: Option<&str>) -> bool {
        match client_id {
            Some(id) => self.auth.can_access_client(id),
            None => self.auth.is_anchor(),
        }
    }

    fn can_reach_owner(&self, owner: &FunctionOwner) -> bool {
        self.can_access_scope(owner.client_id_or_none())
    }

    /// Java `Access.canReach`: the owner, and the owning application.
    pub fn can_reach(&self, f: &Function) -> bool {
        self.can_reach_owner(&f.owner) && self.applications.allows(&f.application_id)
    }

    /// Java `Access.canReachDomain`: the owner alone (a domain has no
    /// application).
    pub fn can_reach_domain(&self, d: &FunctionDomain) -> bool {
        self.can_reach_owner(&d.owner)
    }

    /// Java `Checks.checkScopeAccess`, for a create: nothing exists yet to
    /// hide, so this is a real 403 `SCOPE_FORBIDDEN`.
    pub fn check_scope_access(&self, client_id: Option<&str>) -> Result<(), UseCaseError> {
        if self.can_access_scope(client_id) {
            return Ok(());
        }
        Err(UseCaseError::forbidden(
            "SCOPE_FORBIDDEN",
            if client_id.is_some() {
                "no access to this resource's client"
            } else {
                "anchor scope required for this resource"
            },
        ))
    }

    /// Java `Checks.checkApplicationAccess`: 403 `FORBIDDEN`
    /// `Not authorised for application '<code>'`.
    pub fn check_application_access(
        &self,
        application_id: &str,
        application_code: &str,
    ) -> Result<(), UseCaseError> {
        if self.applications.allows(application_id) {
            return Ok(());
        }
        Err(UseCaseError::forbidden(
            "FORBIDDEN",
            format!("Not authorised for application '{application_code}'"),
        ))
    }

    /// The owner half of the list route's reach, applied in SQL.
    pub fn owner_reach(&self) -> OwnerReach {
        if self.auth.is_anchor() {
            OwnerReach::Everything
        } else if self.auth.accessible_clients.iter().any(|c| c == "*") {
            OwnerReach::AnyClient
        } else {
            OwnerReach::Clients(self.auth.accessible_clients.clone())
        }
    }

    /// The application half of the list route's reach: `None` for every
    /// application, else exactly the granted ones (none when empty).
    pub fn application_reach(&self) -> Option<Vec<String>> {
        match &self.applications {
            ApplicationScope::All => None,
            ApplicationScope::Only(ids) => {
                let mut ids: Vec<String> = ids.iter().cloned().collect();
                ids.sort();
                Some(ids)
            }
        }
    }
}

/// Java's canonical not-found (`UseCaseException.resourceNotFound`):
/// `<Resource> not found: <id>`, with the code in UPPER_SNAKE
/// (`FUNCTION_VERSION_NOT_FOUND`; owner decision 5). Java appends
/// `_NOT_FOUND` to the name as given.
pub fn resource_not_found(resource: &str, id: &str) -> UseCaseError {
    UseCaseError::not_found(
        crate::shared::error::not_found_code(resource),
        format!("{resource} not found: {id}"),
    )
}

/// Load-or-404, out-of-reach-or-404: `FUNCTION_NOT_FOUND` either way.
pub async fn function_by_address(
    functions: &FunctionRepository,
    address: &FunctionAddress,
    caller: &Caller,
) -> Result<Function, UseCaseError> {
    match functions.find_by_address(address).await? {
        Some(f) if caller.can_reach(&f) => Ok(f),
        _ => Err(resource_not_found("Function", &address.render())),
    }
}

/// The claim covering `hostname` (the hostname itself or its zone), or
/// `FUNCTION_DOMAIN_NOT_FOUND` when there is none or it is out of reach.
pub async fn domain_by_hostname(
    domains: &FunctionDomainRepository,
    hostname: &Hostname,
    caller: &Caller,
) -> Result<FunctionDomain, UseCaseError> {
    match domains.covering(hostname).await? {
        Some(d) if caller.can_reach_domain(&d) => Ok(d),
        _ => Err(resource_not_found("FunctionDomain", hostname.value())),
    }
}

/// Java `AccessTest`.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::function::Runtime;
    use crate::{PrincipalType, UserScope};
    use std::collections::HashSet;

    pub(crate) fn caller(scope: UserScope, clients: &[&str], apps: Option<&[&str]>) -> Caller {
        Caller::new(
            AuthContext {
                principal_id: "prn_1".into(),
                principal_type: PrincipalType::User,
                scope,
                email: None,
                name: "Test".into(),
                accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
                permissions: HashSet::new(),
                roles: vec![],
                credential: crate::shared::authorization_service::Credential::BearerToken,
            },
            match apps {
                None => ApplicationScope::All,
                Some(ids) => ApplicationScope::Only(ids.iter().map(|a| a.to_string()).collect()),
            },
        )
    }

    fn function(owner: FunctionOwner) -> Function {
        Function::create(
            "app_1",
            FunctionAddress::parse("a.b.c").unwrap(),
            owner,
            Runtime::Wasm,
            None,
        )
    }

    #[test]
    fn a_platform_function_needs_anchor_scope() {
        let f = function(FunctionOwner::Platform);
        assert!(caller(UserScope::Anchor, &["*"], None).can_reach(&f));
        assert!(!caller(UserScope::Client, &["clt_1"], None).can_reach(&f));
        assert!(!caller(UserScope::Partner, &["clt_1", "clt_2"], None).can_reach(&f));
    }

    #[test]
    fn a_client_function_needs_that_client() {
        let f = function(FunctionOwner::Client("clt_1".into()));
        assert!(caller(UserScope::Client, &["clt_1"], None).can_reach(&f));
        assert!(caller(UserScope::Anchor, &["*"], None).can_reach(&f));
        assert!(!caller(UserScope::Client, &["clt_2"], None).can_reach(&f));
    }

    #[test]
    fn the_owning_application_must_be_reached_too() {
        let f = function(FunctionOwner::Client("clt_1".into()));
        assert!(caller(UserScope::Client, &["clt_1"], Some(&["app_1"])).can_reach(&f));
        assert!(!caller(UserScope::Client, &["clt_1"], Some(&["app_2"])).can_reach(&f));
        assert!(!caller(UserScope::Anchor, &["*"], Some(&[])).can_reach(&f));
    }

    #[test]
    fn a_domain_needs_only_its_owner() {
        let d = FunctionDomain::claim(
            FunctionOwner::Client("clt_1".into()),
            Hostname::parse("acme.com").unwrap(),
            chrono::Utc::now(),
        );
        assert!(caller(UserScope::Client, &["clt_1"], Some(&[])).can_reach_domain(&d));
        assert!(!caller(UserScope::Client, &["clt_2"], None).can_reach_domain(&d));
    }

    #[test]
    fn scope_and_application_checks_are_403_with_javas_codes() {
        let client = caller(UserScope::Client, &["clt_1"], Some(&["app_1"]));
        assert!(client.check_scope_access(Some("clt_1")).is_ok());
        let err = client.check_scope_access(Some("clt_2")).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code()),
            (403, "SCOPE_FORBIDDEN")
        );
        assert_eq!(err.message(), "no access to this resource's client");
        let err = client.check_scope_access(None).unwrap_err();
        assert_eq!(err.message(), "anchor scope required for this resource");
        let err = client
            .check_application_access("app_2", "billing")
            .unwrap_err();
        assert_eq!((err.http_status_code(), err.code()), (403, "FORBIDDEN"));
        assert_eq!(err.message(), "Not authorised for application 'billing'");
    }

    #[test]
    fn list_reach() {
        assert_eq!(
            caller(UserScope::Anchor, &["*"], None).owner_reach(),
            OwnerReach::Everything
        );
        assert_eq!(
            caller(UserScope::Partner, &["clt_1", "clt_2"], None).owner_reach(),
            OwnerReach::Clients(vec!["clt_1".into(), "clt_2".into()])
        );
        assert_eq!(
            caller(UserScope::Client, &["clt_1"], Some(&["b", "a"])).application_reach(),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            caller(UserScope::Client, &["clt_1"], None).application_reach(),
            None
        );
    }

    #[test]
    fn not_found_has_javas_shape() {
        let err = resource_not_found("Function", "a.b.c");
        assert_eq!(err.code(), "FUNCTION_NOT_FOUND");
        assert_eq!(err.message(), "Function not found: a.b.c");
        assert_eq!(err.http_status_code(), 404);
    }
}

//! Who may cause a delivery to go out under a service account's webhook
//! credentials (Java `serviceaccount/SigningReach.java`, security fixes
//! b1ce6e55, eac7ef57, b8005931; `docs/parity/java-2026-09-25-triage.md` S5,
//! S7).
//!
//! A caller that names the account a subscription or connection signs with,
//! or ingests a job or event some account will sign, chooses where that
//! account's bearer token and signature go. So it must already hold
//! everything the account can reach. The rule, in order:
//!
//! 1. a super-admin (`platform:*:*:*`) may use any account;
//! 2. an account that belongs to an application may be used by that
//!    application itself (the caller is one of its service accounts) and by
//!    nobody else. Not even on a subscription that application synced: its
//!    endpoint is editable by whoever administers the subscription's client;
//! 3. a caller that *is* an application may use no other account at all:
//!    application accounts are anchor-tier, so tenancy alone would let one
//!    application borrow any tenant's or the operator's account;
//! 4. otherwise the caller's tenancy must cover the account's: an account
//!    that reaches no client is anchor-tier, covered only by an anchor; one
//!    that reaches clients is covered only when the caller reaches **every**
//!    one of them.
//!
//! The application a caller *is* is found through its principal: a SERVICE
//! principal's linked account's `application_id`. A stored account reference
//! may be the account's own id or its principal's (Java b8005931:
//! `app_applications.service_account_id` holds the principal id and synced
//! connections copy it); the lookup resolves both, exactly as the delivery
//! signer does, so the check never reads as dangling a reference the signer
//! signs with.

use std::fmt;

use crate::connection::repository::ConnectionRepository;
use crate::permissions;
use crate::service_account::repository::ServiceAccountRepository;
use crate::shared::authorization_service::AuthContext;
use crate::shared::caller_reach;
use crate::shared::error::Result;
use crate::usecase::UseCaseError;

/// Which clients a service account reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountReach {
    /// No client: an anchor-tier account.
    Anchor,
    /// These clients (never empty).
    Clients(Vec<String>),
}

impl AccountReach {
    /// An empty client list is no client: anchor-tier.
    pub fn of_clients(clients: Vec<String>) -> AccountReach {
        if clients.is_empty() {
            AccountReach::Anchor
        } else {
            AccountReach::Clients(clients)
        }
    }
}

/// A service account as the reach check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningAccount {
    pub code: String,
    /// The application the account belongs to, if any.
    pub application_id: Option<String>,
    pub reach: AccountReach,
}

/// Why a caller may not use a signing identity. [`fmt::Display`] is the text
/// the 403 carries (Java's `Refusal.message()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachRefusal {
    /// The account reaches clients the caller does not.
    OutOfReach { account_code: String },
    /// The account belongs to an application the caller is not.
    OtherApplication { account_code: String },
    /// The caller is an application, and the account is not one of its own.
    ApplicationCaller { account_code: String },
    /// An application's own account would sign, and the caller is not that
    /// application.
    NotTheApplication { application_code: String },
}

impl fmt::Display for ReachRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReachRefusal::OutOfReach { account_code } => write!(
                f,
                "service account {account_code} reaches clients the caller cannot access"
            ),
            ReachRefusal::OtherApplication { account_code } => write!(
                f,
                "service account {account_code} belongs to an application the caller is not"
            ),
            ReachRefusal::ApplicationCaller { account_code } => write!(
                f,
                "service account {account_code} is not the calling application's own; \
                 an application signs only with its own accounts"
            ),
            ReachRefusal::NotTheApplication { application_code } => write!(
                f,
                "application {application_code} signs only its own jobs; the caller is not that application"
            ),
        }
    }
}

/// One caller's signing reach, for one request: the caller and the
/// application it acts as (looked up once).
#[derive(Debug, Clone)]
pub struct SigningReach {
    caller: AuthContext,
    caller_application_id: Option<String>,
}

impl SigningReach {
    /// `caller` acting as application `caller_application_id` (`None`: a
    /// user, or an account of no application).
    pub fn new(caller: AuthContext, caller_application_id: Option<String>) -> SigningReach {
        SigningReach {
            caller,
            caller_application_id,
        }
    }

    /// The caller's reach, reading the application it acts as (one query;
    /// none for a super-admin, who needs no answer).
    pub async fn for_caller(
        caller: &AuthContext,
        accounts: &ServiceAccountRepository,
    ) -> Result<SigningReach> {
        let application_id = if is_super_admin(caller) {
            None
        } else {
            accounts.caller_application_id(&caller.principal_id).await?
        };
        Ok(SigningReach::new(caller.clone(), application_id))
    }

    pub fn caller(&self) -> &AuthContext {
        &self.caller
    }

    pub fn is_super_admin(&self) -> bool {
        is_super_admin(&self.caller)
    }

    /// Whether the caller may send deliveries signed by `account` (Java
    /// `SigningReach.mayUse`).
    pub fn may_use(&self, account: &SigningAccount) -> std::result::Result<(), ReachRefusal> {
        if self.is_super_admin() {
            return Ok(());
        }
        let caller_application = self.caller_application_id.as_deref();
        if let Some(owner) = account.application_id.as_deref() {
            return if Some(owner) == caller_application {
                Ok(())
            } else {
                Err(ReachRefusal::OtherApplication {
                    account_code: account.code.clone(),
                })
            };
        }
        if caller_application.is_some() {
            return Err(ReachRefusal::ApplicationCaller {
                account_code: account.code.clone(),
            });
        }
        let covered = match &account.reach {
            AccountReach::Anchor => self.caller.is_anchor(),
            AccountReach::Clients(clients) => clients
                .iter()
                .all(|c| caller_reach::reaches_client(&self.caller, c)),
        };
        if covered {
            Ok(())
        } else {
            Err(ReachRefusal::OutOfReach {
                account_code: account.code.clone(),
            })
        }
    }

    /// Whether the caller may send deliveries signed by application
    /// `application_id`'s own account (the delivery signer's last step):
    /// only that application itself, or a super-admin (Java
    /// `SigningReach.mayUseApplication`).
    pub fn may_use_application(
        &self,
        application_id: &str,
        application_code: &str,
    ) -> std::result::Result<(), ReachRefusal> {
        if self.is_super_admin() || self.caller_application_id.as_deref() == Some(application_id) {
            Ok(())
        } else {
            Err(ReachRefusal::NotTheApplication {
                application_code: application_code.to_string(),
            })
        }
    }
}

/// A super-admin holds `platform:*:*:*` (Java `AuthContext.isSuperAdmin`).
pub fn is_super_admin(caller: &AuthContext) -> bool {
    caller.has_permission(permissions::ADMIN_ALL)
}

/// The account a subscription's (or connection's) deliveries will be signed
/// with, named directly or through its connection, must be one the caller
/// may use (Java `subscription/operations/Access.requireUsableSigners`,
/// S3.1): the caller chooses the endpoint, so naming an account is choosing
/// where its bearer token and signatures go.
///
/// `service_account_id` / `connection_id` are the resulting references (blank
/// is none). A reference the caller is setting (`*_must_exist`) must name an
/// existing row (404 `SERVICE_ACCOUNT_NOT_FOUND` / `CONNECTION_NOT_FOUND`);
/// one carried over unchanged may be dangling (it signs nothing) and is then
/// not refused. A connection outside the caller's scope is 403
/// `CONNECTION_OUT_OF_REACH`; an account the caller may not use is 403
/// `SERVICE_ACCOUNT_OUT_OF_REACH`. Nothing to check needs no caller; anything
/// to check without one is 403 `UNAUTHENTICATED`.
pub async fn require_usable_signers(
    caller: Option<&AuthContext>,
    accounts: &ServiceAccountRepository,
    connections: &ConnectionRepository,
    service_account_id: Option<&str>,
    account_must_exist: bool,
    connection_id: Option<&str>,
    connection_must_exist: bool,
) -> std::result::Result<(), UseCaseError> {
    let service_account_id = service_account_id.filter(|v| !v.trim().is_empty());
    let connection_id = connection_id.filter(|v| !v.trim().is_empty());
    if service_account_id.is_none() && connection_id.is_none() {
        return Ok(());
    }
    let Some(caller) = caller else {
        return Err(UseCaseError::forbidden(
            "UNAUTHENTICATED",
            "authentication required",
        ));
    };
    let reach = SigningReach::for_caller(caller, accounts).await?;

    // The accounts to check: the one named, and the connection's.
    let mut to_check: Vec<(String, bool)> = Vec::new();
    if let Some(id) = service_account_id {
        to_check.push((id.to_string(), account_must_exist));
    }
    if let Some(id) = connection_id {
        match connections.find_by_id(id).await? {
            None if connection_must_exist => {
                return Err(UseCaseError::not_found(
                    "CONNECTION_NOT_FOUND",
                    format!("Connection not found: {id}"),
                ))
            }
            None => {}
            Some(connection) => {
                if !caller_reach::reaches_scope(caller, connection.client_id.as_deref()) {
                    return Err(UseCaseError::forbidden(
                        "CONNECTION_OUT_OF_REACH",
                        format!(
                            "connection {} belongs to a scope the caller cannot access",
                            connection.code
                        ),
                    ));
                }
                if !connection.service_account_id.trim().is_empty() {
                    to_check.push((connection.service_account_id, false));
                }
            }
        }
    }
    let references: Vec<String> = to_check.iter().map(|(id, _)| id.clone()).collect();
    let found = accounts.find_signing_accounts(&references).await?;
    for (id, must_exist) in &to_check {
        match found.get(id) {
            None if *must_exist => {
                return Err(UseCaseError::not_found(
                    "SERVICE_ACCOUNT_NOT_FOUND",
                    format!("ServiceAccount not found: {id}"),
                ))
            }
            None => {}
            Some(account) => {
                if let Err(refusal) = reach.may_use(account) {
                    return Err(UseCaseError::forbidden(
                        "SERVICE_ACCOUNT_OUT_OF_REACH",
                        refusal.to_string(),
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{PrincipalType, UserScope};
    use std::collections::HashSet;

    pub(crate) fn caller(scope: UserScope, clients: &[&str], perms: &[&str]) -> AuthContext {
        AuthContext {
            principal_id: "prn_caller".into(),
            principal_type: PrincipalType::Service,
            scope,
            email: None,
            name: "caller".into(),
            accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
            permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
            roles: vec![],
            credential: crate::shared::authorization_service::Credential::BearerToken,
        }
    }

    fn account(application_id: Option<&str>, reach: AccountReach) -> SigningAccount {
        SigningAccount {
            code: "acct".into(),
            application_id: application_id.map(str::to_string),
            reach,
        }
    }

    fn clients(ids: &[&str]) -> AccountReach {
        AccountReach::of_clients(ids.iter().map(|c| c.to_string()).collect())
    }

    #[test]
    fn a_super_admin_uses_any_account_and_application() {
        let r = SigningReach::new(
            caller(UserScope::Client, &["clt_a"], &[permissions::ADMIN_ALL]),
            None,
        );
        assert_eq!(
            r.may_use(&account(Some("app_x"), AccountReach::Anchor)),
            Ok(())
        );
        assert_eq!(r.may_use(&account(None, clients(&["clt_z"]))), Ok(()));
        assert_eq!(r.may_use_application("app_x", "x"), Ok(()));
    }

    #[test]
    fn an_applications_account_is_that_applications_alone() {
        let own = SigningReach::new(caller(UserScope::Anchor, &["*"], &[]), Some("app_x".into()));
        assert_eq!(
            own.may_use(&account(Some("app_x"), AccountReach::Anchor)),
            Ok(())
        );
        assert_eq!(own.may_use_application("app_x", "x"), Ok(()));
        assert!(matches!(
            own.may_use(&account(Some("app_y"), AccountReach::Anchor)),
            Err(ReachRefusal::OtherApplication { .. })
        ));
        assert_eq!(
            own.may_use_application("app_y", "y"),
            Err(ReachRefusal::NotTheApplication {
                application_code: "y".into()
            })
        );
        // An application may not borrow an account of no application, even
        // one its anchor tier would cover.
        assert!(matches!(
            own.may_use(&account(None, AccountReach::Anchor)),
            Err(ReachRefusal::ApplicationCaller { .. })
        ));

        // A plain anchor (no application, not super-admin) may use neither.
        let anchor = SigningReach::new(caller(UserScope::Anchor, &["*"], &[]), None);
        assert!(anchor
            .may_use(&account(Some("app_x"), AccountReach::Anchor))
            .is_err());
        assert!(anchor.may_use_application("app_x", "x").is_err());
    }

    #[test]
    fn tenancy_must_cover_every_client_the_account_reaches() {
        let a = SigningReach::new(caller(UserScope::Client, &["clt_a"], &[]), None);
        assert_eq!(a.may_use(&account(None, clients(&["clt_a"]))), Ok(()));
        assert_eq!(
            a.may_use(&account(None, clients(&["clt_a", "clt_b"]))),
            Err(ReachRefusal::OutOfReach {
                account_code: "acct".into()
            })
        );
        assert!(a.may_use(&account(None, AccountReach::Anchor)).is_err());
        assert!(a
            .may_use(&account(None, AccountReach::of_clients(vec![])))
            .is_err());

        let anchor = SigningReach::new(caller(UserScope::Anchor, &["*"], &[]), None);
        assert_eq!(anchor.may_use(&account(None, AccountReach::Anchor)), Ok(()));
        assert_eq!(anchor.may_use(&account(None, clients(&["clt_z"]))), Ok(()));
    }

    #[test]
    fn refusals_say_why() {
        assert_eq!(
            ReachRefusal::NotTheApplication {
                application_code: "billing".into()
            }
            .to_string(),
            "application billing signs only its own jobs; the caller is not that application"
        );
    }
}

//! The lookups messaging, the scheduled jobs and the functions make into
//! IAM, as traits.
//!
//! Messaging (fc-platform-messaging), the scheduled jobs
//! (fc-platform-scheduled-jobs) and the functions (fc-platform-functions)
//! read a few IAM facts: a client's name or
//! identifier, an application's id or code, which service accounts a caller
//! may sign with, an application's outbound credentials, a principal's
//! application scope. They read them through these traits, which
//! fc-platform-iam implements on its repositories and services, and the
//! assembly (`fc-platform`) injects as `Arc<dyn …>`. So an IAM edit does
//! not rebuild messaging, the scheduled jobs or the functions (docs/plans/build-speed-2026-09-28.md,
//! section 6.3, step 2).
//!
//! Each method is the repository method of the same name, and returns the
//! fields its callers read; the implementations run the same queries.

use crate::shared::id::ApplicationId;
use std::collections::HashMap;
use std::fmt;

use async_trait::async_trait;

use crate::shared::authorization_service::{
    ApplicationScope, AuthContext, PrincipalApplicationBinding,
};
use crate::shared::error::Result;

// ── Clients ──────────────────────────────────────────────────────────────

/// A client, as the other domains look it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRef {
    pub id: String,
    pub name: String,
    pub identifier: String,
    /// Its status is ACTIVE.
    pub active: bool,
}

/// `ClientRepository`'s lookups (fc-platform-iam).
#[async_trait]
pub trait ClientDirectory: Send + Sync {
    async fn find_by_id(&self, id: &str) -> Result<Option<ClientRef>>;
    async fn find_by_ids(&self, ids: &[String]) -> Result<Vec<ClientRef>>;
    /// Client ids by identifier, for the identifiers that exist.
    async fn find_ids_by_identifiers(
        &self,
        identifiers: &[String],
    ) -> Result<HashMap<String, String>>;
    async fn find_all(&self) -> Result<Vec<ClientRef>>;
}

// ── Applications ─────────────────────────────────────────────────────────

/// An application, as the other domains look it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationRef {
    pub id: ApplicationId,
    pub code: String,
    pub name: String,
    pub active: bool,
    /// The service account attached to it (an account id or its principal's).
    pub service_account_id: Option<String>,
}

/// `ApplicationRepository`'s lookups (fc-platform-iam).
#[async_trait]
pub trait ApplicationDirectory: Send + Sync {
    async fn find_by_id(&self, id: &ApplicationId) -> Result<Option<ApplicationRef>>;
    async fn find_by_code(&self, code: &str) -> Result<Option<ApplicationRef>>;
    /// Application ids by code, for the codes that exist.
    async fn find_ids_by_codes(&self, codes: &[String]) -> Result<HashMap<String, ApplicationId>>;
    async fn find_all(&self) -> Result<Vec<ApplicationRef>>;
}

/// `ApplicationAccessService` (fc-platform-iam): an application the caller
/// may act on, by code.
#[async_trait]
pub trait ApplicationAccess: Send + Sync {
    /// The application `app_code` names, when the caller's application
    /// scope covers it; otherwise the same 404 as a missing application.
    async fn require_application_access(
        &self,
        context: &AuthContext,
        app_code: &str,
    ) -> Result<ApplicationRef>;

    /// A principal's application scope (cached).
    async fn scope_for(&self, principal_id: &str) -> Result<ApplicationScope>;
}

// ── Service accounts ─────────────────────────────────────────────────────

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

/// A service account as the signing-reach check sees it
/// (`service_account::signing_reach`, fc-platform-messaging).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningAccount {
    pub code: String,
    /// The application the account belongs to, if any.
    pub application_id: Option<ApplicationId>,
    pub reach: AccountReach,
}

/// A service account, as the other domains look it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAccountRef {
    pub id: String,
    pub code: String,
}

/// `ServiceAccountRepository`'s lookups (fc-platform-iam): an account by id,
/// and what the signing-reach check reads.
#[async_trait]
pub trait ServiceAccountDirectory: Send + Sync {
    async fn find_by_id(&self, id: &str) -> Result<Option<ServiceAccountRef>>;
    /// The application a caller *is*: its SERVICE principal's linked
    /// account's `application_id`.
    async fn caller_application_id(&self, principal_id: &str) -> Result<Option<ApplicationId>>;
    /// The accounts the references name (an account id or its principal's),
    /// by reference.
    async fn find_signing_accounts(
        &self,
        references: &[String],
    ) -> Result<HashMap<String, SigningAccount>>;
    /// Whether the application's oldest active account has a signing secret.
    async fn oldest_active_has_signing_secret(
        &self,
        application_id: &ApplicationId,
    ) -> Result<bool>;
}

// ── Principals ───────────────────────────────────────────────────────────

/// `PrincipalRepository`'s lookups (fc-platform-iam).
#[async_trait]
pub trait PrincipalDirectory: Send + Sync {
    /// A principal's application-access facts; `None` when it doesn't exist.
    async fn find_application_binding(
        &self,
        principal_id: &str,
    ) -> Result<Option<PrincipalApplicationBinding>>;
}

// ── Outbound credentials ─────────────────────────────────────────────────

/// Opened credentials. Either may be absent; `signed_by` is the account's
/// code. Never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct OutboundCredentials {
    pub token: Option<String>,
    pub signing_secret: Option<String>,
    pub signed_by: String,
}

impl OutboundCredentials {
    /// Neither credential is set.
    pub fn is_empty(&self) -> bool {
        self.token.is_none() && self.signing_secret.is_none()
    }
}

impl fmt::Debug for OutboundCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mask = |v: &Option<String>| if v.is_some() { "<redacted>" } else { "null" };
        f.debug_struct("OutboundCredentials")
            .field("token", &mask(&self.token))
            .field("signing_secret", &mask(&self.signing_secret))
            .field("signed_by", &self.signed_by)
            .finish()
    }
}

/// A named account's outcome (Java `OutboundCredentials.ById`): every reason
/// a named account cannot sign is kept apart, never collapsed into "not
/// found". An inactive account is a decline, not a fallback signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ById {
    Found(OutboundCredentials),
    Missing,
    Inactive(String),
    NoCredentials(String),
}

/// `OutboundCredentialsResolver` (fc-platform-iam): the credentials a
/// delivery or a scheduled-job firing is sent with.
#[async_trait]
pub trait OutboundCredentialSource: Send + Sync {
    /// The application's oldest active account's credentials (cached).
    async fn for_application(
        &self,
        application_id: &ApplicationId,
    ) -> Result<Option<OutboundCredentials>>;
    /// A named account's (cached).
    async fn by_service_account_id(&self, id: &str) -> Result<ById>;
    /// Several applications' credentials, read now (not cached).
    async fn for_applications_fresh(
        &self,
        application_ids: &[ApplicationId],
    ) -> Result<HashMap<ApplicationId, OutboundCredentials>>;
}

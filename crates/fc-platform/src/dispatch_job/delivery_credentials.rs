//! Whose credentials a dispatch job is delivered with (Java
//! `dispatchjob/processing/DeliveryCredentials.java`,
//! `docs/spec/dispatch-delivery-credentials.md` §2), in Java's order:
//!
//! 1. the job's subscription names its own service account;
//! 2. else that subscription's connection names one;
//! 3. else the application's oldest active account, the application being the
//!    subscription's `applicationCode` or, failing that, the job code's
//!    segment before its first `:`.
//!
//! A named account that is missing, inactive or has no credentials declines
//! with its own reason; it never falls through to step 3. No application
//! (or none by that code, or no active account) is a bare delivery, which
//! is the normal state of a platform-scoped job, not a failure.
//!
//! A function's subscriptions name no account and no connection and carry
//! the function's application code, so their deliveries are signed with the
//! application's secret: the one the function host verifies against
//! (`function-invocation.md` §6).

use std::fmt;
use std::sync::Arc;

use tracing::warn;

use super::entity::DispatchJob;
use crate::connection::entity::Connection;
use crate::connection::repository::ConnectionRepository;
use crate::service_account::outbound_credentials::{ById, OutboundCredentialsResolver};
use crate::shared::error::Result;
use crate::{ApplicationRepository, Subscription, SubscriptionRepository};

/// What a delivery carries (Java `DeliveryCredentials.Resolved`). `reason`
/// says why a bare delivery is bare; `signed_by` names the account. Never
/// printed with its credentials.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub bearer_token: Option<String>,
    pub signing_secret: Option<String>,
    pub reason: String,
    pub signed_by: Option<String>,
}

impl Resolved {
    fn bare(reason: impl Into<String>) -> Resolved {
        Resolved {
            reason: reason.into(),
            ..Resolved::default()
        }
    }

    fn signed(token: Option<String>, secret: Option<String>, signed_by: String) -> Resolved {
        Resolved {
            bearer_token: token.filter(|t| !t.is_empty()),
            signing_secret: secret.filter(|s| !s.is_empty()),
            reason: String::new(),
            signed_by: Some(signed_by),
        }
    }

    /// Neither credential.
    pub fn is_bare(&self) -> bool {
        self.bearer_token.is_none() && self.signing_secret.is_none()
    }
}

impl fmt::Debug for Resolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mask = |v: &Option<String>| if v.is_some() { "<redacted>" } else { "null" };
        f.debug_struct("Resolved")
            .field("bearer_token", &mask(&self.bearer_token))
            .field("signing_secret", &mask(&self.signing_secret))
            .field("reason", &self.reason)
            .field("signed_by", &self.signed_by)
            .finish()
    }
}

pub struct DeliveryCredentials {
    subscriptions: Arc<SubscriptionRepository>,
    connections: Arc<ConnectionRepository>,
    applications: Arc<ApplicationRepository>,
    outbound: Arc<OutboundCredentialsResolver>,
}

impl DeliveryCredentials {
    pub fn new(
        subscriptions: Arc<SubscriptionRepository>,
        connections: Arc<ConnectionRepository>,
        applications: Arc<ApplicationRepository>,
        outbound: Arc<OutboundCredentialsResolver>,
    ) -> Self {
        Self {
            subscriptions,
            connections,
            applications,
            outbound,
        }
    }

    /// [`Self::resolve`], degraded to a bare delivery with a WARN when a
    /// lookup fails (Java `resolveOrBare`, `dispatch-seam.md` §5).
    pub async fn resolve_or_bare(&self, job: &DispatchJob) -> Resolved {
        match self.resolve(job).await {
            Ok(r) => r,
            Err(e) => {
                warn!(job_id = %job.id, error = %e, "delivery credential lookup failed; delivering unsigned");
                Resolved::bare(format!("credential lookup failed: {e}"))
            }
        }
    }

    pub async fn resolve(&self, job: &DispatchJob) -> Result<Resolved> {
        let subscription = match job.subscription_id.as_deref() {
            Some(id) => self.subscriptions.find_by_id(id).await?,
            None => None,
        };
        let connection = match subscription.as_ref().and_then(connection_to_load) {
            Some(connection_id) => self.connections.find_by_id(connection_id).await?,
            None => None,
        };

        let application_code =
            match signer_of(&job.code, subscription.as_ref(), connection.as_ref()) {
                Signer::Named { account_id, who } => return self.named(&who, &account_id).await,
                Signer::Nobody => {
                    return Ok(Resolved::bare(
                        "no subscription, connection or application names a service account",
                    ))
                }
                Signer::OfApplication(code) => code,
            };
        let Some(application) = self.applications.find_by_code(&application_code).await? else {
            return Ok(Resolved::bare(format!(
                "application {application_code} does not exist"
            )));
        };
        match self.outbound.for_application(&application.id).await? {
            Some(creds) if !creds.is_empty() => Ok(Resolved::signed(
                creds.token,
                creds.signing_secret,
                creds.signed_by,
            )),
            _ => Ok(Resolved::bare(format!(
                "application {application_code} has no active service account"
            ))),
        }
    }

    async fn named(&self, who: &str, account: &str) -> Result<Resolved> {
        Ok(match self.outbound.by_service_account_id(account).await? {
            ById::Found(creds) => {
                Resolved::signed(creds.token, creds.signing_secret, creds.signed_by)
            }
            ById::Missing => {
                Resolved::bare(format!("{who}: service account {account} does not exist"))
            }
            ById::Inactive(code) => {
                Resolved::bare(format!("{who}: service account {code} is inactive"))
            }
            ById::NoCredentials(code) => Resolved::bare(format!(
                "{who}: service account {code} has no webhook credentials"
            )),
        })
    }
}

/// Which identity the resolution order (module doc) signs a job with,
/// decided from configuration alone, before any credential is looked up
/// (Java `DeliveryCredentials.signerOf`). [`DeliveryCredentials::resolve`] is
/// built on it, and so is the ingest guard
/// ([`crate::dispatch_job::signing_guard`]), which asks whether the caller
/// may cause that identity's signature: one definition of the order, so the
/// guard cannot check a different account than the one that will sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signer {
    /// Steps 1-2: an account the subscription (or its connection) names.
    /// `who` is `subscription <code>` / `connection <code>`.
    Named { account_id: String, who: String },
    /// Step 3: this application's oldest active account.
    OfApplication(String),
    /// Nothing names an account: a bare delivery.
    Nobody,
}

/// The connection whose account step 2 would read: the subscription's, when
/// the subscription names no account of its own.
pub fn connection_to_load(subscription: &Subscription) -> Option<&str> {
    if non_blank(subscription.service_account_id.as_deref()).is_some() {
        return None;
    }
    non_blank(subscription.connection_id.as_deref())
}

/// [`Signer`] for a job with code `job_code`, its subscription (if it names
/// one that exists) and that subscription's connection (as
/// [`connection_to_load`] names it, if it exists).
pub fn signer_of(
    job_code: &str,
    subscription: Option<&Subscription>,
    connection: Option<&Connection>,
) -> Signer {
    if let Some(sub) = subscription {
        if let Some(account) = non_blank(sub.service_account_id.as_deref()) {
            return Signer::Named {
                account_id: account.to_string(),
                who: format!("subscription {}", sub.code),
            };
        }
        if let Some(connection) =
            connection.filter(|c| connection_to_load(sub) == Some(c.id.as_str()))
        {
            if let Some(account) = non_blank(Some(&connection.service_account_id)) {
                return Signer::Named {
                    account_id: account.to_string(),
                    who: format!("connection {}", connection.code),
                };
            }
        }
    }
    match subscription
        .and_then(|s| non_blank(s.application_code.as_deref()))
        .map(str::to_string)
        .or_else(|| leading_segment(job_code))
    {
        Some(application_code) => Signer::OfApplication(application_code),
        None => Signer::Nobody,
    }
}

/// Java `String.isBlank` as `blankToNull` uses it.
fn non_blank(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.trim().is_empty())
}

/// The code up to its first `:`, when there is one and it is non-empty.
pub fn leading_segment(code: &str) -> Option<String> {
    let (segment, _) = code.split_once(':')?;
    (!segment.is_empty()).then(|| segment.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_application_is_the_code_before_its_first_colon() {
        assert_eq!(
            leading_segment("billing:invoice:created"),
            Some("billing".into())
        );
        assert_eq!(leading_segment(":invoice"), None);
        assert_eq!(leading_segment("no-colon"), None);
    }

    fn subscription(
        account: Option<&str>,
        connection: Option<&str>,
        app: Option<&str>,
    ) -> Subscription {
        let mut s = Subscription::new("sub-a", "Sub A", "https://example.test/hook");
        s.service_account_id = account.map(str::to_string);
        s.connection_id = connection.map(str::to_string);
        s.application_code = app.map(str::to_string);
        s
    }

    #[test]
    fn the_signer_follows_the_resolution_order() {
        let mut conn = Connection::new("conn-a", "Conn A", "sac_conn");
        conn.id = "con_1".into();

        // 1. The subscription's own account wins over its connection's.
        let sub = subscription(Some("sac_sub"), Some("con_1"), Some("billing"));
        assert_eq!(
            signer_of("billing:x:y", Some(&sub), Some(&conn)),
            Signer::Named {
                account_id: "sac_sub".into(),
                who: "subscription sub-a".into()
            }
        );
        // 2. Else its connection's.
        let sub = subscription(Some(" "), Some("con_1"), Some("billing"));
        assert_eq!(
            signer_of("billing:x:y", Some(&sub), Some(&conn)),
            Signer::Named {
                account_id: "sac_conn".into(),
                who: "connection conn-a".into()
            }
        );
        // A connection that is not the subscription's is ignored.
        let sub = subscription(None, Some("con_2"), Some("billing"));
        assert_eq!(
            signer_of("orders:x:y", Some(&sub), Some(&conn)),
            Signer::OfApplication("billing".into())
        );
        // 3. The subscription's application, else the code's leading segment.
        assert_eq!(
            signer_of("orders:x:y", None, None),
            Signer::OfApplication("orders".into())
        );
        assert_eq!(signer_of("no-colon", None, None), Signer::Nobody);
    }

    #[test]
    fn debug_never_prints_a_credential() {
        let r = Resolved::signed(
            Some("tok_MARKER".into()),
            Some("sec_MARKER".into()),
            "sa".into(),
        );
        assert!(!format!("{r:?}").contains("MARKER"));
        assert!(!r.is_bare());
        assert!(Resolved::bare("x").is_bare());
    }
}

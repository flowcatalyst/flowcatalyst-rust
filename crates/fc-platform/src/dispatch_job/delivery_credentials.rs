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
use crate::connection::repository::ConnectionRepository;
use crate::service_account::outbound_credentials::{ById, OutboundCredentialsResolver};
use crate::shared::error::Result;
use crate::{ApplicationRepository, SubscriptionRepository};

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
        if let Some(sub) = &subscription {
            if let Some(account) = non_blank(sub.service_account_id.as_deref()) {
                return self
                    .named(&format!("subscription {}", sub.code), account)
                    .await;
            }
            if let Some(connection_id) = non_blank(sub.connection_id.as_deref()) {
                if let Some(connection) = self.connections.find_by_id(connection_id).await? {
                    if let Some(account) = non_blank(Some(&connection.service_account_id)) {
                        return self
                            .named(&format!("connection {}", connection.code), account)
                            .await;
                    }
                }
            }
        }

        let application_code = subscription
            .as_ref()
            .and_then(|s| non_blank(s.application_code.as_deref()))
            .map(str::to_string)
            .or_else(|| leading_segment(&job.code));
        let Some(application_code) = application_code else {
            return Ok(Resolved::bare(
                "no subscription, connection or application names a service account",
            ));
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

/// Java `String.isBlank` as `blankToNull` uses it.
fn non_blank(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.trim().is_empty())
}

/// The code up to its first `:`, when there is one and it is non-empty.
fn leading_segment(code: &str) -> Option<String> {
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

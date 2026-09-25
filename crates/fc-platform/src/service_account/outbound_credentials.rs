//! An application's outbound credentials (Java
//! `serviceaccount/OutboundCredentials.java`): the bearer token and HMAC
//! signing secret of the application's **oldest active service account**, or
//! of one named account. One resolver, shared by the platform's two
//! deliveries, as Java shares it: the dispatch-job processing endpoint
//! (`docs/spec/dispatch-delivery-credentials.md` §2) and the scheduled-job
//! dispatcher (`docs/spec/scheduled-job-scheduler.md` §3 step 5). Answers
//! are cached for one minute per key, as Java's `OutboundCredentials.cached`.
//!
//! The stored values are `encrypted:` refs; this is the only place they are
//! opened for a delivery. A ref that cannot be opened (no encryption key, a
//! plaintext value at rest, a bad ciphertext) reads as absent, with a WARN
//! that names the account and never the value: the delivery then goes out
//! without that credential, as Java's degraded cases do.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tracing::warn;

use super::repository::{ServiceAccountRepository, StoredWebhookCredentials};
use crate::shared::encryption_service::EncryptionService;
use crate::shared::error::Result;

/// How long one answer is reused (Java `OutboundCredentials.Cache.TTL`).
const TTL: Duration = Duration::from_secs(60);

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

/// Resolves and caches outbound credentials.
pub struct OutboundCredentialsResolver {
    service_accounts: Arc<ServiceAccountRepository>,
    encryption: Option<Arc<EncryptionService>>,
    by_application: DashMap<String, (Option<OutboundCredentials>, Instant)>,
    by_id: DashMap<String, (ById, Instant)>,
}

impl OutboundCredentialsResolver {
    pub fn new(
        service_accounts: Arc<ServiceAccountRepository>,
        encryption: Option<Arc<EncryptionService>>,
    ) -> Self {
        Self {
            service_accounts,
            encryption,
            by_application: DashMap::new(),
            by_id: DashMap::new(),
        }
    }

    /// The application's oldest active service account's credentials
    /// (Java `OutboundCredentials.resolve`); `None` when it has no active
    /// account at all.
    pub async fn for_application(
        &self,
        application_id: &str,
    ) -> Result<Option<OutboundCredentials>> {
        if let Some(hit) = self.by_application.get(application_id) {
            if hit.1.elapsed() < TTL {
                return Ok(hit.0.clone());
            }
        }
        let stored = self
            .service_accounts
            .oldest_active_webhook_credentials(application_id)
            .await?;
        let resolved = stored.map(|s| self.open(&s));
        self.by_application.insert(
            application_id.to_string(),
            (resolved.clone(), Instant::now()),
        );
        Ok(resolved)
    }

    /// [`Self::for_application`] for many applications at once, read fresh:
    /// no cache is consulted or filled (Java's desired state calls the
    /// uncached `OutboundCredentials.resolve`, memoised for one build only,
    /// so a rotated secret reaches the next poll). Absent for an application
    /// with no active account.
    pub async fn for_applications_fresh(
        &self,
        application_ids: &[String],
    ) -> Result<std::collections::HashMap<String, OutboundCredentials>> {
        let stored = self
            .service_accounts
            .oldest_active_webhook_credentials_for(application_ids)
            .await?;
        Ok(stored
            .into_iter()
            .map(|(application_id, s)| (application_id, self.open(&s)))
            .collect())
    }

    /// One named account's credentials (Java `OutboundCredentials.resolveById`).
    pub async fn by_service_account_id(&self, id: &str) -> Result<ById> {
        if let Some(hit) = self.by_id.get(id) {
            if hit.1.elapsed() < TTL {
                return Ok(hit.0.clone());
            }
        }
        let resolved = match self.service_accounts.webhook_credentials_by_id(id).await? {
            None => ById::Missing,
            Some(s) if !s.active => ById::Inactive(s.code),
            Some(s) => {
                let creds = self.open(&s);
                if creds.is_empty() {
                    ById::NoCredentials(creds.signed_by)
                } else {
                    ById::Found(creds)
                }
            }
        };
        self.by_id
            .insert(id.to_string(), (resolved.clone(), Instant::now()));
        Ok(resolved)
    }

    fn open(&self, stored: &StoredWebhookCredentials) -> OutboundCredentials {
        OutboundCredentials {
            token: self.open_ref(stored.token_ref.as_deref(), &stored.code, "bearer token"),
            signing_secret: self.open_ref(
                stored.signing_secret_ref.as_deref(),
                &stored.code,
                "signing secret",
            ),
            signed_by: stored.code.clone(),
        }
    }

    fn open_ref(&self, stored: Option<&str>, code: &str, what: &str) -> Option<String> {
        let stored = stored.filter(|s| !s.is_empty())?;
        let Some(encryption) = &self.encryption else {
            warn!(service_account = %code, "{what} cannot be opened: encryption is not configured");
            return None;
        };
        match encryption.decrypt_ref(stored) {
            Ok(plain) if !plain.is_empty() => Some(plain),
            Ok(_) => None,
            Err(e) => {
                warn!(service_account = %code, error = %e, "{what} cannot be opened");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_a_credential() {
        let creds = OutboundCredentials {
            token: Some("tok_MARKER".into()),
            signing_secret: Some("sec_MARKER".into()),
            signed_by: "billing-sa".into(),
        };
        let text = format!("{creds:?} {:?}", ById::Found(creds.clone()));
        assert!(!text.contains("MARKER"), "{text}");
        assert!(text.contains("billing-sa"));
    }
}

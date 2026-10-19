//! Confinement of an application-scoped caller on the SDK ingest endpoints.
//!
//! Each application delivers its own outbox with its own service-account
//! credentials, so an application service account holds the three batch
//! ingest permissions (events, dispatch jobs, audit logs). Granting them must
//! not widen reach: for a caller whose application scope is not
//! [`ApplicationScope::All`] (an application-scoped caller), the ingest
//! endpoints refuse items belonging to applications outside that scope.
//! Callers with all-applications access are never restricted and cost no
//! application lookup.
//!
//! The application of an event is the first `:` segment of its type, of a
//! dispatch job the first segment of its code (the `split_part(code, ':', 1)`
//! the platform's application facet uses). An unknown application is refused
//! exactly like an inaccessible one. Answers are remembered for the life of
//! one request ([`RequestAppScope`]), so a thousand-item batch costs one
//! lookup per distinct application code.

use std::collections::HashMap;
use std::sync::Arc;

use fc_platform_core::directory::{ApplicationAccess, ApplicationDirectory};
use fc_platform_core::shared::authorization_service::{ApplicationScope, AuthContext};
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::shared::id::SubscriptionId;

use crate::subscription::repository::SubscriptionRepository;

/// Refusal text for an event whose application the caller may not ingest for.
pub const EVENT_TYPE_APP_REFUSED: &str =
    "event type belongs to an application this credential cannot ingest for";
/// Refusal text for a dispatch job whose code names such an application.
pub const JOB_CODE_APP_REFUSED: &str =
    "dispatch job code belongs to an application this credential cannot ingest for";
/// Refusal text for a dispatch job routed through such an application's subscription.
pub const JOB_SUBSCRIPTION_APP_REFUSED: &str =
    "subscription belongs to an application this credential cannot ingest for";

/// Refusal text for a read by a caller bound to no (resolvable) application.
pub const READ_SCOPE_REFUSED: &str = "this credential is not bound to an application";

/// The application code of a `{application}:...` code: everything before the
/// first colon, or the whole string when there is none.
pub fn first_segment(code: &str) -> &str {
    code.trim().split(':').next().unwrap_or_default()
}

/// Builds the per-request [`RequestAppScope`].
#[derive(Clone)]
pub struct AppScopeGuard {
    applications: Arc<dyn ApplicationDirectory>,
    access: Arc<dyn ApplicationAccess>,
    subscriptions: Arc<SubscriptionRepository>,
}

impl AppScopeGuard {
    pub fn new(
        applications: Arc<dyn ApplicationDirectory>,
        access: Arc<dyn ApplicationAccess>,
        subscriptions: Arc<SubscriptionRepository>,
    ) -> Self {
        Self {
            applications,
            access,
            subscriptions,
        }
    }

    /// The caller's scope for one request.
    pub async fn for_request(&self, auth: &AuthContext) -> Result<RequestAppScope<'_>> {
        Ok(RequestAppScope {
            guard: self,
            scope: self.access.scope_for(&auth.principal_id).await?,
            codes: HashMap::new(),
            subscription_apps: HashMap::new(),
        })
    }
}

/// One caller's application scope for one request, with its lookups cached.
pub struct RequestAppScope<'a> {
    guard: &'a AppScopeGuard,
    scope: ApplicationScope,
    /// code -> whether the scope reaches that application (unknown: false).
    codes: HashMap<String, bool>,
    /// subscription id -> its application code (empty: unknown subscription
    /// or one without an application).
    subscription_apps: HashMap<String, String>,
}

impl RequestAppScope<'_> {
    /// Whether the caller is application-scoped (not all-applications).
    pub fn is_scoped(&self) -> bool {
        self.scope.is_application_scoped()
    }

    /// Whether the caller may act for the application with `code`.
    /// All-applications callers always may (no lookup); a scoped caller only
    /// when the code names an existing application it can access (an empty
    /// or unknown code is refused).
    pub async fn can_access_code(&mut self, code: &str) -> Result<bool> {
        if !self.is_scoped() {
            return Ok(true);
        }
        if code.is_empty() {
            return Ok(false);
        }
        if let Some(known) = self.codes.get(code) {
            return Ok(*known);
        }
        let reaches = match self.guard.applications.find_by_code(code).await? {
            Some(application) => self.scope.allows(&application.id),
            None => false,
        };
        self.codes.insert(code.to_string(), reaches);
        Ok(reaches)
    }

    /// The application codes a read is confined to (Go F20
    /// `AppScope.ConfinedCodes`): `None` for a caller that reaches every
    /// application, whose reads are not narrowed; otherwise the codes of the
    /// applications it is bound to, sorted. A caller bound to none (or to ids
    /// that resolve to none) is refused with 403 `FORBIDDEN`, never answered
    /// an empty page: a mis-provisioned credential must be loud.
    pub async fn confined_codes(&self) -> Result<Option<Vec<String>>> {
        let ApplicationScope::Only(ids) = &self.scope else {
            return Ok(None);
        };
        let mut codes: Vec<String> = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(application) = self.guard.applications.find_by_id(id).await? {
                if !application.code.is_empty() && !codes.contains(&application.code) {
                    codes.push(application.code);
                }
            }
        }
        if codes.is_empty() {
            return Err(PlatformError::forbidden_code(
                "FORBIDDEN",
                READ_SCOPE_REFUSED,
            ));
        }
        codes.sort();
        Ok(Some(codes))
    }

    /// Why a dispatch job must be refused, `None` when it passes. Both must
    /// hold: the application named by the first segment of the job `code`
    /// exists and is accessible; and, when the job carries a subscription,
    /// that subscription exists, has an application, and the application is
    /// accessible (a job cannot be routed through another application's
    /// subscription). A no-op for all-applications callers.
    pub async fn job_refusal(
        &mut self,
        code: &str,
        subscription_id: Option<&str>,
    ) -> Result<Option<&'static str>> {
        if !self.is_scoped() {
            return Ok(None);
        }
        if !self.can_access_code(first_segment(code)).await? {
            return Ok(Some(JOB_CODE_APP_REFUSED));
        }
        let Some(subscription_id) = subscription_id.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let application_code = match self.subscription_apps.get(subscription_id) {
            Some(code) => code.clone(),
            None => {
                let code = self
                    .guard
                    .subscriptions
                    .find_by_id(&SubscriptionId::from_wire(subscription_id))
                    .await?
                    .and_then(|s| s.application_code)
                    .unwrap_or_default();
                self.subscription_apps
                    .insert(subscription_id.to_string(), code.clone());
                code
            }
        };
        if self.can_access_code(&application_code).await? {
            Ok(None)
        } else {
            Ok(Some(JOB_SUBSCRIPTION_APP_REFUSED))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_application_is_the_code_before_the_first_colon() {
        assert_eq!(first_segment("orders:billing:invoice:paid"), "orders");
        assert_eq!(first_segment("  orders:x "), "orders");
        assert_eq!(first_segment("orders"), "orders");
        assert_eq!(first_segment(""), "");
        assert_eq!(first_segment(":x"), "");
    }
}

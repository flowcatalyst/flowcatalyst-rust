//! Confinement of an application-scoped caller on reads (Go
//! `sdk.AppScope.ConfinedCodes`, introduced by F20 and used by F21).
//!
//! An application service account (a caller without all-applications access)
//! may read only what belongs to the applications it is bound to. A caller
//! that reaches every application is never narrowed. A caller bound to none
//! (or to ids that no longer resolve) is refused with 403 `FORBIDDEN`, not
//! answered an empty page, so a mis-provisioned credential is loud, never
//! silently blind.

use std::collections::HashSet;

use crate::application::repository::ApplicationRepository;
use crate::shared::authorization_service::ApplicationAccessService;
use fc_platform_core::shared::authorization_service::{ApplicationScope, AuthContext};
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::shared::id::ApplicationId;

/// The message of the refusal an application-scoped read gives a caller bound
/// to no (resolvable) application.
pub const READ_SCOPE_REFUSED: &str = "this credential is not bound to an application";

/// The applications a read is confined to: the ids the caller is bound to and
/// the codes those ids resolve to (a dangling id resolves to no code).
#[derive(Debug, Clone)]
pub struct ConfinedApplications {
    ids: HashSet<ApplicationId>,
    codes: Vec<String>,
}

impl ConfinedApplications {
    /// The codes of the applications the caller is bound to.
    pub fn codes(&self) -> &[String] {
        &self.codes
    }

    /// Whether the caller is bound to the application with this id.
    pub fn allows_id(&self, id: &ApplicationId) -> bool {
        self.ids.contains(id)
    }

    /// Whether the caller is bound to the application with this code.
    pub fn allows_code(&self, code: &str) -> bool {
        self.codes.iter().any(|c| c == code)
    }
}

/// Which applications a read is confined to for `auth`: `None` for a caller
/// that reaches every application; otherwise the applications it is bound
/// to. A caller bound to none, or only to ids that no longer resolve, is
/// refused: 403 `FORBIDDEN`.
pub async fn confined_applications(
    access: &ApplicationAccessService,
    applications: &ApplicationRepository,
    auth: &AuthContext,
) -> Result<Option<ConfinedApplications>> {
    let ApplicationScope::Only(ids) = access.scope_for(&auth.principal_id).await? else {
        return Ok(None);
    };
    let wanted: Vec<ApplicationId> = ids.iter().cloned().collect();
    let found = applications.find_by_ids(&wanted).await?;
    let mut codes: Vec<String> = found
        .values()
        .map(|a| a.code.clone())
        .filter(|c| !c.is_empty())
        .collect();
    codes.sort();
    if codes.is_empty() {
        return Err(PlatformError::forbidden(READ_SCOPE_REFUSED));
    }
    Ok(Some(ConfinedApplications { ids, codes }))
}

/// The first `:` segment of a code (an event type, a permission, a dispatch
/// job code): the application code. Everything when there is no colon.
pub fn first_segment(code: &str) -> &str {
    code.trim().split(':').next().unwrap_or_default()
}

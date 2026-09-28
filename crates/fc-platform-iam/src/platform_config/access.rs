//! Who may read or write an application's platform config (Go
//! platformconfig/api/api.go:47-57, 90-100, 147-157 and
//! operations/set_property.go:60-73): an anchor, or a caller one of whose
//! roles holds a read or write grant for the application.

use super::access_repository::PlatformConfigAccessRepository;
use fc_platform_core::shared::error::PlatformError;

/// Refuses unless the caller is an anchor or one of `roles` holds a `write`
/// (else read) grant for `app_code`: 403 `No {read|write} access to
/// platform config for {app}`.
pub async fn require_config_access(
    access_repo: &PlatformConfigAccessRepository,
    is_anchor: bool,
    roles: &[String],
    app_code: &str,
    write: bool,
) -> Result<(), PlatformError> {
    if is_anchor {
        return Ok(());
    }
    let granted = !roles.is_empty()
        && access_repo
            .find_by_role_codes(app_code, roles)
            .await?
            .iter()
            .any(|a| if write { a.can_write } else { a.can_read });
    if granted {
        return Ok(());
    }
    let kind = if write { "write" } else { "read" };
    Err(PlatformError::forbidden(format!(
        "No {kind} access to platform config for {app_code}"
    )))
}

//! Resource-level authorization shared by the user-administration use cases
//! (Go `principal/operations/authz.go`): which users a caller may manage.

use crate::principal::{entity::Principal, repository::PrincipalRepository};
use fc_platform_core::principal_kind::UserScope;
use fc_platform_core::shared::authorization_service::Authority;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::usecase::UseCaseError;

/// Go's per-resource user-admin gate (`requireUserResourceAccess` /
/// `requireUserAdmin`): a non-anchor administrator (a client administrator)
/// acts only on CLIENT-tier users (403 otherwise, Go
/// `blockNonClientTarget`) homed at a client it reaches; a user out of reach
/// answers the same 404 as a missing one (`resource` names it:
/// `Principal` or `User`, as Go does). Anchors pass.
pub fn require_user_resource_access(
    caller: &impl Authority,
    target: &Principal,
    resource: &str,
) -> Result<(), PlatformError> {
    if !caller.is_anchor() && target.scope != UserScope::Client {
        return Err(PlatformError::forbidden(
            "Client administrators can only manage client-scope users",
        ));
    }
    // Go `auth.CanAccessScope`, the rule `check_scope_access` applies; out
    // of reach answers the not-found a missing id would (PR-3(b)).
    if !caller_reach::can_access_scope(caller, target.client_id.as_deref()) {
        return Err(PlatformError::not_found(resource, target.id.as_str()));
    }
    Ok(())
}

/// Load the user a user-administration write targets and apply
/// [`require_user_resource_access`]: missing is `Principal_NOT_FOUND`,
/// out of reach `<resource>_NOT_FOUND`, exactly as the handlers answered.
pub async fn load_administered_user(
    principals: &PrincipalRepository,
    caller: &impl Authority,
    id: &str,
    resource: &str,
) -> Result<Principal, UseCaseError> {
    let target = principals
        .find_by_id(id)
        .await?
        .ok_or_else(|| UseCaseError::verbatim(PlatformError::not_found("Principal", id)))?;
    require_user_resource_access(caller, &target, resource).map_err(UseCaseError::verbatim)?;
    Ok(target)
}

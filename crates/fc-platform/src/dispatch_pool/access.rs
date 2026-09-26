//! Resource-level access to dispatch pools, for the server-rendered
//! `fc-web` UI. Permission checks come first; these decide whether the
//! caller reaches *this* pool (or client).
//!
//! The `/api/dispatch-pools` handlers use Go's `CheckScopeAccess` instead
//! (`caller_reach::check_scope_access`, 403 `SCOPE_FORBIDDEN`), after
//! `CanWriteDispatchPools`.

use crate::shared::error::PlatformError;
use crate::{AuthContext, DispatchPool};

/// Creating a pool: an anchor anywhere; anyone else only for a client they
/// can access (a platform-wide pool needs an anchor).
pub fn ensure_can_create(auth: &AuthContext, client_id: Option<&str>) -> Result<(), PlatformError> {
    if auth.is_anchor() {
        return Ok(());
    }
    match client_id {
        Some(cid) if auth.can_access_client(cid) => Ok(()),
        Some(_) => Err(PlatformError::forbidden("No access to this client")),
        None => Err(PlatformError::forbidden(
            "Client ID required for non-anchor users",
        )),
    }
}

/// Whether the caller sees this pool: anchor-level pools are visible to
/// every authenticated reader, a client's pools to callers with access to
/// that client.
pub fn is_visible(auth: &AuthContext, pool: &DispatchPool) -> bool {
    auth.is_anchor()
        || pool
            .client_id
            .as_deref()
            .is_none_or(|cid| auth.can_access_client(cid))
}

/// As [`is_visible`], refusing with the handlers' 403.
pub fn ensure_visible(auth: &AuthContext, pool: &DispatchPool) -> Result<(), PlatformError> {
    if is_visible(auth, pool) {
        Ok(())
    } else {
        Err(PlatformError::forbidden("No access to this dispatch pool"))
    }
}

/// As [`ensure_visible`], and an anchor-level pool may only be changed by
/// an anchor user. `verb` names the change in the refusal ("update",
/// "archive", "suspend", "activate").
pub fn ensure_modifiable(
    auth: &AuthContext,
    pool: &DispatchPool,
    verb: &str,
) -> Result<(), PlatformError> {
    if auth.is_anchor() {
        return Ok(());
    }
    match pool.client_id.as_deref() {
        Some(cid) if auth.can_access_client(cid) => Ok(()),
        Some(_) => Err(PlatformError::forbidden("No access to this dispatch pool")),
        None => Err(PlatformError::forbidden(format!(
            "Cannot {verb} anchor-level dispatch pool"
        ))),
    }
}

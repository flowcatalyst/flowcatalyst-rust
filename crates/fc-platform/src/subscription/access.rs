//! Resource-level access to subscriptions, shared by the `/api/subscriptions`
//! handlers and the server-rendered `fc-web` UI. Permission checks
//! (`checks::can_read_subscriptions` / `can_write_subscriptions` /
//! `can_delete_subscriptions`) come first; these decide whether the caller
//! may touch *this* subscription.

use crate::shared::error::PlatformError;
use crate::{AuthContext, Subscription};

/// Whether a subscription appears in the caller's list: a client-owned one
/// to callers with access to that client, an anchor-level one to anchor
/// users only.
pub fn is_listed(auth: &AuthContext, subscription: &Subscription) -> bool {
    match subscription.client_id.as_deref() {
        Some(cid) => auth.can_access_client(cid),
        None => auth.is_anchor(),
    }
}

/// A client-owned subscription is visible to callers with access to that
/// client; an anchor-level one to everyone who passed the permission check.
pub fn ensure_visible(
    auth: &AuthContext,
    subscription: &Subscription,
) -> Result<(), PlatformError> {
    match subscription.client_id.as_deref() {
        Some(cid) if !auth.can_access_client(cid) => {
            Err(PlatformError::forbidden("No access to this subscription"))
        }
        _ => Ok(()),
    }
}

/// As [`ensure_visible`], and an anchor-level subscription may only be
/// changed by an anchor user. `verb` names the change in the refusal
/// ("modify", "delete").
pub fn ensure_modifiable(
    auth: &AuthContext,
    subscription: &Subscription,
    verb: &str,
) -> Result<(), PlatformError> {
    ensure_visible(auth, subscription)?;
    if subscription.client_id.is_none() && !auth.is_anchor() {
        return Err(PlatformError::forbidden(format!(
            "Only anchor users can {verb} anchor-level subscriptions"
        )));
    }
    Ok(())
}

/// Whether the caller may create a subscription owned by `client_id`: a
/// client-owned one needs access to that client, an anchor-level one
/// (`None`) an anchor user.
pub fn ensure_can_create(auth: &AuthContext, client_id: Option<&str>) -> Result<(), PlatformError> {
    match client_id {
        Some(cid) if !auth.can_access_client(cid) => Err(PlatformError::forbidden(format!(
            "No access to client: {}",
            cid
        ))),
        Some(_) => Ok(()),
        None if !auth.is_anchor() => Err(PlatformError::forbidden(
            "Only anchor users can create anchor-level subscriptions",
        )),
        None => Ok(()),
    }
}

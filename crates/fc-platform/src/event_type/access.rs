//! Resource-level access to an existing event type, shared by the BFF
//! handlers and the server-rendered `fc-web` UI. Permission checks
//! (`checks::can_read_event_types` / `can_write_event_types`) come first;
//! these decide whether the caller may touch *this* event type.

use crate::shared::error::PlatformError;
use crate::{AuthContext, EventType};

/// A client-owned event type is visible to callers with access to that
/// client; an anchor-level one to everyone who passed the permission check.
pub fn ensure_visible(auth: &AuthContext, event_type: &EventType) -> Result<(), PlatformError> {
    match event_type.client_id.as_deref() {
        Some(cid) if !auth.can_access_client(cid) => {
            Err(PlatformError::forbidden("No access to this event type"))
        }
        _ => Ok(()),
    }
}

/// As [`ensure_visible`], and an anchor-level event type may only be
/// changed by an anchor user. `verb` names the change in the refusal
/// ("modify", "delete", "archive").
pub fn ensure_modifiable(
    auth: &AuthContext,
    event_type: &EventType,
    verb: &str,
) -> Result<(), PlatformError> {
    ensure_visible(auth, event_type)?;
    if event_type.client_id.is_none() && !auth.is_anchor() {
        return Err(PlatformError::forbidden(format!(
            "Only anchor users can {verb} anchor-level event types"
        )));
    }
    Ok(())
}

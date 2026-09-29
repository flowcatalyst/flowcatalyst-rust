//! Resource-level access to an existing connection, shared by the API
//! handlers and the server-rendered `fc-web` UI. The permission checks
//! (`checks::can_read_connections`, …) come first; this decides whether the
//! caller reaches *this* connection.

use crate::connection::entity::Connection;
use fc_platform_core::shared::authorization_service::AuthContext;
use fc_platform_core::shared::caller_reach::reaches_client;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::id::OptionIdExt;

/// A platform connection is visible to every holder of the read
/// permission; a client's only to callers reaching that client (Go
/// `FilterClientScoped`).
pub fn is_visible(auth: &AuthContext, connection: &Connection) -> bool {
    connection
        .client_id
        .as_id_str()
        .is_none_or(|cid| reaches_client(auth, cid))
}

/// As [`is_visible`], refusing with the handlers' 403.
pub fn ensure_visible(auth: &AuthContext, connection: &Connection) -> Result<(), PlatformError> {
    if is_visible(auth, connection) {
        Ok(())
    } else {
        Err(PlatformError::forbidden("No access to this connection"))
    }
}

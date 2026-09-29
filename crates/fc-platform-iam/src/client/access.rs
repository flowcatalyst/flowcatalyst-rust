//! Resource-level access to one client, shared by the client API handlers
//! and the server-rendered `fc-web` UI. Permission checks
//! (`checks::can_read_clients`, …) come first; this decides whether the
//! caller may see *this* client.

use fc_platform_core::shared::authorization_service::AuthContext;
use fc_platform_core::shared::error::PlatformError;

/// An anchor caller sees every client; anyone else only the clients they
/// can access.
pub fn ensure_visible(auth: &AuthContext, client_id: &str) -> Result<(), PlatformError> {
    if !auth.is_anchor() && !auth.can_access_client(client_id) {
        return Err(PlatformError::forbidden("No access to this client"));
    }
    Ok(())
}

//! Authentication and authorization for UI handlers.
//!
//! Two layers, like the API's handler + use case split:
//!
//! 1. **Authentication** — the `#[layer("/ui/(app)")]` guard in
//!    `app::shell` resolves the caller from the request headers and puts the
//!    [`AuthContext`] into the request context. Anonymous GETs are sent to
//!    the login page; anything else (form posts, shard and procedure calls)
//!    is refused with 401. Every page, shard, procedure and route that needs
//!    a user is registered under `/ui/(app)` so the layer wraps it — a
//!    shard's default path (`/_topcoat/runtime/...`) would bypass it.
//! 2. **Authorization** — each handler then calls [`auth`] and one of the
//!    platform's `checks::*` functions through [`permit`], exactly as the
//!    axum handlers do. `tests/auth_convention_test.rs` enforces both.
//!
//! [`auth`] fails closed: a handler that somehow runs outside the layer gets
//! a 401, not a panic and not an anonymous page.

use fc_platform::shared::middleware::authenticate_headers;
use fc_platform::{AuthContext, PlatformError};
use topcoat::Result;
use topcoat::context::{Cx, try_request_context};
use topcoat::router::error::{
    bad_request, forbidden, internal_server_error, not_found, unauthorized,
};
use topcoat::router::request::headers;

/// Resolve the caller from the request headers: the `fc_session` cookie or
/// a Bearer token, with the same rules as the API's `Authenticated`
/// extractor. `Ok(None)` is anonymous.
pub(crate) async fn authenticate(cx: &Cx) -> Result<Option<AuthContext>> {
    authenticate_headers(&crate::deps(cx).app_state, headers(cx))
        .await
        .map_err(|e| {
            if e.status.is_server_error() {
                internal_server_error(PlatformError::internal(e.message)).into()
            } else {
                unauthorized().into()
            }
        })
}

/// The authenticated caller, as registered by the `/ui/(app)` layer.
pub fn auth(cx: &Cx) -> Result<&AuthContext> {
    try_request_context::<AuthContext>(cx).ok_or_else(|| unauthorized().into())
}

/// Map a platform authorization or lookup result onto Topcoat's router
/// errors: `permit(checks::can_read_event_types(auth))?`.
pub fn permit<T>(result: fc_platform::Result<T>) -> Result<T> {
    result.map_err(platform_error)
}

/// A platform error as the router error of the same status. Messages are
/// kept for 4xx validation-style errors (they are written for the user);
/// everything else maps to the bare status so internals don't leak.
pub fn platform_error(e: PlatformError) -> topcoat::Error {
    match e.status_code().as_u16() {
        401 => unauthorized().into(),
        403 => forbidden().into(),
        404 => not_found().into(),
        400 | 409 | 422 => bad_request(e.to_string()).into(),
        _ => {
            tracing::error!(error = %e, "fc-web: internal error");
            internal_server_error(e).into()
        }
    }
}

//! Embedded HTML operator dashboard.

use axum::response::{Html, IntoResponse};

/// Serve dashboard HTML, with the mount prefix injected so the page works
/// both standalone (no prefix) and when nested under a parent router
/// (e.g. fc-dev nests this whole crate under `/q/router`).
///
/// The injected `window.__API_BASE__` is consumed by `fetchWithAuth` in
/// `dashboard.html` to prepend onto every `/monitoring/...` request.
pub(crate) async fn dashboard_html_handler(
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> impl IntoResponse {
    const DASHBOARD_HTML: &str = include_str!("../../resources/dashboard.html");

    // The handler is mounted at both `/monitoring/dashboard` and
    // `/dashboard.html`; strip whichever matches to recover the prefix
    // that any nesting parent contributed. Fall back to empty when the
    // request URI doesn't end with either (shouldn't happen, but better
    // than guessing).
    let path = uri.path();
    let prefix = path
        .strip_suffix("/monitoring/dashboard")
        .or_else(|| path.strip_suffix("/dashboard.html"))
        .unwrap_or("");

    Html(DASHBOARD_HTML.replace("__FC_API_BASE__", prefix))
}

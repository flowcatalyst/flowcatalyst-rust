//! fc-server's CORS layer: the browser-side policy for the origins in the
//! CORS Origins table (`tnt_cors_allowed_origins`).
//!
//! **No credentials.** The layer never sends
//! `Access-Control-Allow-Credentials`, so a browser does not let a page on
//! another origin read a response to a request that carried the user's
//! cookies, and a credentialed preflight fails. The platform's browser
//! credential is the `fc_session` cookie; a cross-origin page that could
//! read cookie-authenticated responses could read whatever the signed-in
//! user can. The allow-list serves cross-origin calls that carry their own
//! bearer token (`Authorization`), which the browser sends without
//! credentials mode.
//!
//! An allowed origin is reflected in `Access-Control-Allow-Origin`; any
//! other origin gets no CORS headers at all (never `*`). Every response
//! carries `Vary: Origin` (and the preflight request headers), so a cache
//! does not serve one origin's answer to another.
//!
//! The platform's own SPA does not need CORS: it calls its own origin
//! (served by the platform, or through Vite's dev proxy in development).
//! The Go platform has no CORS layer at all. There is deliberately no switch
//! to turn credentials on: no first-party flow sends the cookie from another
//! origin (`docs/go-mirror/2026-10-14-platform-bug-fixes-7.md`, "CORS
//! follow-up").

use std::collections::HashSet;
use std::result;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use axum::http::{header, HeaderName, HeaderValue, Method};
use regex::Regex;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::warn;

use super::repository::CorsOriginRepository;
use fc_platform_core::shared::error::Result;

/// How long a browser may cache a preflight answer.
const PREFLIGHT_MAX_AGE: Duration = Duration::from_secs(86400);

/// The allowed origins, shared between the layer and the refresh loop.
/// Entries are exact origins (`https://app.example.com`) or patterns with
/// `*` standing for one DNS label (`https://*.example.com`).
#[derive(Clone, Default)]
pub struct AllowedOrigins(Arc<RwLock<OriginSet>>);

#[derive(Default)]
struct OriginSet {
    exact: HashSet<String>,
    /// The `*` entries, compiled once per refresh.
    patterns: Vec<Regex>,
}

impl AllowedOrigins {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the whole set.
    pub fn replace(&self, origins: impl IntoIterator<Item = String>) {
        let mut set = OriginSet::default();
        for origin in origins {
            if origin.contains('*') {
                match pattern(&origin) {
                    Ok(re) => set.patterns.push(re),
                    Err(e) => warn!(origin, error = %e, "Skipping an unusable CORS origin pattern"),
                }
            } else {
                set.exact.insert(origin);
            }
        }
        // A poisoned lock only means another thread panicked mid-refresh; the
        // set is replaced whole, so keep serving rather than fail every request.
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = set;
    }

    /// Reload the set from the table; the number of entries on success.
    pub async fn refresh(&self, repo: &CorsOriginRepository) -> Result<usize> {
        let origins = repo.get_allowed_origins().await?;
        let n = origins.len();
        self.replace(origins);
        Ok(n)
    }

    /// Whether `origin` (an `Origin` header value) is allowed.
    pub fn allows(&self, origin: &str) -> bool {
        let set = self.0.read().unwrap_or_else(PoisonError::into_inner);
        set.exact.contains(origin) || set.patterns.iter().any(|re| re.is_match(origin))
    }
}

/// `entry` with each `*` standing for one DNS label (letters, digits,
/// hyphens), anchored to match the whole origin.
fn pattern(entry: &str) -> result::Result<Regex, regex::Error> {
    Regex::new(&format!(
        "^{}$",
        regex::escape(entry).replace(r"\*", "[a-zA-Z0-9-]+")
    ))
}

/// The CORS layer for `origins` (see the module docs).
pub fn cors_layer(origins: AllowedOrigins) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(
            move |origin: &HeaderValue, _parts| {
                origin.to_str().is_ok_and(|origin| origins.allows(origin))
            },
        ))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
            Method::HEAD,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::ORIGIN,
            HeaderName::from_static("x-requested-with"),
            HeaderName::from_static("x-client-id"),
        ])
        // Never credentials: the platform's browser credential is the
        // session cookie, and no other origin may read what it unlocks.
        .allow_credentials(false)
        .vary([
            header::ORIGIN,
            header::ACCESS_CONTROL_REQUEST_METHOD,
            header::ACCESS_CONTROL_REQUEST_HEADERS,
        ])
        .max_age(PREFLIGHT_MAX_AGE)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    use super::*;

    const LISTED: &str = "https://app.example.com";

    fn app(origins: &[&str]) -> Router {
        let allowed = AllowedOrigins::new();
        allowed.replace(origins.iter().map(|o| o.to_string()));
        Router::new()
            .route(
                "/api/thing",
                get(|| async { "ok" }).delete(|| async { "gone" }),
            )
            .layer(cors_layer(allowed))
    }

    async fn send(app: Router, req: Request<Body>) -> Response<Body> {
        app.oneshot(req).await.expect("oneshot")
    }

    fn get_from(origin: &str) -> Request<Body> {
        Request::get("/api/thing")
            .header("origin", origin)
            .body(Body::empty())
            .unwrap()
    }

    fn preflight_from(origin: &str, headers: &str) -> Request<Body> {
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/thing")
            .header("origin", origin)
            .header("access-control-request-method", "DELETE")
            .header("access-control-request-headers", headers)
            .body(Body::empty())
            .unwrap()
    }

    fn header<'a>(resp: &'a Response<Body>, name: &str) -> Option<&'a str> {
        resp.headers().get(name).map(|v| v.to_str().unwrap())
    }

    fn varies_on_origin(resp: &Response<Body>) -> bool {
        resp.headers().get_all("vary").iter().any(|v| {
            v.to_str()
                .unwrap()
                .split(',')
                .any(|h| h.trim().eq_ignore_ascii_case("origin"))
        })
    }

    #[tokio::test]
    async fn a_listed_origin_is_reflected_without_credentials() {
        let resp = send(app(&[LISTED]), get_from(LISTED)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(header(&resp, "access-control-allow-origin"), Some(LISTED));
        assert_eq!(header(&resp, "access-control-allow-credentials"), None);
        assert!(varies_on_origin(&resp));
    }

    #[tokio::test]
    async fn a_listed_origins_preflight_allows_a_bearer_but_not_credentials() {
        let resp = send(
            app(&[LISTED]),
            preflight_from(LISTED, "authorization, content-type"),
        )
        .await;
        assert!(resp.status().is_success(), "{}", resp.status());
        assert_eq!(header(&resp, "access-control-allow-origin"), Some(LISTED));
        let allowed = header(&resp, "access-control-allow-headers")
            .unwrap()
            .to_ascii_lowercase();
        assert!(allowed.contains("authorization"), "{allowed}");
        assert!(header(&resp, "access-control-allow-methods")
            .unwrap()
            .contains("DELETE"));
        // Without `Allow-Credentials: true` a browser fails a credentialed
        // preflight, so the cookie-carrying request is never sent.
        assert_eq!(header(&resp, "access-control-allow-credentials"), None);
        assert!(varies_on_origin(&resp));
    }

    #[tokio::test]
    async fn an_unlisted_origin_gets_no_cors_headers() {
        for origin in [
            "https://evil.example.com",
            "https://app.example.com.evil.test",
            "http://app.example.com",
            "null",
        ] {
            for req in [get_from(origin), preflight_from(origin, "authorization")] {
                let resp = send(app(&[LISTED]), req).await;
                assert_eq!(
                    header(&resp, "access-control-allow-origin"),
                    None,
                    "{origin}"
                );
                assert_eq!(
                    header(&resp, "access-control-allow-credentials"),
                    None,
                    "{origin}"
                );
                assert!(varies_on_origin(&resp), "{origin}");
            }
        }
    }

    #[tokio::test]
    async fn no_origin_no_cors_headers_but_still_varies() {
        let req = Request::get("/api/thing").body(Body::empty()).unwrap();
        let resp = send(app(&[LISTED]), req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(header(&resp, "access-control-allow-origin"), None);
        assert!(varies_on_origin(&resp));
    }

    #[tokio::test]
    async fn never_a_wildcard_allow_origin() {
        // Even a table entry of `*` (refused by the create rules, but the
        // table is the source) is a pattern, not `Access-Control-Allow-Origin: *`.
        for origin in ["https://anything.test", "*"] {
            let resp = send(app(&["*"]), get_from(origin)).await;
            assert_ne!(header(&resp, "access-control-allow-origin"), Some("*"));
            assert_eq!(header(&resp, "access-control-allow-credentials"), None);
        }
    }

    #[test]
    fn wildcards_stand_for_one_label_and_match_the_whole_origin() {
        let allowed = AllowedOrigins::new();
        allowed.replace(["https://*.example.com".to_string()]);
        assert!(allowed.allows("https://app.example.com"));
        assert!(allowed.allows("https://a-1.example.com"));
        assert!(!allowed.allows("https://a.b.example.com"));
        assert!(!allowed.allows("https://example.com"));
        assert!(!allowed.allows("https://app.example.com.evil.test"));
        assert!(!allowed.allows("https://evil.test/https://app.example.com"));
        assert!(!allowed.allows("http://app.example.com"));
        assert!(!allowed.allows("https://app.example.com:8443"));
    }

    #[test]
    fn exact_entries_match_exactly_and_replace_drops_old_entries() {
        let allowed = AllowedOrigins::new();
        allowed.replace([LISTED.to_string()]);
        assert!(allowed.allows(LISTED));
        assert!(!allowed.allows("https://app.example.co"));
        allowed.replace(["https://other.test".to_string()]);
        assert!(!allowed.allows(LISTED));
        assert!(allowed.allows("https://other.test"));
    }
}

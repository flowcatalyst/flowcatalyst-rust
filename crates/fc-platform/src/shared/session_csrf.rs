//! The session-cookie request guard (Go `middleware.SessionCSRF`,
//! internal/platform/shared/middleware/crossorigin.go; docs F16).
//!
//! The SPA authenticates to `/api`, `/bff`, `/auth` and `/portal` with the
//! `fc_session` cookie (HttpOnly, SameSite=Lax by default). Lax keeps the
//! cookie off cross-site POSTs, but not off a page on a sibling subdomain
//! of the same registrable domain (same-site), nor in a browser that
//! ignores SameSite, nor at all when `FC_SESSION_COOKIE_SAME_SITE=None`. A
//! page there could change state with a request that needs no CORS
//! preflight: a body-less POST, or a `text/plain`, form or multipart body,
//! which the handlers that read their body as bytes decode as JSON.
//!
//! For a POST, PUT, PATCH or DELETE that the session cookie authenticates:
//!
//! 1. **Provenance** (Go's `net/http.CrossOriginProtection`):
//!    `Sec-Fetch-Site` `same-origin` or `none` passes; any other value is
//!    refused unless `Origin` is a trusted origin; without `Sec-Fetch-Site`,
//!    `Origin` must name the request's host or a trusted origin. The
//!    trusted origin is the public base URL (`FC_JWT_ISSUER` /
//!    `FC_EXTERNAL_BASE_URL`), for a proxy that rewrites `Host`. A request
//!    with **neither** header is refused too (every browser sends one of
//!    them on an unsafe request; a script replaying the cookie uses a
//!    bearer token instead). 403 `CROSS_ORIGIN_REQUEST`, logged.
//! 2. **JSON only**: a request with a body (a non-zero or unknown length)
//!    must declare `application/json` or `application/*+json`; anything
//!    else, an absent or malformed `Content-Type` included, is 415
//!    `UNSUPPORTED_MEDIA_TYPE`. A body-less request needs no `Content-Type`.
//!
//! Not affected: safe methods; a request authenticated by `Authorization:
//! Bearer` (the bearer wins when both are sent); a stale or invalid cookie
//! (it authenticates nothing, and the handler refuses the request as
//! unauthenticated); and the routes Go serves outside its authenticated
//! group ([`outside_the_authenticated_group`]: OAuth/OIDC, the public
//! sign-in, 2FA-challenge and password-reset routes, the dispatch
//! callbacks).

use std::sync::Arc;

use axum::body::HttpBody;
use axum::extract::{Request, State};
use axum::http::header::{
    AUTHORIZATION, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, HOST, TRANSFER_ENCODING,
};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::shared::api_common::ApiError;
use crate::shared::middleware::{extract_session_cookie, resolve_context};

/// The 403's code.
pub const CODE_CROSS_ORIGIN_REQUEST: &str = "CROSS_ORIGIN_REQUEST";
/// The 415's code.
pub const CODE_UNSUPPORTED_MEDIA_TYPE: &str = "UNSUPPORTED_MEDIA_TYPE";
/// Go's 403 message.
pub const CROSS_ORIGIN_MESSAGE: &str =
    "This request was refused because it did not come from this site. \
     Requests authenticated by the session cookie must be sent by the platform's own pages; \
     API clients authenticate with a bearer token.";
/// Go's 415 message.
pub const UNSUPPORTED_MEDIA_TYPE_MESSAGE: &str =
    "Requests authenticated by the session cookie must send a JSON body (Content-Type: application/json).";

/// Go `CredentialCSRF`'s 403 message (the sign-in routes).
pub const SIGN_IN_CROSS_ORIGIN_MESSAGE: &str =
    "This request was refused because it did not come from this site. \
     Sign-in and credential requests must be sent by the platform's own pages.";
/// Go `CredentialCSRF`'s 415 message.
pub const SIGN_IN_UNSUPPORTED_MEDIA_TYPE_MESSAGE: &str =
    "This endpoint accepts only a JSON body (Content-Type: application/json).";

/// Why the guard refuses a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 403 `CROSS_ORIGIN_REQUEST`.
    CrossOrigin,
    /// 415 `UNSUPPORTED_MEDIA_TYPE`.
    UnsupportedMediaType,
    /// 403 `CROSS_ORIGIN_REQUEST` on a sign-in route.
    SignInCrossOrigin,
    /// 415 `UNSUPPORTED_MEDIA_TYPE` on a sign-in route.
    SignInUnsupportedMediaType,
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::CrossOrigin => (
                StatusCode::FORBIDDEN,
                CODE_CROSS_ORIGIN_REQUEST,
                CROSS_ORIGIN_MESSAGE,
            ),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                CODE_UNSUPPORTED_MEDIA_TYPE,
                UNSUPPORTED_MEDIA_TYPE_MESSAGE,
            ),
            Self::SignInCrossOrigin => (
                StatusCode::FORBIDDEN,
                CODE_CROSS_ORIGIN_REQUEST,
                SIGN_IN_CROSS_ORIGIN_MESSAGE,
            ),
            Self::SignInUnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                CODE_UNSUPPORTED_MEDIA_TYPE,
                SIGN_IN_UNSUPPORTED_MEDIA_TYPE_MESSAGE,
            ),
        };
        (
            status,
            [(CACHE_CONTROL, HeaderValue::from_static("no-store"))],
            Json(ApiError::new(code, message)),
        )
            .into_response()
    }
}

/// The guard's configuration: the origins trusted besides the request's own
/// host.
#[derive(Debug, Clone, Default)]
pub struct SessionCsrf {
    trusted_origins: Vec<String>,
}

impl SessionCsrf {
    /// A guard trusting `origins` (each an http(s) URL; only its scheme and
    /// host are kept, Go `AddTrustedOrigin(u.Scheme + "://" + u.Host)`;
    /// empty entries are skipped). An entry that is not an http(s) URL with
    /// a host is an error (Go `SessionCSRF`).
    pub fn new(origins: &[&str]) -> Result<Self, String> {
        let mut trusted_origins = Vec::new();
        for o in origins.iter().filter(|o| !o.is_empty()) {
            let origin = origin_of(o)
                .ok_or_else(|| format!("trusted origin {o:?} is not an http(s) origin"))?;
            trusted_origins.push(origin);
        }
        Ok(Self { trusted_origins })
    }

    /// The guard of the platform at `base` (the public base URL, Go
    /// `publicOrigins`): `base`'s origin is trusted; a value that is not an
    /// http(s) URL is left out with a warning (cookie writes are then
    /// accepted from the request's own host only) rather than failing boot.
    pub fn for_public_base_url(base: &str) -> Self {
        match origin_of(base) {
            Some(origin) => Self {
                trusted_origins: vec![origin],
            },
            None => {
                tracing::warn!(
                    value = base,
                    "public base URL is not an http(s) URL; cookie-session posts are accepted from the request's own host only"
                );
                Self::default()
            }
        }
    }

    /// The rule for an unsafe request the session cookie authenticated
    /// (`has_body`: it carries, or may carry, a body).
    pub fn check(&self, headers: &HeaderMap, has_body: bool) -> Result<(), Refusal> {
        if !self.is_from_this_origin(headers, Provenance::Required) {
            return Err(Refusal::CrossOrigin);
        }
        if has_body && !is_json_media_type(header(headers, CONTENT_TYPE.as_str())) {
            return Err(Refusal::UnsupportedMediaType);
        }
        Ok(())
    }

    /// The rule for an unsafe request to a sign-in route ([`sign_in_routes`],
    /// Go `CredentialCSRF`), authenticated or not: refused when a browser
    /// says another origin sent it, but a request with neither
    /// `Sec-Fetch-Site` nor `Origin` passes (it did not come from a browser,
    /// and login CSRF needs the victim's browser); a body must be JSON for
    /// every caller.
    pub fn check_sign_in(&self, headers: &HeaderMap, has_body: bool) -> Result<(), Refusal> {
        if !self.is_from_this_origin(headers, Provenance::IfStated) {
            return Err(Refusal::SignInCrossOrigin);
        }
        if has_body && !is_json_media_type(header(headers, CONTENT_TYPE.as_str())) {
            return Err(Refusal::SignInUnsupportedMediaType);
        }
        Ok(())
    }

    /// Go `CrossOriginProtection.Check`, plus, when `provenance` is
    /// required, the neither-header refusal.
    fn is_from_this_origin(&self, headers: &HeaderMap, provenance: Provenance) -> bool {
        let origin = header(headers, "origin");
        let trusted = || origin.is_some_and(|o| self.trusted_origins.iter().any(|t| t == o));
        match header(headers, "sec-fetch-site") {
            Some("same-origin" | "none") => true,
            Some(_) => trusted(),
            None => match origin {
                None => provenance == Provenance::IfStated,
                Some(o) => {
                    let host = header(headers, HOST.as_str());
                    host.is_some_and(|h| !h.is_empty() && origin_host(o) == Some(h)) || trusted()
                }
            },
        }
    }
}

/// Whether a request must state where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provenance {
    /// Neither `Sec-Fetch-Site` nor `Origin` is a refusal (the cookie rule).
    Required,
    /// Checked when stated; neither header passes (the sign-in rule).
    IfStated,
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// `scheme://host[:port]` of an http(s) URL with a host (Go: `url.Parse`,
/// then `u.Scheme + "://" + u.Host`; the scheme lower-cased, the host as
/// written).
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = authority_host(rest)?;
    Some(format!("{scheme}://{host}"))
}

/// The host (with any port) of an `Origin` value (Go `url.Parse(origin).Host`).
fn origin_host(origin: &str) -> Option<&str> {
    authority_host(origin.split_once("://")?.1)
}

/// The authority of what follows `scheme://`, without user info; `None`
/// when empty.
fn authority_host(rest: &str) -> Option<&str> {
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    (!host.is_empty()).then_some(host)
}

/// `application/json` or `application/*+json`, the media type parsed as Go's
/// `mime.ParseMediaType` does (a malformed value is not JSON).
pub fn is_json_media_type(content_type: Option<&str>) -> bool {
    let Some(ct) = content_type else {
        return false;
    };
    let Some(media_type) = parse_media_type(ct) else {
        return false;
    };
    media_type == "application/json"
        || (media_type.starts_with("application/") && media_type.ends_with("+json"))
}

/// RFC 2045 `token` characters (Go `isTokenChar`).
fn is_token_char(c: char) -> bool {
    c.is_ascii() && !c.is_ascii_control() && c != ' ' && !"()<>@,;:\\\"/[]?=".contains(c)
}

/// Go `mime.ParseMediaType`'s media type, lower-cased, or `None` when the
/// value (its parameters included) is malformed.
fn parse_media_type(value: &str) -> Option<String> {
    let (media_type, mut params) = value.split_once(';').unwrap_or((value, ""));
    let media_type = media_type.trim();
    let (typ, subtype) = match media_type.split_once('/') {
        Some((t, s)) => (t, Some(s)),
        None => (media_type, None),
    };
    let token = |s: &str| !s.is_empty() && s.chars().all(is_token_char);
    if !token(typ) || subtype.is_some_and(|s| !token(s)) {
        return None;
    }
    // Parameters: `; key=value`, value a token or a quoted string; a
    // trailing `;` is allowed; a key twice is an error.
    let mut seen: Vec<String> = Vec::new();
    loop {
        params = params.trim_start();
        if params.is_empty() || params == ";" {
            break;
        }
        params = params.strip_prefix(';').unwrap_or(params).trim_start();
        if params.is_empty() {
            break;
        }
        let key_end = params
            .find(|c: char| !is_token_char(c))
            .unwrap_or(params.len());
        let key = &params[..key_end];
        let rest = params[key_end..].trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        if key.is_empty() {
            return None;
        }
        let rest = if let Some(quoted) = rest.strip_prefix('"') {
            let mut escaped = false;
            let mut close = None;
            for (i, c) in quoted.char_indices() {
                match c {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => {
                        close = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            &quoted[close? + 1..]
        } else {
            let end = rest.find(|c: char| !is_token_char(c)).unwrap_or(rest.len());
            if end == 0 {
                return None;
            }
            &rest[end..]
        };
        let key = key.to_ascii_lowercase();
        if seen.contains(&key) {
            return None;
        }
        seen.push(key);
        params = rest.trim_start();
        if !params.is_empty() && !params.starts_with(';') {
            return None;
        }
    }
    Some(media_type.to_ascii_lowercase())
}

/// Whether the request carries, or may carry, a body (Go `hasBody`: a
/// non-zero or unknown length).
fn has_body(request: &Request) -> bool {
    if let Some(length) = header(request.headers(), CONTENT_LENGTH.as_str()) {
        return length.trim() != "0";
    }
    if request.headers().contains_key(TRANSFER_ENCODING) {
        return true;
    }
    let body = request.body();
    !body.is_end_stream() && body.size_hint().exact() != Some(0)
}

fn safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// The routes Go mounts outside its authenticated route group, which the
/// guard therefore never sees there (internal/server/wire_public.go): they
/// authenticate their own callers, or sign a browser in. Every other
/// platform route is in the group.
pub fn outside_the_authenticated_group(path: &str) -> bool {
    path.starts_with("/oauth/")
        || path.starts_with("/.well-known/")
        || matches!(
            path,
            // Go loginEP.RegisterPublicRoutes
            "/auth/check-domain"
                | "/auth/login"
                | "/auth/logout"
                | "/auth/refresh"
                // Go RegisterTwoFactorRoutes: the sign-in's second step
                | "/auth/2fa/verify"
                | "/auth/2fa/challenge/email"
                | "/auth/2fa/enroll/totp/begin"
                | "/auth/2fa/enroll/totp/confirm"
                | "/auth/2fa/enroll/email/begin"
                | "/auth/2fa/enroll/email/confirm"
                // Go passwordresetapi.RegisterRoutes
                | "/auth/password-reset/request"
                | "/auth/password-reset/confirm"
                | "/auth/password-setup/request"
                // The router's dispatch callbacks (HMAC job tokens)
                | "/api/dispatch/process"
                | "/api/dispatch/settled"
        )
}

/// What protects a public unsafe route: every POST, PUT, PATCH and DELETE
/// outside `/api` and `/bff` (and the public `/api/dispatch/*` callbacks)
/// has a decision in [`PUBLIC_UNSAFE_ROUTES`] (Go
/// `TestEveryPublicUnsafeRouteIsDecided`; pinned against the route table
/// by `route_table_snapshot_test`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicRouteDecision {
    /// JSON only and refused cross-origin, for every caller
    /// ([`SessionCsrf::check_sign_in`]): it signs a browser in or sets its
    /// credentials.
    SignIn,
    /// Authenticated: the cookie rule ([`SessionCsrf::check`]) covers the
    /// session cookie; a bearer is exempt.
    CookieSession,
    /// Left as it is, for the reason given.
    Left,
}

/// Every public unsafe route, its decision and why (Go F16 follow-up,
/// decision 24). Paths as the router registers them.
pub const PUBLIC_UNSAFE_ROUTES: &[(&str, &str, PublicRouteDecision, &str)] = {
    use PublicRouteDecision::{CookieSession, Left, SignIn};
    &[
        ("POST", "/auth/login", SignIn, "mints fc_session"),
        ("POST", "/auth/2fa/verify", SignIn, "finishes a login, mints fc_session"),
        ("POST", "/auth/2fa/enroll/totp/confirm", SignIn, "finishes a login, mints fc_session"),
        ("POST", "/auth/2fa/enroll/email/confirm", SignIn, "finishes a login, mints fc_session"),
        ("POST", "/auth/2fa/challenge/email", SignIn, "a step of the same flow; sends mail"),
        ("POST", "/auth/2fa/enroll/totp/begin", SignIn, "a step of the same flow; a factor secret"),
        ("POST", "/auth/2fa/enroll/email/begin", SignIn, "a step of the same flow; sends mail"),
        ("POST", "/auth/webauthn/authenticate/begin", SignIn, "passkey sign-in"),
        ("POST", "/auth/webauthn/authenticate/complete", SignIn, "passkey sign-in mints fc_session"),
        ("POST", "/auth/password-reset/confirm", SignIn, "sets a password; an INVITE confirm mints fc_session"),
        ("POST", "/auth/password-reset/request", SignIn, "mints a token, sends mail"),
        ("POST", "/auth/password-setup/request", SignIn, "mints a token, sends mail"),
        ("POST", "/portal/auth/login", SignIn, "portal password login (a code redirect in JSON, no cookie)"),
        ("POST", "/portal/auth/password-reset", SignIn, "portal reset request, sends mail"),
        ("POST", "/portal/auth/check-domain", SignIn, "guarded with the rest of the portal flow, as Go's one registration"),
        ("POST", "/auth/check-domain", Left, "read-only lookup"),
        ("POST", "/auth/logout", Left, "issues nothing; GET /auth/oidc/session/end clears the cookie by design"),
        ("POST", "/auth/refresh", Left, "refresh token in the body, tokens in the JSON body, no cookie"),
        ("POST", "/auth/oidc/interaction/{uid}/login", Left, "Rust-only and inert (nothing creates an interaction state); typed JSON body; answers an IdP URL, sets no session, and the callback requires the start's binding cookie"),
        ("POST", "/oauth/token", Left, "form-encoded by the spec, client-authenticated; no password grant; sets no cookie"),
        ("POST", "/oauth/introspect", Left, "form-encoded, client-authenticated"),
        ("POST", "/oauth/revoke", Left, "form-encoded, client-authenticated"),
        ("POST", "/oauth/userinfo", Left, "bearer access token"),
        ("POST", "/auth/change-password", CookieSession, "authenticated"),
        ("POST", "/auth/change-password/send-email-code", CookieSession, "authenticated"),
        ("POST", "/auth/client/switch", CookieSession, "authenticated"),
        ("POST", "/auth/2fa/methods/totp/begin", CookieSession, "authenticated"),
        ("POST", "/auth/2fa/methods/totp/confirm", CookieSession, "authenticated"),
        ("POST", "/auth/2fa/methods/email/begin", CookieSession, "authenticated"),
        ("POST", "/auth/2fa/methods/email/confirm", CookieSession, "authenticated"),
        ("DELETE", "/auth/2fa/methods/{method}", CookieSession, "authenticated"),
        ("POST", "/auth/2fa/recovery-codes/regenerate", CookieSession, "authenticated"),
        ("DELETE", "/auth/2fa/trusted-devices/{id}", CookieSession, "authenticated"),
        ("POST", "/auth/webauthn/register/begin", CookieSession, "authenticated"),
        ("POST", "/auth/webauthn/register/complete", CookieSession, "authenticated"),
        ("DELETE", "/auth/webauthn/credentials/{id}", CookieSession, "authenticated"),
        ("POST", "/control/functions/events", Left, "function runner bearer"),
        ("POST", "/control/functions/heartbeat", Left, "function runner bearer"),
        ("POST", "/api/dispatch/process", Left, "HMAC job token"),
        ("POST", "/api/dispatch/settled", Left, "HMAC job token"),
    ]
};

/// The sign-in routes ([`PublicRouteDecision::SignIn`]), all POST.
pub fn sign_in_routes() -> Vec<&'static str> {
    PUBLIC_UNSAFE_ROUTES
        .iter()
        .filter(|(_, _, decision, _)| *decision == PublicRouteDecision::SignIn)
        .map(|(_, path, _, _)| *path)
        .collect()
}

fn is_sign_in_route(method: &Method, path: &str) -> bool {
    PUBLIC_UNSAFE_ROUTES.iter().any(|(m, p, decision, _)| {
        *decision == PublicRouteDecision::SignIn && *p == path && method.as_str() == *m
    })
}

/// The guard, as an axum middleware. Mount it inside the layer that
/// installs the auth services (`AuthLayer`) and outside the profile-only
/// gate (Go runs it right after the Authenticator, before that gate, so a
/// refused request gets the same answer whatever the caller's roles).
pub async fn session_csrf(
    State(guard): State<Arc<SessionCsrf>>,
    request: Request,
    next: Next,
) -> Response {
    if safe_method(request.method()) {
        return next.run(request).await;
    }
    let path = request.uri().path();
    let sign_in = is_sign_in_route(request.method(), path);
    // Only a request presenting the cookie, and no Authorization header
    // (which decides on its own), can be cookie-authenticated.
    let cookie_candidate = !outside_the_authenticated_group(path)
        && !request.headers().contains_key(AUTHORIZATION)
        && extract_session_cookie(request.headers()).is_some();
    if !sign_in && !cookie_candidate {
        return next.run(request).await;
    }
    let body_present = has_body(&request);
    let (mut parts, body) = request.into_parts();
    if cookie_candidate {
        // Authenticates once; the handler's extractor reuses the context.
        let by_cookie = resolve_context(&mut parts)
            .await
            .is_some_and(|context| context.via_session_cookie());
        if by_cookie {
            if let Err(refusal) = guard.check(&parts.headers, body_present) {
                return refuse(&parts, refusal);
            }
        }
    }
    // A sign-in route (Go `CredentialCSRF`), whoever calls it.
    if sign_in {
        if let Err(refusal) = guard.check_sign_in(&parts.headers, body_present) {
            return refuse(&parts, refusal);
        }
    }
    next.run(Request::from_parts(parts, body)).await
}

/// The refusal, a cross-origin one logged (warn) with what the browser said.
fn refuse(parts: &Parts, refusal: Refusal) -> Response {
    let what = match refusal {
        Refusal::CrossOrigin => Some("cookie-session request refused: not from this origin"),
        Refusal::SignInCrossOrigin => Some("sign-in request refused: not from this origin"),
        Refusal::UnsupportedMediaType | Refusal::SignInUnsupportedMediaType => None,
    };
    if let Some(what) = what {
        tracing::warn!(
            method = %parts.method,
            path = parts.uri.path(),
            origin = header(&parts.headers, "origin").unwrap_or(""),
            sec_fetch_site = header(&parts.headers, "sec-fetch-site").unwrap_or(""),
            "{what}"
        );
    }
    refusal.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SELF: &str = "https://platform.example.com";

    struct Case {
        name: &'static str,
        host: &'static str,
        sec_fetch_site: Option<&'static str>,
        origin: Option<&'static str>,
        content_type: Option<&'static str>,
        body: bool,
        want: Result<(), Refusal>,
    }

    fn case(name: &'static str) -> Case {
        Case {
            name,
            host: "platform.example.com",
            sec_fetch_site: None,
            origin: None,
            content_type: None,
            body: false,
            want: Ok(()),
        }
    }

    /// Go `TestSessionCSRF`'s cookie-authenticated cases (the bearer and
    /// safe-method cases are the middleware's, in the integration tests).
    #[test]
    fn the_rule_matrix() {
        use Refusal::{CrossOrigin, UnsupportedMediaType};
        let json = Some("application/json");
        let cases = [
            Case {
                sec_fetch_site: Some("same-origin"),
                origin: Some(SELF),
                content_type: json,
                body: true,
                ..case("same-origin")
            },
            Case {
                sec_fetch_site: Some("none"),
                ..case("user-initiated")
            },
            Case {
                sec_fetch_site: Some("same-site"),
                origin: Some("https://evil.example.com"),
                content_type: json,
                body: true,
                want: Err(CrossOrigin),
                ..case("same-site sibling")
            },
            Case {
                sec_fetch_site: Some("cross-site"),
                origin: Some("https://attacker.test"),
                want: Err(CrossOrigin),
                ..case("cross-site")
            },
            Case {
                origin: Some("http://platform.example.com"),
                content_type: json,
                body: true,
                ..case("old browser, Origin matches Host")
            },
            Case {
                origin: Some("https://evil.example.com"),
                content_type: json,
                body: true,
                want: Err(CrossOrigin),
                ..case("old browser, foreign Origin")
            },
            Case {
                origin: Some("https://platform.example.com.evil.test"),
                want: Err(CrossOrigin),
                ..case("old browser, Origin extends the host")
            },
            Case {
                content_type: json,
                body: true,
                want: Err(CrossOrigin),
                ..case("neither header")
            },
            Case {
                origin: Some("null"),
                content_type: json,
                body: true,
                want: Err(CrossOrigin),
                ..case("Origin null")
            },
            Case {
                host: "internal:8080",
                origin: Some(SELF),
                content_type: json,
                body: true,
                ..case("rewritten Host, trusted public Origin")
            },
            Case {
                host: "internal:8080",
                sec_fetch_site: Some("same-site"),
                origin: Some(SELF),
                content_type: json,
                body: true,
                ..case("rewritten Host, trusted public Origin, cross-site fetch metadata")
            },
            Case {
                host: "internal:8080",
                origin: Some("http://platform.example.com"),
                want: Err(CrossOrigin),
                ..case("rewritten Host, trusted host under another scheme")
            },
            Case {
                sec_fetch_site: Some("cross-site"),
                body: true,
                content_type: Some("text/plain"),
                want: Err(CrossOrigin),
                ..case("cross-origin with a non-JSON body: 403 first")
            },
            // JSON only.
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("text/plain;charset=UTF-8"),
                body: true,
                want: Err(UnsupportedMediaType),
                ..case("text/plain body")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("application/x-www-form-urlencoded"),
                body: true,
                want: Err(UnsupportedMediaType),
                ..case("form body")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("multipart/form-data; boundary=x"),
                body: true,
                want: Err(UnsupportedMediaType),
                ..case("multipart body")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                body: true,
                want: Err(UnsupportedMediaType),
                ..case("body without Content-Type")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("application/json; ="),
                body: true,
                want: Err(UnsupportedMediaType),
                ..case("malformed Content-Type")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("application/wasm"),
                body: true,
                want: Err(UnsupportedMediaType),
                ..case("cookie wasm upload")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("application/json; charset=utf-8"),
                body: true,
                ..case("json with charset")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("Application/JSON"),
                body: true,
                ..case("json in capitals")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("application/merge-patch+json"),
                body: true,
                ..case("merge-patch+json")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                ..case("no body, no Content-Type")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("text/plain"),
                ..case("no body, text/plain declared")
            },
        ];
        let guard = SessionCsrf::new(&["https://platform.example.com/"]).unwrap();
        for c in cases {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, HeaderValue::from_static(c.host));
            if let Some(v) = c.sec_fetch_site {
                headers.insert("sec-fetch-site", HeaderValue::from_static(v));
            }
            if let Some(v) = c.origin {
                headers.insert("origin", HeaderValue::from_static(v));
            }
            if let Some(v) = c.content_type {
                headers.insert(CONTENT_TYPE, HeaderValue::from_static(v));
            }
            assert_eq!(guard.check(&headers, c.body), c.want, "{}", c.name);
        }
    }

    /// Go `TestCredentialCSRF`: the same provenance check, except that a
    /// request with neither header passes, and JSON only for every caller.
    #[test]
    fn the_sign_in_rule_matrix() {
        use Refusal::{SignInCrossOrigin, SignInUnsupportedMediaType};
        let json = Some("application/json");
        let cases = [
            Case {
                sec_fetch_site: Some("same-origin"),
                origin: Some(SELF),
                content_type: json,
                body: true,
                ..case("same-origin")
            },
            Case {
                sec_fetch_site: Some("none"),
                content_type: json,
                body: true,
                ..case("user-initiated")
            },
            Case {
                sec_fetch_site: Some("cross-site"),
                origin: Some("https://attacker.test"),
                content_type: Some("text/plain"),
                body: true,
                want: Err(SignInCrossOrigin),
                ..case("cross-site text/plain (the form attack)")
            },
            Case {
                sec_fetch_site: Some("same-site"),
                origin: Some("https://evil.example.com"),
                content_type: json,
                body: true,
                want: Err(SignInCrossOrigin),
                ..case("same-site sibling")
            },
            Case {
                origin: Some("https://evil.example.com"),
                content_type: Some("text/plain"),
                body: true,
                want: Err(SignInCrossOrigin),
                ..case("old browser, foreign Origin")
            },
            Case {
                origin: Some("http://platform.example.com"),
                content_type: json,
                body: true,
                ..case("old browser, Origin matches Host")
            },
            Case {
                origin: Some("null"),
                content_type: json,
                body: true,
                want: Err(SignInCrossOrigin),
                ..case("Origin null")
            },
            Case {
                host: "internal:8080",
                origin: Some(SELF),
                content_type: json,
                body: true,
                ..case("rewritten Host, trusted public Origin")
            },
            Case {
                content_type: json,
                body: true,
                ..case("neither header: a non-browser caller passes")
            },
            Case {
                content_type: Some("application/x-www-form-urlencoded"),
                body: true,
                want: Err(SignInUnsupportedMediaType),
                ..case("non-browser form body")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("text/plain;charset=UTF-8"),
                body: true,
                want: Err(SignInUnsupportedMediaType),
                ..case("same-origin text/plain")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                content_type: Some("multipart/form-data; boundary=x"),
                body: true,
                want: Err(SignInUnsupportedMediaType),
                ..case("same-origin multipart")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                body: true,
                want: Err(SignInUnsupportedMediaType),
                ..case("body without Content-Type")
            },
            Case {
                content_type: Some("application/json; ="),
                body: true,
                want: Err(SignInUnsupportedMediaType),
                ..case("malformed Content-Type")
            },
            Case {
                content_type: Some("application/vnd.fc+json"),
                body: true,
                ..case("+json")
            },
            Case {
                sec_fetch_site: Some("same-origin"),
                ..case("no body, no Content-Type")
            },
            Case {
                sec_fetch_site: Some("cross-site"),
                origin: Some("https://attacker.test"),
                content_type: Some("application/x-www-form-urlencoded"),
                body: true,
                want: Err(SignInCrossOrigin),
                ..case("cross-origin and not JSON: provenance first")
            },
        ];
        let guard = SessionCsrf::new(&["https://platform.example.com/"]).unwrap();
        for c in cases {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, HeaderValue::from_static(c.host));
            if let Some(v) = c.sec_fetch_site {
                headers.insert("sec-fetch-site", HeaderValue::from_static(v));
            }
            if let Some(v) = c.origin {
                headers.insert("origin", HeaderValue::from_static(v));
            }
            if let Some(v) = c.content_type {
                headers.insert(CONTENT_TYPE, HeaderValue::from_static(v));
            }
            assert_eq!(guard.check_sign_in(&headers, c.body), c.want, "{}", c.name);
        }
    }

    #[test]
    fn the_sign_in_routes_are_the_routes_decided_sign_in() {
        assert_eq!(sign_in_routes().len(), 15);
        assert!(is_sign_in_route(&Method::POST, "/auth/login"));
        assert!(!is_sign_in_route(&Method::PUT, "/auth/login"));
        assert!(!is_sign_in_route(&Method::POST, "/auth/logout"));
        assert!(!is_sign_in_route(&Method::POST, "/auth/login/"));
    }

    /// Go `TestSessionCSRFRejectsBadTrustedOrigin`.
    #[test]
    fn a_trusted_origin_must_be_an_http_origin() {
        for o in ["platform.example.com", "ftp://x", "https://"] {
            assert!(SessionCsrf::new(&[o]).is_err(), "{o}");
        }
        let guard = SessionCsrf::new(&["", "https://ok.example.com/path"]).unwrap();
        assert_eq!(guard.trusted_origins, vec!["https://ok.example.com"]);
        assert!(SessionCsrf::for_public_base_url("flowcatalyst-test")
            .trusted_origins
            .is_empty());
        assert_eq!(
            SessionCsrf::for_public_base_url("HTTPS://app.example.com:8443/base").trusted_origins,
            vec!["https://app.example.com:8443"]
        );
    }

    #[test]
    fn media_types_parse_as_go_parses_them() {
        for ok in [
            "application/json",
            "application/json;",
            "application/json ; charset=utf-8",
            "application/json; charset=\"utf-8\"",
            "application/vnd.api+json",
            "application/problem+json; q=1",
        ] {
            assert!(is_json_media_type(Some(ok)), "{ok}");
        }
        for bad in [
            "",
            "json",
            "text/json",
            "application/json; charset",
            "application/json; charset=a; charset=b",
            "application/json, text/plain",
            "application/json; a=\"unterminated",
            "application/jsonx",
            "text/plain",
        ] {
            assert!(!is_json_media_type(Some(bad)), "{bad}");
        }
        assert!(!is_json_media_type(None));
    }

    #[test]
    fn the_public_routes_are_outside_the_group_and_nothing_else() {
        for p in [
            "/oauth/token",
            "/.well-known/openid-configuration",
            "/auth/login",
            "/auth/logout",
            "/auth/2fa/verify",
            "/auth/password-reset/confirm",
            "/api/dispatch/process",
            "/api/dispatch/settled",
        ] {
            assert!(outside_the_authenticated_group(p), "{p}");
        }
        for p in [
            "/api/clients",
            "/bff/roles/permissions",
            "/auth/change-password",
            "/auth/2fa/methods/totp/begin",
            "/auth/client/switch",
            "/auth/webauthn/register/begin",
            "/auth/oidc/interaction/x/login",
            "/portal/auth/login",
            "/api/dispatch/router-config",
            "/auth/login/x",
            "/oauth",
        ] {
            assert!(!outside_the_authenticated_group(p), "{p}");
        }
    }
}

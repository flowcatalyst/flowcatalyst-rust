//! Browser binding of an OIDC login state (Go bridge `login_binding.go`,
//! docs F16 follow-up: SSO login CSRF).
//!
//! The login state lives in the database, so on its own a callback URL
//! (state + code) completes in any browser: someone with an account at any
//! mapped identity provider could start a login, sign in at the IdP, stop
//! before the callback and send the link to a victim, whose browser would be
//! signed in to the attacker's account. Every start therefore sets a cookie
//! in the browser that started it, and the callback requires it.
//!
//! One cookie per login (the name carries a digest of the state), so two
//! sign-ins in two tabs do not overwrite each other's binding. Over HTTPS the
//! name takes the `__Host-` prefix: the browser then accepts it only with
//! `Secure`, `Path=/` and no `Domain`, so a sibling subdomain cannot plant
//! one for the platform's host (cookie tossing). `SameSite=Lax`: the IdP's
//! redirect back is a top-level GET navigation, on which Lax cookies are
//! sent.

use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// The binding's lifetime: the login state's (ten minutes).
pub const LOGIN_BINDING_TTL_SECONDS: i64 = 600;

/// The refusal's code.
pub const LOGIN_NOT_STARTED_HERE: &str = "LOGIN_NOT_STARTED_HERE";
/// Go's refusal message.
pub const LOGIN_NOT_STARTED_HERE_MESSAGE: &str =
    "This sign-in was not started in this browser. Start the sign-in again.";

/// The binding cookie's name for `state`: `fc_sso_` and the first 8 bytes of
/// the state's SHA-256 in hex, `__Host-`-prefixed when `secure`.
pub fn cookie_name(state: &str, secure: bool) -> String {
    let digest = Sha256::digest(state.as_bytes());
    let name = format!("fc_sso_{}", hex::encode(&digest[..8]));
    if secure {
        format!("__Host-{name}")
    } else {
        name
    }
}

fn cookie(state: &str, secure: bool, value: String, max_age: time::Duration) -> Cookie<'static> {
    Cookie::build((cookie_name(state, secure), value))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(max_age)
        .build()
}

/// The cookie a login start sets for `state`.
pub fn binding_cookie(state: &str, secure: bool) -> Cookie<'static> {
    cookie(
        state,
        secure,
        state.to_string(),
        time::Duration::seconds(LOGIN_BINDING_TTL_SECONDS),
    )
}

/// The cookie that expires the binding for `state`.
pub fn clear_binding_cookie(state: &str, secure: bool) -> Cookie<'static> {
    cookie(state, secure, String::new(), time::Duration::ZERO)
}

/// Whether the request's cookies hold the binding cookie for `state`, with
/// the state as its value (compared in constant time).
pub fn bound_to_browser(cookies: &CookieJar, state: &str, secure: bool) -> bool {
    cookies
        .get(&cookie_name(state, secure))
        .is_some_and(|c| bool::from(c.value().as_bytes().ct_eq(state.as_bytes())))
}

/// Expire the binding for `state` on `response` (the callback clears it,
/// whatever its outcome).
pub fn clear_on(response: &mut Response, state: &str, secure: bool) {
    if let Ok(value) = HeaderValue::from_str(&clear_binding_cookie(state, secure).to_string()) {
        response.headers_mut().append(SET_COOKIE, value);
    }
}

/// The callback's refusal of a login started elsewhere: 400
/// `LOGIN_NOT_STARTED_HERE` (the caller clears the binding, [`clear_on`]).
pub fn not_started_here() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": LOGIN_NOT_STARTED_HERE,
            "message": LOGIN_NOT_STARTED_HERE_MESSAGE,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::COOKIE;
    use axum::http::HeaderMap;

    #[test]
    fn the_name_is_per_login_and_host_prefixed_over_https() {
        let digest = Sha256::digest(b"state-1");
        let plain = format!("fc_sso_{}", hex::encode(&digest[..8]));
        assert_eq!(cookie_name("state-1", false), plain);
        assert_eq!(cookie_name("state-1", true), format!("__Host-{plain}"));
        assert_ne!(cookie_name("state-1", false), cookie_name("state-2", false));
        assert_eq!(plain.len(), "fc_sso_".len() + 16);
    }

    #[test]
    fn the_cookie_is_http_only_lax_rooted_and_lives_ten_minutes() {
        let set = binding_cookie("s", true).to_string();
        for part in [
            "HttpOnly",
            "SameSite=Lax",
            "Secure",
            "Path=/",
            "Max-Age=600",
        ] {
            assert!(set.contains(part), "{set}");
        }
        assert!(!set.contains("Domain"), "{set}");
        assert!(
            set.starts_with(&format!("{}=s;", cookie_name("s", true))),
            "{set}"
        );
        let plain = binding_cookie("s", false).to_string();
        assert!(
            !plain.contains("Secure") && plain.starts_with("fc_sso_"),
            "{plain}"
        );
        let cleared = clear_binding_cookie("s", true).to_string();
        assert!(cleared.contains("Max-Age=0"), "{cleared}");
    }

    #[test]
    fn only_the_logins_own_cookie_and_value_bind() {
        let headers = |cookie: String| {
            let mut h = HeaderMap::new();
            h.insert(COOKIE, HeaderValue::from_str(&cookie).unwrap());
            CookieJar::from_headers(&h)
        };
        let name = cookie_name("s1", false);
        assert!(bound_to_browser(
            &headers(format!("a=b; {name}=s1")),
            "s1",
            false
        ));
        assert!(!bound_to_browser(
            &headers(format!("{name}=forged")),
            "s1",
            false
        ));
        let other = cookie_name("s2", false);
        assert!(!bound_to_browser(
            &headers(format!("{other}=s2")),
            "s1",
            false
        ));
        // Over HTTPS only the __Host- cookie counts.
        assert!(!bound_to_browser(
            &headers(format!("{name}=s1")),
            "s1",
            true
        ));
        assert!(!bound_to_browser(&CookieJar::new(), "s1", false));
    }
}

//! Session cookie settings shared by every handler that issues or clears
//! the platform session cookie (password login, OIDC login, passkeys).

use axum_extra::extract::cookie::{Cookie, SameSite};
use tracing::warn;

/// How the session cookie is named and flagged. Built once at startup.
#[derive(Debug, Clone)]
pub struct SessionCookieConfig {
    pub name: String,
    pub secure: bool,
    pub same_site: SameSite,
    /// Cookie `Max-Age`.
    pub ttl: time::Duration,
}

impl SessionCookieConfig {
    /// Parse a configured `SameSite` value. `strict`, `none` and `lax` are
    /// accepted in any case; anything else falls back to `Lax` with a warning.
    pub fn parse_same_site(value: &str) -> SameSite {
        match value.to_lowercase().as_str() {
            "strict" => SameSite::Strict,
            "none" => SameSite::None,
            "lax" => SameSite::Lax,
            _ => {
                warn!(
                    value,
                    "Unrecognised session cookie SameSite value; using Lax"
                );
                SameSite::Lax
            }
        }
    }

    /// The session cookie carrying `token`, with `Expires` as well as
    /// `Max-Age`, as Go's login sets it (auth/login/endpoint.go:576-585).
    pub fn build_cookie(&self, token: String) -> Cookie<'static> {
        Cookie::build((self.name.clone(), token))
            .path("/")
            .http_only(true)
            .secure(self.secure)
            .same_site(self.same_site)
            .max_age(self.ttl)
            .expires(time::OffsetDateTime::now_utc() + self.ttl)
            .build()
    }

    /// A cookie that clears the session cookie (empty value, `Max-Age=0`),
    /// with the same `Secure` and `SameSite` as the cookie it clears (Go
    /// `handleLogout`), so the browser treats it as that cookie.
    pub fn clear_cookie(&self) -> Cookie<'static> {
        Cookie::build((self.name.clone(), ""))
            .path("/")
            .http_only(true)
            .secure(self.secure)
            .same_site(self.same_site)
            .max_age(time::Duration::ZERO)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(secure: bool, same_site: &str) -> SessionCookieConfig {
        SessionCookieConfig {
            name: "fc_session".to_string(),
            secure,
            same_site: SessionCookieConfig::parse_same_site(same_site),
            ttl: time::Duration::seconds(86400),
        }
    }

    #[test]
    fn parse_same_site_accepts_any_case_and_falls_back_to_lax() {
        assert_eq!(
            SessionCookieConfig::parse_same_site("Strict"),
            SameSite::Strict
        );
        assert_eq!(SessionCookieConfig::parse_same_site("NONE"), SameSite::None);
        assert_eq!(SessionCookieConfig::parse_same_site("lax"), SameSite::Lax);
        assert_eq!(SessionCookieConfig::parse_same_site("bogus"), SameSite::Lax);
        assert_eq!(SessionCookieConfig::parse_same_site(""), SameSite::Lax);
    }

    /// The header without its `Expires` (a wall-clock time).
    fn without_expires(cookie: Cookie<'_>) -> String {
        let header = cookie.to_string();
        assert!(header.contains("; Expires="), "{header}");
        header
            .split("; ")
            .filter(|part| !part.starts_with("Expires="))
            .collect::<Vec<_>>()
            .join("; ")
    }

    #[test]
    fn build_cookie_header_is_stable() {
        assert_eq!(
            without_expires(config(true, "Lax").build_cookie("tok".into())),
            "fc_session=tok; HttpOnly; SameSite=Lax; Secure; Path=/; Max-Age=86400"
        );
        assert_eq!(
            without_expires(config(false, "strict").build_cookie("tok".into())),
            "fc_session=tok; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400"
        );
        assert_eq!(
            without_expires(config(true, "None").build_cookie("tok".into())),
            "fc_session=tok; HttpOnly; SameSite=None; Secure; Path=/; Max-Age=86400"
        );
    }

    #[test]
    fn clear_cookie_header_is_stable() {
        assert_eq!(
            config(true, "Strict").clear_cookie().to_string(),
            "fc_session=; HttpOnly; SameSite=Strict; Secure; Path=/; Max-Age=0"
        );
    }
}

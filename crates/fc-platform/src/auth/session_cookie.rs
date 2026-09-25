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
    /// The platform session cookie as password login issues it: `fc_session`,
    /// `SameSite=Lax`, one day, `Secure` per deployment (on in fc-server, off
    /// only for fc-dev's plain-http localhost).
    pub fn password_login(secure: bool) -> Self {
        Self {
            name: crate::shared::middleware::SESSION_COOKIE_NAME.to_string(),
            secure,
            same_site: SameSite::Lax,
            ttl: time::Duration::seconds(86400),
        }
    }

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

    /// The session cookie carrying `token`.
    pub fn build_cookie(&self, token: String) -> Cookie<'static> {
        Cookie::build((self.name.clone(), token))
            .path("/")
            .http_only(true)
            .secure(self.secure)
            .same_site(self.same_site)
            .max_age(self.ttl)
            .build()
    }

    /// A cookie that clears the session cookie (empty value, `Max-Age=0`).
    pub fn clear_cookie(&self) -> Cookie<'static> {
        Cookie::build((self.name.clone(), ""))
            .path("/")
            .http_only(true)
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

    #[test]
    fn build_cookie_header_is_stable() {
        assert_eq!(
            config(true, "Lax").build_cookie("tok".into()).to_string(),
            "fc_session=tok; HttpOnly; SameSite=Lax; Secure; Path=/; Max-Age=86400"
        );
        assert_eq!(
            config(false, "strict")
                .build_cookie("tok".into())
                .to_string(),
            "fc_session=tok; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400"
        );
        assert_eq!(
            config(true, "None").build_cookie("tok".into()).to_string(),
            "fc_session=tok; HttpOnly; SameSite=None; Secure; Path=/; Max-Age=86400"
        );
    }

    #[test]
    fn clear_cookie_header_is_stable() {
        assert_eq!(
            config(true, "Strict").clear_cookie().to_string(),
            "fc_session=; HttpOnly; Path=/; Max-Age=0"
        );
    }
}

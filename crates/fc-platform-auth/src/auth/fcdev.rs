//! The built-in first-party OAuth client behind `fcdev login` (Go F25,
//! `internal/platform/auth/fcdev_client.go`, `oauthapi/redirecturi.go`).
//!
//! A PUBLIC client (no secret) that lets a person sign in to a remote platform
//! from the CLI with the normal browser login and then act as themselves. It
//! is seeded at every start (`fc_platform::shared::database::seed_fcdev_cli_client`)
//! and a few of its rules live in code rather than in the editable row,
//! because they are security properties of THIS client and must not drift with
//! console edits:
//!
//! * redirect matching: the registered URIs carry no port, and only for this
//!   `client_id` does `/oauth/authorize` accept any loopback port on them
//!   ([`match_loopback_redirect_uri`], RFC 8252 section 7.3);
//! * refresh lifetime: an ABSOLUTE cap per family (rotation never extends it),
//!   independent of `OIDC_REFRESH_TOKEN_TTL`.

use std::env;

/// The `client_id` of the seeded client.
pub const FCDEV_CLI_CLIENT_ID: &str = "fcdev-cli";

/// Display name of the seeded client.
pub const FCDEV_CLI_CLIENT_NAME: &str = "fcdev CLI (browser login)";

/// The registered loopback redirects. Port-less on purpose; the port is chosen
/// by fcdev per login.
pub const FCDEV_CLI_REDIRECT_URIS: [&str; 2] =
    ["http://127.0.0.1/callback", "http://[::1]/callback"];

/// The only grants the client may use.
pub const FCDEV_CLI_GRANT_TYPES: [&str; 2] = ["authorization_code", "refresh_token"];

/// How long an fcdev login lasts before the person signs in again, counted from
/// the original sign-in (absolute, not sliding): 7 days.
pub const DEFAULT_FCDEV_CLI_REFRESH_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// The env var that overrides [`DEFAULT_FCDEV_CLI_REFRESH_TTL_SECS`].
pub const FCDEV_REFRESH_TTL_ENV: &str = "FC_FCDEV_LOGIN_REFRESH_TTL_SECS";

/// The configured lifetime in seconds: `FC_FCDEV_LOGIN_REFRESH_TTL_SECS`, or
/// the default when it is unset, unparseable or not positive (Go `positiveOr`).
pub fn refresh_ttl_secs_from_env() -> i64 {
    parse_refresh_ttl(env::var(FCDEV_REFRESH_TTL_ENV).ok().as_deref())
}

fn parse_refresh_ttl(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_FCDEV_CLI_REFRESH_TTL_SECS)
}

/// The `/oauth/authorize` redirect check for one client: the caller's general
/// matcher result `general` for every client, plus the RFC 8252 loopback-port
/// exception for the built-in fcdev client ONLY (keyed on its fixed
/// `client_id`, so no other client, however it is registered in the console,
/// can use it).
pub fn redirect_allowed(
    client_id: &str,
    general: bool,
    redirect_uri: &str,
    registered: &[String],
) -> bool {
    general
        || (client_id == FCDEV_CLI_CLIENT_ID
            && match_loopback_redirect_uri(redirect_uri, registered))
}

/// The RFC 8252 section 7.3 loopback exception for ONE registered pattern
/// shape: a port-less `http://127.0.0.1/path` or `http://[::1]/path`. The
/// incoming `uri` matches when it is that same URI with an explicit port added
/// and nothing else.
///
/// Deliberately narrower than the general matcher and only ever consulted for
/// the built-in fcdev client ([`redirect_allowed`]):
///
/// * the host is literally `127.0.0.1` or `[::1]`, and the same literal is
///   registered: never `localhost`, never a wildcard, never any other host;
/// * the scheme is `http` (loopback has no certificate to protect);
/// * the port is required, canonical decimal, 1-65535;
/// * the path equals the registered path exactly;
/// * no userinfo, query, fragment or any other component.
///
/// Go closes this by rebuilding the URI from its parsed parts and demanding
/// byte equality with the input. Here the input is parsed positionally (no
/// URL library, so no parser differential to exploit): scheme prefix, host
/// literal, `:port`, then a remainder that must be byte-equal to the path of a
/// registered port-less URI for that same host.
pub fn match_loopback_redirect_uri(uri: &str, registered: &[String]) -> bool {
    let Some(rest) = uri.strip_prefix("http://") else {
        return false;
    };
    let (host, after_host) = if let Some(r) = rest.strip_prefix("127.0.0.1") {
        ("127.0.0.1", r)
    } else if let Some(r) = rest.strip_prefix("[::1]") {
        ("[::1]", r)
    } else {
        return false;
    };
    let Some(after_colon) = after_host.strip_prefix(':') else {
        return false;
    };
    let digits = after_colon
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let (port, path) = after_colon.split_at(digits);
    // Canonical decimal in 1..=65535: no leading zero, no sign, no empty port.
    if port.is_empty() || port.starts_with('0') || port.len() > 5 {
        return false;
    }
    match port.parse::<u32>() {
        Ok(n) if (1..=65535).contains(&n) => {}
        _ => return false,
    }
    // The path begins with `/`, is never empty, and carries only unreserved
    // characters and `/`: anything else (a query, fragment, backslash,
    // userinfo `@`, percent-escape, space, control byte) is refused outright,
    // before comparing it with what is registered.
    if !path.starts_with('/')
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~'))
    {
        return false;
    }
    let want = format!("http://{host}{path}");
    registered.contains(&want)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> Vec<String> {
        FCDEV_CLI_REDIRECT_URIS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    /// Go `TestMatchLoopbackRedirectURI`, case for case.
    #[test]
    fn loopback_redirect_matrix() {
        let cases: &[(&str, bool)] = &[
            ("http://127.0.0.1:54321/callback", true),
            ("http://127.0.0.1:1/callback", true),
            ("http://127.0.0.1:65535/callback", true),
            ("http://[::1]:8123/callback", true),
            // port is required, canonical and in range
            ("http://127.0.0.1/callback", false),
            ("http://127.0.0.1:0/callback", false),
            ("http://127.0.0.1:65536/callback", false),
            ("http://127.0.0.1:-1/callback", false),
            ("http://127.0.0.1:0080/callback", false),
            ("http://127.0.0.1:80a/callback", false),
            ("http://127.0.0.1:/callback", false),
            // host tricks
            ("http://127.0.0.1.evil.com:5000/callback", false),
            ("http://127.0.0.1.:5000/callback", false),
            ("http://evil.com:5000/callback", false),
            ("http://localhost:5000/callback", false),
            ("http://127.0.0.2:5000/callback", false),
            ("http://0.0.0.0:5000/callback", false),
            ("http://[::ffff:127.0.0.1]:5000/callback", false),
            ("http://[0:0:0:0:0:0:0:1]:5000/callback", false),
            ("http://127.1:5000/callback", false),
            ("http://2130706433:5000/callback", false),
            ("http://HTTP://127.0.0.1:5000/callback", false),
            // userinfo
            ("http://evil.com@127.0.0.1:5000/callback", false),
            ("http://127.0.0.1:5000@evil.com/callback", false),
            ("http://127.0.0.1@evil.com:5000/callback", false),
            ("http://user:pw@127.0.0.1:5000/callback", false),
            // scheme
            ("https://127.0.0.1:5000/callback", false),
            ("ftp://127.0.0.1:5000/callback", false),
            ("HTTP://127.0.0.1:5000/callback", false),
            // path variations
            ("http://127.0.0.1:5000/callback/", false),
            ("http://127.0.0.1:5000/callback/x", false),
            ("http://127.0.0.1:5000/other", false),
            ("http://127.0.0.1:5000/", false),
            ("http://127.0.0.1:5000", false),
            ("http://127.0.0.1:5000/callback/../callback", false),
            ("http://127.0.0.1:5000//callback", false),
            ("http://127.0.0.1:5000/%63allback", false),
            ("http://127.0.0.1:5000/Callback", false),
            ("http://127.0.0.1:5000/callback;x", false),
            // other components
            ("http://127.0.0.1:5000/callback?x=1", false),
            ("http://127.0.0.1:5000/callback?", false),
            ("http://127.0.0.1:5000/callback#f", false),
            ("http://127.0.0.1:5000/callback#", false),
            ("http://127.0.0.1:5000\\@evil.com/callback", false),
            ("http://127.0.0.1:5000/callback ", false),
            (" http://127.0.0.1:5000/callback", false),
            ("http://127.0.0.1:5000/call\nback", false),
            ("//127.0.0.1:5000/callback", false),
            ("", false),
        ];
        let reg = reg();
        for (uri, want) in cases {
            assert_eq!(
                match_loopback_redirect_uri(uri, &reg),
                *want,
                "match_loopback_redirect_uri({uri:?})"
            );
        }
        // Only the registered loopback literal(s) are variable-port.
        assert!(!match_loopback_redirect_uri(
            "http://[::1]:5000/callback",
            &["http://127.0.0.1/callback".to_string()]
        ));
        assert!(!match_loopback_redirect_uri(
            "http://127.0.0.1:5000/callback",
            &["http://localhost/callback".to_string()]
        ));
        // A registration that already pins a port enables nothing here.
        assert!(!match_loopback_redirect_uri(
            "http://127.0.0.1:5000/callback",
            &["http://127.0.0.1:5000/callback".to_string()]
        ));
    }

    /// Go `TestRedirectAllowedLoopbackIsFcdevOnly`: another client registered
    /// with the very same port-less loopback URIs gets exact matching only.
    #[test]
    fn loopback_variance_is_fcdev_only() {
        let reg = reg();
        let uri = "http://127.0.0.1:50000/callback";
        assert!(redirect_allowed(FCDEV_CLI_CLIENT_ID, false, uri, &reg));
        assert!(!redirect_allowed("someone-else", false, uri, &reg));
        assert!(!redirect_allowed(
            FCDEV_CLI_CLIENT_ID,
            false,
            "http://localhost:50000/callback",
            &reg
        ));
        assert!(!redirect_allowed(
            FCDEV_CLI_CLIENT_ID,
            false,
            "https://127.0.0.1:50000/callback",
            &reg
        ));
    }

    /// Go `envcfg_test.go`: unset, junk and non-positive mean the default.
    #[test]
    fn refresh_ttl_parsing() {
        assert_eq!(parse_refresh_ttl(None), 604_800);
        assert_eq!(parse_refresh_ttl(Some("")), 604_800);
        assert_eq!(parse_refresh_ttl(Some("abc")), 604_800);
        assert_eq!(parse_refresh_ttl(Some("0")), 604_800);
        assert_eq!(parse_refresh_ttl(Some("-5")), 604_800);
        assert_eq!(parse_refresh_ttl(Some("129600")), 129_600);
    }
}

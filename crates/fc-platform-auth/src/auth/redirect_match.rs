//! Redirect-URI matching for `/oauth/authorize` and the OIDC
//! `post_logout_redirect_uri` check (Go `oauthapi/redirecturi.go`,
//! `MatchRedirectURI`).
//!
//! The previous matcher treated the URI as a string: `*` was "any run without a
//! dot", so `https://evil%2Ecom/.example.com/callback` matched
//! `https://*.example.com/callback` while a browser sends the user (and the
//! authorization code) to `evil.com`. This matcher is URL-aware:
//!
//! * an exact string match always passes (as before);
//! * otherwise only registered patterns that contain `*` are considered, and
//!   the incoming URI is taken apart by hand (scheme, authority, path) and
//!   then cross-checked against the WHATWG `url` parser, which is what
//!   browsers do: they must agree on host, port and absence of userinfo;
//! * a URI is refused outright when its authority carries userinfo (`@`), a
//!   percent escape or anything outside DNS-host characters, or when the
//!   string carries a backslash, whitespace or control character anywhere;
//! * `*` in the host stands for part of ONE label (`*.x.com`, `qa-*.x.com`,
//!   `*-qa.x.com`), never crossing a dot, and the last two labels of the host
//!   pattern must be literal (so `https://*`, `https://*.com` and
//!   `https://app.example.*` match nothing);
//! * `*` as the whole port stands for any explicit port 1-65535 (today's
//!   `http://localhost:*/cb` keeps working);
//! * path: a pattern with no path or `/` matches any path; one ending `/*`
//!   matches that prefix; a `*` inside a segment matches part of one segment;
//!   otherwise equality. A path with a `..`/`.` segment or an encoded dot or
//!   slash or backslash never matches a wildcard pattern. The URI's query and
//!   fragment are ignored.

use url::Url;

/// True when `uri` is permitted by any of the registered redirect URIs.
pub(crate) fn matches_redirect_uri(uri: &str, registered: &[String]) -> bool {
    if registered.iter().any(|r| r == uri) {
        return true;
    }
    if !registered.iter().any(|r| r.contains('*')) {
        return false;
    }
    let Some(u) = ParsedUri::parse(uri) else {
        return false;
    };
    registered
        .iter()
        .filter(|p| p.contains('*'))
        .any(|p| u.matches_pattern(p))
}

/// An incoming URI that survived the strict parse.
struct ParsedUri<'a> {
    scheme: &'a str,
    /// Lower-cased DNS host (no brackets, no port).
    host: String,
    /// Effective port: explicit, or the scheme's default.
    port: Option<u16>,
    /// Whether the URI spelled the port out.
    explicit_port: bool,
    /// Raw path up to `?` or `#`; empty when absent.
    path: &'a str,
}

/// Split `scheme://authority/rest` without interpreting anything.
fn split_url(s: &str) -> Option<(&str, &str, &str)> {
    let (scheme, rest) = s.split_once("://")?;
    if scheme.is_empty()
        || !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let tail = &rest[end..];
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    Some((scheme, authority, &tail[..path_end]))
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

impl<'a> ParsedUri<'a> {
    fn parse(uri: &'a str) -> Option<Self> {
        if uri
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        {
            return None;
        }
        let (scheme, authority, path) = split_url(uri)?;
        // No userinfo, no escapes in the authority, DNS characters only.
        if authority.is_empty()
            || !authority
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
        {
            return None;
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => {
                if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) || p.len() > 5 {
                    return None;
                }
                (h, Some(p.parse::<u16>().ok()?))
            }
            None => (authority, None),
        };
        if host.is_empty() || host.contains(':') || host.starts_with('.') || host.ends_with('.') {
            return None;
        }
        let host = host.to_ascii_lowercase();

        // Cross-check against the WHATWG parser (the browser's view).
        let parsed = Url::parse(uri).ok()?;
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return None;
        }
        if parsed.host_str()?.to_ascii_lowercase() != host {
            return None;
        }
        let browser_port = parsed.port_or_known_default();
        let ours = port.or_else(|| default_port(scheme));
        if browser_port != ours {
            return None;
        }
        if !parsed.scheme().eq_ignore_ascii_case(scheme) {
            return None;
        }
        Some(Self {
            scheme,
            host,
            port: ours,
            explicit_port: port.is_some(),
            path,
        })
    }

    fn matches_pattern(&self, pattern: &str) -> bool {
        if pattern
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        {
            return false;
        }
        let Some((p_scheme, p_auth, p_path)) = split_url(pattern) else {
            return false;
        };
        if !self.scheme.eq_ignore_ascii_case(p_scheme) {
            return false;
        }
        if p_auth.contains('@')
            || !p_auth
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '*'))
        {
            return false;
        }
        let (p_host, p_port) = match p_auth.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (p_auth, None),
        };
        match p_port {
            Some("*") => {
                if !self.explicit_port || self.port.is_none_or(|p| p < 1) {
                    return false;
                }
            }
            Some(p) => match p.parse::<u16>() {
                Ok(n) if p.chars().all(|c| c.is_ascii_digit()) => {
                    if self.port != Some(n) {
                        return false;
                    }
                }
                _ => return false,
            },
            None => {
                if self.port != default_port(p_scheme) {
                    return false;
                }
            }
        }
        if !host_matches(&self.host, &p_host.to_ascii_lowercase()) {
            return false;
        }
        path_matches(self.path, p_path)
    }
}

/// Host equality or the label-confined wildcard.
fn host_matches(host: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return host == pattern;
    }
    let p_labels: Vec<&str> = pattern.split('.').collect();
    let h_labels: Vec<&str> = host.split('.').collect();
    // A concrete base of at least two labels: no `https://*`, `*.com`,
    // `app.example.*`.
    if p_labels.len() < 3 || p_labels.len() != h_labels.len() {
        return false;
    }
    if p_labels[p_labels.len() - 2..]
        .iter()
        .any(|l| l.contains('*'))
    {
        return false;
    }
    p_labels
        .iter()
        .zip(&h_labels)
        .all(|(p, h)| label_matches(h, p))
}

/// One dot-free label against a pattern that may contain `*`. Each `*` must
/// consume at least one character; the label must be non-empty.
fn label_matches(label: &str, pattern: &str) -> bool {
    if label.is_empty() {
        return false;
    }
    if !pattern.contains('*') {
        return label == pattern;
    }
    let segs: Vec<&str> = pattern.split('*').collect();
    let Some(mut rest) = label.strip_prefix(segs[0]) else {
        return false;
    };
    for (i, seg) in segs.iter().enumerate().skip(1) {
        if i == segs.len() - 1 {
            if seg.is_empty() {
                return !rest.is_empty();
            }
            return rest.ends_with(seg) && rest.len() > seg.len();
        }
        match rest.find(seg) {
            Some(idx) if idx >= 1 => rest = &rest[idx + seg.len()..],
            _ => return false,
        }
    }
    true
}

fn path_matches(uri_path: &str, pattern_path: &str) -> bool {
    // Dot segments and encoded separators are how a path escapes its prefix.
    if path_is_tricky(uri_path) {
        return false;
    }
    if pattern_path.is_empty() || pattern_path == "/" {
        return true;
    }
    if let Some(prefix) = pattern_path.strip_suffix('*') {
        if prefix.ends_with('/') && !prefix.contains('*') {
            return uri_path.starts_with(prefix);
        }
    }
    if !pattern_path.contains('*') {
        return uri_path == pattern_path;
    }
    // A `*` inside a segment stands for part of that one segment.
    let p: Vec<&str> = pattern_path.split('/').collect();
    let u: Vec<&str> = uri_path.split('/').collect();
    p.len() == u.len() && p.iter().zip(&u).all(|(ps, us)| label_matches_seg(us, ps))
}

fn label_matches_seg(seg: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return seg == pattern;
    }
    label_matches(seg, pattern)
}

fn path_is_tricky(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c")
        || path.split('/').any(|s| s == "." || s == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(uri: &str, registered: &[&str]) -> bool {
        let r: Vec<String> = registered.iter().map(|s| (*s).to_string()).collect();
        matches_redirect_uri(uri, &r)
    }

    /// Go's `TestMatchRedirectURI` matrix, row for row.
    #[test]
    fn go_matrix() {
        let cases: &[(&str, &str, &[&str], bool)] = &[
            (
                "exact match",
                "https://app.inhanceapps.com/done",
                &["https://app.inhanceapps.com/done"],
                true,
            ),
            (
                "no match",
                "https://evil.com/x",
                &["https://app.inhanceapps.com/done"],
                false,
            ),
            (
                "subdomain wildcard host-only matches bare",
                "https://app.inhanceapps.com",
                &["https://*.inhanceapps.com"],
                true,
            ),
            (
                "subdomain wildcard host-only matches path",
                "https://app.inhanceapps.com/auth/logged-out",
                &["https://*.inhanceapps.com"],
                true,
            ),
            (
                "subdomain wildcard different sub",
                "https://portal.inhanceapps.com/x",
                &["https://*.inhanceapps.com"],
                true,
            ),
            (
                "partial wildcard matches",
                "https://qa-acme.inhanceapps.com/done",
                &["https://qa-*.inhanceapps.com"],
                true,
            ),
            (
                "partial wildcard matches any path",
                "https://qa-acme.inhanceapps.com/x/y",
                &["https://qa-*.inhanceapps.com"],
                true,
            ),
            (
                "partial wildcard wrong prefix",
                "https://prod-acme.inhanceapps.com/x",
                &["https://qa-*.inhanceapps.com"],
                false,
            ),
            (
                "partial wildcard empty expansion rejected",
                "https://qa-.inhanceapps.com/x",
                &["https://qa-*.inhanceapps.com"],
                false,
            ),
            (
                "partial wildcard cannot cross dot",
                "https://qa-a.b.inhanceapps.com/x",
                &["https://qa-*.inhanceapps.com"],
                false,
            ),
            (
                "suffix-label wildcard",
                "https://acme-qa.inhanceapps.com/x",
                &["https://*-qa.inhanceapps.com"],
                true,
            ),
            (
                "bare base domain rejected",
                "https://inhanceapps.com/x",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "multi-label sub rejected",
                "https://a.b.inhanceapps.com/x",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "suffix-append attack",
                "https://inhanceapps.com.evil.com/x",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "prefix-append attack",
                "https://app.inhanceapps.com.evil.com/x",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "substring (no dot) attack",
                "https://evilinhanceapps.com/x",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "userinfo host confusion",
                "https://app.inhanceapps.com@evil.com/x",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "bare wildcard rejected",
                "https://anything.com",
                &["https://*"],
                false,
            ),
            (
                "wildcard on single-label base rejected",
                "https://x.com",
                &["https://*.com"],
                false,
            ),
            (
                "scheme downgrade rejected",
                "http://app.inhanceapps.com",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "port mismatch rejected",
                "https://app.inhanceapps.com:8443/x",
                &["https://*.inhanceapps.com"],
                false,
            ),
            (
                "exact path pattern matches",
                "https://app.inhanceapps.com/cb",
                &["https://*.inhanceapps.com/cb"],
                true,
            ),
            (
                "exact path pattern other path",
                "https://app.inhanceapps.com/other",
                &["https://*.inhanceapps.com/cb"],
                false,
            ),
            (
                "path-prefix wildcard",
                "https://app.inhanceapps.com/auth/done",
                &["https://*.inhanceapps.com/auth/*"],
                true,
            ),
            (
                "path-prefix wildcard miss",
                "https://app.inhanceapps.com/admin",
                &["https://*.inhanceapps.com/auth/*"],
                false,
            ),
        ];
        for (name, uri, reg, want) in cases {
            assert_eq!(m(uri, reg), *want, "{name}: {uri} vs {reg:?}");
        }
    }

    /// The review's reproduced bypass (R1) and its relatives. Each of these
    /// matched the old string matcher.
    #[test]
    fn encoded_and_confusing_hosts_never_match_a_wildcard() {
        let reg = ["https://*.example.com/callback"];
        for bad in [
            "https://evil%2Ecom/.example.com/callback",
            "https://evil%2ecom#.example.com/callback",
            "https://evil%2Ecom#.example.com/callback",
            "https://evil.com#.example.com/callback",
            "https://evil.com?.example.com/callback",
            "https://evil.com/.example.com/callback",
            "https://evil.com\\.example.com/callback",
            "https://evil.com\\@a.example.com/callback",
            "https://a.example.com@evil.com/callback",
            "https://user:pw@a.example.com/callback",
            "https://a.example.com%40evil.com/callback",
            "https://a%2Eexample.com/callback",
            "https://a.example.com./callback",
            "https://a.example.com:443@evil.com/callback",
            "https://a.example.com:/callback",
            "https://a.example.com:99999/callback",
            "https:///a.example.com/callback",
            " https://a.example.com/callback",
            "https://a.example.com/callback\n",
            "https://a.example.com/callback/../x",
            "https://a.example.com/%2e%2e/callback",
        ] {
            assert!(!m(bad, &reg), "must not match: {bad:?}");
        }
        // The legitimate neighbours still match.
        assert!(m("https://a.example.com/callback", &reg));
        assert!(m("https://a.example.com/callback?x=1#f", &reg));
        assert!(m("https://A.Example.COM/callback", &reg));
        assert!(m("HTTPS://a.example.com/callback", &reg));
        assert!(m("https://a.example.com:443/callback", &reg));
    }

    /// Patterns the previous matcher accepted and real deployments may hold.
    #[test]
    fn previous_legitimate_patterns_keep_matching() {
        // Path wildcard on a literal host.
        assert!(m("https://example.com/tenant1", &["https://example.com/*"]));
        // Wildcard in a non-leftmost label.
        assert!(m(
            "https://app.eu.example.com/cb",
            &["https://app.*.example.com/cb"]
        ));
        // Wildcard port on a literal host (development).
        assert!(m("http://localhost:3000/cb", &["http://localhost:*/cb"]));
        assert!(!m("http://localhost/cb", &["http://localhost:*/cb"]));
        assert!(!m(
            "http://localhost:3000/cb",
            &["http://localhost:4000/cb"]
        ));
        // Wildcard inside a path segment.
        assert!(m("https://x.com/t/acme/cb", &["https://x.com/t/*/cb"]));
        assert!(!m("https://x.com/t/a/b/cb", &["https://x.com/t/*/cb"]));
        // Custom scheme with a wildcard host.
        assert!(m("myapp://a.example.com/cb", &["myapp://*.example.com/cb"]));
        // Several registered, one wildcard.
        assert!(m(
            "https://t1.example.com/callback",
            &[
                "https://fixed.example.org/cb",
                "https://*.example.com/callback"
            ]
        ));
        // Dots in a wildcarded path are fine (the old matcher refused them).
        assert!(m("https://x.com/app/index.html", &["https://x.com/app/*"]));
    }

    #[test]
    fn wildcards_are_confined_to_the_subdomain_part() {
        assert!(!m(
            "https://app.example.evil/cb",
            &["https://app.example.*/cb"]
        ));
        assert!(!m("https://a.b.com/cb", &["https://a.*.com/cb"]));
        assert!(!m("https://x.com/cb", &["https://*/cb"]));
        assert!(!m("https://x.com/cb", &["https://*.com/cb"]));
        assert!(!m("http://a.example.com/cb", &["https://*.example.com/cb"]));
        assert!(!m(
            "https://a.example.com:8443/cb",
            &["https://*.example.com/cb"]
        ));
    }

    #[test]
    fn exact_registration_is_unchanged() {
        assert!(m(
            "https://app.example.com/cb",
            &["https://app.example.com/cb"]
        ));
        assert!(!m(
            "https://app.example.com/cb2",
            &["https://app.example.com/cb"]
        ));
        assert!(!m("https://app.example.com/cb", &[]));
        // Exact still compares as a string (no normalisation without a wildcard).
        assert!(!m(
            "https://APP.example.com/cb",
            &["https://app.example.com/cb"]
        ));
    }
}

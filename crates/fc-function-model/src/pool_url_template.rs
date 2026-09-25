//! Java `function/PoolUrlTemplate.java`: the `FC_FN_POOL_URL` convention.

use std::fmt;

use crate::dns_label::DnsLabel;

/// One URL template with an optional `{pool}` placeholder, resolved per
/// manifest pool at promote. A single-pool environment (or dev) names the
/// host directly and never mentions `{pool}`.
///
/// The whole shape is checked up front: at most one `{pool}`; an absolute
/// `http`/`https` URL; no userinfo; no path beyond an optional trailing
/// slash (stripped by [`PoolUrlTemplate::resolve`], because the caller
/// appends `/functions/...`); no query; no fragment. The URL grammar is
/// `java.net.URI`'s (RFC 2396), reproduced by the parser at the bottom of
/// this file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolUrlTemplate(String);

/// A template that breaks one of the rules. The message always names
/// `FC_FN_POOL_URL`, the rule and the template, as Java's
/// `IllegalStateException` does.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("FC_FN_POOL_URL {rule}: '{template}'")]
pub struct InvalidPoolUrl {
    pub rule: &'static str,
    pub template: String,
}

const PLACEHOLDER: &str = "{pool}";

impl PoolUrlTemplate {
    /// The template when `FC_FN_POOL_URL` is unset.
    pub const DEFAULT: &'static str = "http://fn-{pool}:8080";

    pub fn parse(raw: &str) -> Result<PoolUrlTemplate, InvalidPoolUrl> {
        let fail = |rule| {
            Err(InvalidPoolUrl {
                rule,
                template: raw.to_string(),
            })
        };
        if raw.matches(PLACEHOLDER).count() > 1 {
            return fail("must contain at most one '{pool}' placeholder");
        }
        // '{' and '}' are not URI characters: check the shape with a legal
        // stand-in; the template itself is what is kept.
        let for_parsing = raw.replace(PLACEHOLDER, "fc-pool-placeholder");
        let Some(uri) = JavaUri::parse(&for_parsing) else {
            return fail("must be a valid URL");
        };
        let Some(scheme) = uri.scheme else {
            return fail("must be an absolute http/https URL");
        };
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return fail("must use the http or https scheme");
        }
        if uri.user_info {
            return fail("must not contain userinfo");
        }
        if !uri.host {
            return fail("must name a host");
        }
        if uri.query {
            return fail("must not contain a query");
        }
        if uri.fragment {
            return fail("must not contain a fragment");
        }
        if !uri.path.is_empty() && uri.path != "/" {
            return fail("must not contain a path beyond an optional trailing slash");
        }
        Ok(PoolUrlTemplate(raw.to_string()))
    }

    /// From `FC_FN_POOL_URL`, or [`PoolUrlTemplate::DEFAULT`] when unset.
    pub fn from_env() -> Result<PoolUrlTemplate, InvalidPoolUrl> {
        Self::parse(&std::env::var("FC_FN_POOL_URL").unwrap_or_else(|_| Self::DEFAULT.to_string()))
    }

    /// The template exactly as configured.
    pub fn template(&self) -> &str {
        &self.0
    }

    /// `{pool}` replaced by `pool` (a DNS label is already a legal host
    /// segment, so nothing is encoded), any trailing slash removed. A
    /// template without the placeholder resolves to itself for every pool.
    pub fn resolve(&self, pool: &DnsLabel) -> String {
        let resolved = self.0.replace(PLACEHOLDER, pool.value());
        match resolved.strip_suffix('/') {
            Some(stripped) => stripped.to_string(),
            None => resolved,
        }
    }
}

impl fmt::Display for PoolUrlTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The parts of a `java.net.URI` the template rules look at. `None` from
/// [`JavaUri::parse`] is Java's `URISyntaxException`.
#[derive(Debug, Default)]
struct JavaUri {
    scheme: Option<String>,
    /// A server-based authority with user info.
    user_info: bool,
    /// A server-based authority with a host (Java's `getHost() != null`).
    host: bool,
    path: String,
    query: bool,
    fragment: bool,
}

// RFC 2396 character classes, as `java.net.URI` defines them.
const MARK: &str = "-_.!~*'()";
const RESERVED: &str = ";/?:@&=+$,[]";

fn alphanum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn unreserved(c: char) -> bool {
    alphanum(c) || MARK.contains(c)
}

fn uric(c: char) -> bool {
    unreserved(c) || RESERVED.contains(c)
}

fn path_char(c: char) -> bool {
    unreserved(c) || ":@&=+$,;/".contains(c)
}

fn userinfo_char(c: char) -> bool {
    unreserved(c) || ";:&=+$,".contains(c)
}

fn reg_name_char(c: char) -> bool {
    unreserved(c) || "$,;:@&=+".contains(c)
}

fn server_char(c: char) -> bool {
    userinfo_char(c) || ".:@[]".contains(c)
}

fn scheme_char(c: char) -> bool {
    alphanum(c) || "+-.".contains(c)
}

/// Java's `Character.isSpaceChar` (Unicode Zs, Zl, Zp).
fn is_space_char(c: char) -> bool {
    matches!(
        c,
        '\u{20}' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// Java's `URI.scan` over a class that admits escapes: characters in the
/// class, `%XX` escapes, and visible non-ASCII characters. Returns the end
/// of the run, or `None` for a malformed escape (an exception in Java).
fn scan_escaped(
    chars: &[char],
    start: usize,
    end: usize,
    class: fn(char) -> bool,
) -> Option<usize> {
    let mut p = start;
    while p < end {
        let c = chars[p];
        if (c as u32) < 128 && class(c) {
            p += 1;
        } else if c == '%' {
            let hex = |i: usize| i < end && chars[i].is_ascii_hexdigit();
            if !(hex(p + 1) && hex(p + 2)) {
                return None;
            }
            p += 3;
        } else if (c as u32) > 128 && !is_space_char(c) && !c.is_control() {
            p += 1;
        } else {
            break;
        }
    }
    Some(p)
}

/// `checkChars`: the whole of `start..end` must be in the class.
fn check_escaped(chars: &[char], start: usize, end: usize, class: fn(char) -> bool) -> Option<()> {
    (scan_escaped(chars, start, end, class)? == end).then_some(())
}

fn find(chars: &[char], start: usize, end: usize, stops: &str) -> usize {
    (start..end)
        .find(|&i| stops.contains(chars[i]))
        .unwrap_or(end)
}

impl JavaUri {
    fn parse(input: &str) -> Option<JavaUri> {
        let chars: Vec<char> = input.chars().collect();
        let n = chars.len();
        let mut uri = JavaUri::default();
        let mut p;
        // A scheme is a ':' before any of "/?#".
        let colon = (0..n)
            .take_while(|&i| !"/?#".contains(chars[i]))
            .find(|&i| chars[i] == ':');
        if let Some(colon) = colon {
            if colon == 0
                || !chars[0].is_ascii_alphabetic()
                || !chars[1..colon].iter().all(|&c| scheme_char(c))
            {
                return None;
            }
            uri.scheme = Some(chars[..colon].iter().collect());
            p = colon + 1;
            if chars.get(p) == Some(&'/') {
                p = uri.hierarchical(&chars, p)?;
            } else {
                // Opaque: the scheme-specific part runs to '#'.
                let q = find(&chars, p, n, "#");
                if q <= p {
                    return None;
                }
                check_escaped(&chars, p, q, uric)?;
                p = q;
            }
        } else {
            p = uri.hierarchical(&chars, 0)?;
        }
        if chars.get(p) == Some(&'#') {
            check_escaped(&chars, p + 1, n, uric)?;
            uri.fragment = true;
            p = n;
        }
        (p == n).then_some(uri)
    }

    fn hierarchical(&mut self, chars: &[char], start: usize) -> Option<usize> {
        let n = chars.len();
        let mut p = start;
        if chars.get(p) == Some(&'/') && chars.get(p + 1) == Some(&'/') {
            p += 2;
            let q = find(chars, p, n, "/?#");
            if q > p {
                p = self.authority(chars, p, q)?;
            } else if q >= n {
                return None; // expected authority
            }
        }
        let q = find(chars, p, n, "?#");
        check_escaped(chars, p, q, path_char)?;
        self.path = chars[p..q].iter().collect();
        p = q;
        if chars.get(p) == Some(&'?') {
            p += 1;
            let q = find(chars, p, n, "#");
            check_escaped(chars, p, q, uric)?;
            self.query = true;
            p = q;
        }
        Some(p)
    }

    /// A server-based authority when it parses as one, otherwise a
    /// registry-based one (no host), otherwise an error.
    fn authority(&mut self, chars: &[char], start: usize, end: usize) -> Option<usize> {
        let server_chars = scan_escaped(chars, start, end, server_char)? == end;
        let reg_chars = scan_escaped(chars, start, end, reg_name_char)? == end;
        if reg_chars && !server_chars {
            return Some(end);
        }
        if server_chars {
            if let Some((user_info, q)) = server(chars, start, end) {
                if q == end {
                    self.user_info = user_info;
                    self.host = true;
                    return Some(end);
                }
            }
        }
        reg_chars.then_some(end)
    }
}

/// `[userinfo@]host[:port]`; returns whether there was user info and where
/// the server ended.
fn server(chars: &[char], start: usize, end: usize) -> Option<(bool, usize)> {
    let mut p = start;
    let mut user_info = false;
    let at = find(chars, p, end, "/?#@");
    if at < end && chars[at] == '@' {
        check_escaped(chars, p, at, userinfo_char)?;
        user_info = true;
        p = at + 1;
    }
    if chars.get(p) == Some(&'[') {
        let close = find(chars, p + 1, end, "/?#]");
        if close >= end || chars[close] != ']' {
            return None;
        }
        let literal: String = chars[p + 1..close].iter().collect();
        literal.parse::<std::net::Ipv6Addr>().ok()?;
        p = close + 1;
    } else {
        p = ipv4(chars, p, end).or_else(|| hostname(chars, p, end))?;
    }
    if chars.get(p) == Some(&':') && p < end {
        p += 1;
        let digits = p;
        while p < end && chars[p].is_ascii_digit() {
            p += 1;
        }
        // Java parses the port as an int; one that overflows is malformed.
        if p > digits {
            chars[digits..p]
                .iter()
                .collect::<String>()
                .parse::<i32>()
                .ok()?;
        }
    }
    (p == end).then_some((user_info, p))
}

/// A dotted-quad IPv4 address followed by the end or ':'.
fn ipv4(chars: &[char], start: usize, end: usize) -> Option<usize> {
    let stop = find(chars, start, end, ":");
    let text: String = chars[start..stop].iter().collect();
    let parts: Vec<&str> = text.split('.').collect();
    let valid = parts.len() == 4
        && parts.iter().all(|part| {
            // Any number of digits, as long as the value (an int) is a byte.
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<i32>().is_ok_and(|v| v <= 255)
        });
    valid.then_some(stop)
}

/// RFC 2396 hostname: dot-separated labels of alphanumerics and '-', not
/// ending in '-'; the last label of a multi-label name starts with a
/// letter. Must be followed by the end or ':'.
fn hostname(chars: &[char], start: usize, end: usize) -> Option<usize> {
    let mut p = start;
    let mut last_label = None;
    loop {
        let q = (p..end).find(|&i| !alphanum(chars[i])).unwrap_or(end);
        if q <= p {
            break;
        }
        last_label = Some(p);
        p = q;
        let q = (p..end)
            .find(|&i| !(alphanum(chars[i]) || chars[i] == '-'))
            .unwrap_or(end);
        if q > p {
            if chars[q - 1] == '-' {
                return None;
            }
            p = q;
        }
        if p < end && chars[p] == '.' {
            p += 1;
        } else {
            break;
        }
        if p >= end {
            break;
        }
    }
    if p < end && chars[p] != ':' {
        return None;
    }
    let last = last_label?;
    if last > start && !chars[last].is_ascii_alphabetic() {
        return None;
    }
    Some(p)
}

/// Java `PoolUrlTemplateTest`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts() {
        for raw in [
            "http://fn-{pool}:8080",
            "https://fn-{pool}:8443",
            "http://127.0.0.1:8080",
            "https://fn-pool.internal:8080",
            "http://fn-{pool}:8080/",
            "http://fn-{pool}.internal",
            "http://{pool}:8080",
        ] {
            assert_eq!(PoolUrlTemplate::parse(raw).unwrap().template(), raw);
        }
    }

    #[test]
    fn rejects() {
        for (rule, raw) in [
            (
                "userinfo, the old {pool}-as-userinfo hack",
                "http://{pool}@127.0.0.1:8080",
            ),
            ("userinfo with no placeholder", "http://admin@fn-host:8080"),
            ("two placeholders", "http://{pool}-{pool}:8080"),
            (
                "two placeholders in different segments",
                "http://fn-{pool}:8080/{pool}",
            ),
            ("a path beyond root", "http://fn-{pool}:8080/base"),
            ("a path with no placeholder", "http://fn-host:8080/base"),
            ("a query string", "http://fn-{pool}:8080?x=1"),
            ("a fragment", "http://fn-{pool}:8080#frag"),
            ("not absolute", "//fn-{pool}:8080"),
            ("not http/https", "ftp://fn-{pool}:8080"),
            ("not a URL at all", "not a url"),
            ("blank", ""),
        ] {
            let err = PoolUrlTemplate::parse(raw).unwrap_err();
            assert!(err.to_string().contains("FC_FN_POOL_URL"), "{rule}");
        }
    }

    #[test]
    fn resolve_substitutes_and_strips_a_trailing_slash() {
        let orders = DnsLabel::new_unchecked("orders");
        let t = PoolUrlTemplate::parse("http://fn-{pool}:8080/").unwrap();
        assert_eq!(t.resolve(&orders), "http://fn-orders:8080");
        let t = PoolUrlTemplate::parse("http://127.0.0.1:9090").unwrap();
        assert_eq!(t.resolve(&orders), "http://127.0.0.1:9090");
        assert_eq!(
            t.resolve(&DnsLabel::new_unchecked("billing")),
            "http://127.0.0.1:9090"
        );
        let t = PoolUrlTemplate::parse("http://fn-{pool}:8080").unwrap();
        assert_eq!(t.resolve(&orders), "http://fn-orders:8080");
    }

    #[test]
    fn default_parses() {
        assert!(PoolUrlTemplate::parse(PoolUrlTemplate::DEFAULT).is_ok());
    }
}

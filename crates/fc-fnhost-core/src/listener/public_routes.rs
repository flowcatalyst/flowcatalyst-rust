//! The public listener's routing (Java `fnhost/route/PublicRouteTable.java`
//! and `FnHttpServer.publicHostname` / `publicRemoteAddress`; spec
//! `function-public-routes.md` §3, `function-zones-and-aliases.md` §4).
//!
//! A table is an immutable snapshot of one document's `publicRoutes`. The
//! hostname is looked up exactly; among its routes the longest
//! whole-segment prefix of the raw path wins (`/billing` matches
//! `/billing/x`, never `/billingx`), alias `live`. Failing that, when the
//! hostname's first label contains a `-`, it is split at the first `-` into
//! an alias prefix and a base hostname, looked up the same way, and the
//! winning route must have opted that prefix in. One level only.

use std::collections::HashMap;
use std::net::IpAddr;

use fc_function_abi::FunctionAddress;
use fc_function_model::{Hostname, LIVE_ALIAS};
use http::HeaderMap;

use crate::desired::PublicRouteRef;
use crate::env::TrustedProxies;

/// The alias an exact hostname match resolves to.
pub const LIVE: &str = LIVE_ALIAS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub address: FunctionAddress,
    pub function_path: String,
    /// `live`, or the opted-in alias prefix.
    pub alias: String,
}

#[derive(Debug)]
struct Route {
    segments: Vec<String>,
    address: FunctionAddress,
    alias_prefixes: Vec<String>,
}

#[derive(Debug, Default)]
pub struct PublicRouteTable {
    /// Hostname → its routes, most segments first.
    by_host: HashMap<String, Vec<Route>>,
}

impl PublicRouteTable {
    pub fn of(refs: &[PublicRouteRef]) -> Self {
        let mut by_host: HashMap<String, Vec<Route>> = HashMap::new();
        for r in refs {
            by_host
                .entry(r.hostname.to_lowercase())
                .or_default()
                .push(Route {
                    segments: split_segments(&r.path_prefix),
                    address: r.address.clone(),
                    alias_prefixes: r.alias_prefixes.clone(),
                });
        }
        for routes in by_host.values_mut() {
            routes.sort_by_key(|r| std::cmp::Reverse(r.segments.len())); // stable
        }
        Self { by_host }
    }

    /// `hostname` already lower-cased, port stripped; `path` raw.
    pub fn resolve(&self, hostname: &str, path: &str) -> Option<Match> {
        if let Some((route, function_path)) = self.match_at(hostname, path) {
            return Some(Match {
                address: route.address.clone(),
                function_path,
                alias: LIVE.to_owned(),
            });
        }
        let label_end = hostname.find('.').unwrap_or(hostname.len());
        let dash = hostname.find('-').filter(|&d| d < label_end)?;
        let (prefix, base) = (&hostname[..dash], &hostname[dash + 1..]);
        let (route, function_path) = self.match_at(base, path)?;
        route
            .alias_prefixes
            .iter()
            .any(|p| p == prefix)
            .then(|| Match {
                address: route.address.clone(),
                function_path,
                alias: prefix.to_owned(),
            })
    }

    fn match_at(&self, hostname: &str, path: &str) -> Option<(&Route, String)> {
        let path_segments = split_segments(path);
        self.by_host.get(hostname)?.iter().find_map(|route| {
            let n = route.segments.len();
            (n <= path_segments.len() && route.segments[..] == path_segments[..n]).then(|| {
                let rest = &path_segments[n..];
                let function_path = if rest.is_empty() {
                    "/".to_owned()
                } else {
                    rest.iter().map(|s| format!("/{s}")).collect()
                };
                (route, function_path)
            })
        })
    }
}

/// `/` is no segments; otherwise the raw segments after the leading slash.
fn split_segments(path: &str) -> Vec<String> {
    if path.is_empty() || path == "/" {
        return Vec::new();
    }
    path.strip_prefix('/')
        .unwrap_or(path)
        .split('/')
        .map(str::to_owned)
        .collect()
}

/// `Host` (HTTP/1.1) or `:authority` (HTTP/2): port stripped, then a valid
/// [`Hostname`] (lower-cased; at most 253 characters, at least two DNS
/// labels, not an IP literal), else `None` (404). `X-Forwarded-Host` is
/// never consulted.
pub(crate) fn public_hostname(authority: Option<&str>) -> Option<String> {
    let authority = authority?;
    if authority.starts_with('[') {
        return None; // an IP literal is never a route hostname
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => authority,
    };
    Hostname::try_parse(host).map(|h| h.value().to_owned())
}

/// The right-most `X-Forwarded-For` entry, only when the TCP peer is a
/// trusted proxy and the entry is an IP literal; the peer otherwise.
pub(crate) fn remote_address(
    peer: IpAddr,
    headers: &HeaderMap,
    trusted: &TrustedProxies,
) -> String {
    let peer_text = peer.to_string();
    if !trusted.is_trusted(peer) {
        return peer_text;
    }
    let Some(xff) = headers
        .get("x-forwarded-for")
        .map(|v| super::latin1(v.as_bytes()))
        .filter(|v| !crate::java::is_blank(v))
    else {
        return peer_text;
    };
    let rightmost = xff.rsplit(',').next().unwrap_or("").trim();
    if !rightmost.is_empty() && is_ip_literal(rightmost) {
        rightmost.to_owned()
    } else {
        peer_text
    }
}

/// Java `TrustedProxies.isIpLiteral`: syntactic only, never DNS.
pub(crate) fn is_ip_literal(s: &str) -> bool {
    let octets: Vec<&str> = s.split('.').collect();
    if octets.len() == 4
        && octets
            .iter()
            .all(|o| (1..=3).contains(&o.len()) && o.bytes().all(|b| b.is_ascii_digit()))
    {
        return octets
            .iter()
            .all(|o| o.parse::<u32>().is_ok_and(|v| v <= 255));
    }
    s.contains(':')
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(raw: &str) -> FunctionAddress {
        FunctionAddress::parse(raw).unwrap()
    }

    fn route(host: &str, prefix: &str, address: &str, aliases: &[&str]) -> PublicRouteRef {
        PublicRouteRef {
            hostname: host.into(),
            path_prefix: prefix.into(),
            address: a(address),
            alias_prefixes: aliases.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn resolved(
        table: &PublicRouteTable,
        host: &str,
        path: &str,
    ) -> Option<(String, String, String)> {
        table
            .resolve(host, path)
            .map(|m| (m.address.render(), m.function_path, m.alias))
    }

    // Java PublicRouteTableTest
    #[test]
    fn prefix_matching() {
        let table = PublicRouteTable::of(&[
            route("api.acme.com", "/", "a.b.root", &[]),
            route("api.acme.com", "/billing", "a.b.billing", &[]),
            route("api.acme.com", "/billing/v2", "a.b.v2", &[]),
            route("Other.acme.com", "/x", "a.b.other", &[]),
        ]);
        let r = |host: &str, path: &str| resolved(&table, host, path);
        assert_eq!(
            r("api.acme.com", "/billing"),
            Some(("a.b.billing".into(), "/".into(), "live".into()))
        );
        assert_eq!(r("api.acme.com", "/billing/x/y").unwrap().1, "/x/y");
        assert_eq!(r("api.acme.com", "/billingx").unwrap().0, "a.b.root");
        assert_eq!(r("api.acme.com", "/billingx").unwrap().1, "/billingx");
        assert_eq!(r("api.acme.com", "/billing/v2/z").unwrap().0, "a.b.v2");
        assert_eq!(
            r("api.acme.com", "/").unwrap(),
            ("a.b.root".into(), "/".into(), "live".into())
        );
        assert_eq!(r("other.acme.com", "/x").unwrap().0, "a.b.other");
        assert_eq!(r("nobody.acme.com", "/"), None);
        assert!(resolved(&PublicRouteTable::default(), "api.acme.com", "/").is_none());
    }

    #[test]
    fn alias_prefixes() {
        let table = PublicRouteTable::of(&[
            route("my-app.acme.com", "/", "a.b.base", &["qa"]),
            route("qa-my-app.acme.com", "/exact", "a.b.exact", &[]),
            route("app.acme.com", "/", "a.b.app", &["qa", "staging"]),
        ]);
        let r = |host: &str, path: &str| resolved(&table, host, path);
        assert_eq!(
            r("qa-my-app.acme.com", "/x"),
            Some(("a.b.base".into(), "/x".into(), "qa".into()))
        );
        assert_eq!(r("qa-my-app.acme.com", "/exact").unwrap().0, "a.b.exact");
        assert_eq!(r("dev-my-app.acme.com", "/x"), None, "not opted in");
        assert_eq!(r("staging-app.acme.com", "/").unwrap().2, "staging");
        assert_eq!(r("qa-staging-app.acme.com", "/"), None, "one level only");
        assert_eq!(r("myapp.acme.com", "/"), None);
        assert_eq!(
            r("qa.my-app.acme.com", "/"),
            None,
            "the dash must be in the first label"
        );
    }

    #[test]
    fn hostnames() {
        assert_eq!(
            public_hostname(Some("API.Acme.com:8081")).as_deref(),
            Some("api.acme.com")
        );
        assert_eq!(
            public_hostname(Some("hello.localhost")).as_deref(),
            Some("hello.localhost")
        );
        for bad in [
            "localhost",
            "127.0.0.1",
            "[::1]:80",
            "a_b.com",
            "-a.com",
            "",
        ] {
            assert_eq!(public_hostname(Some(bad)), None, "{bad}");
        }
        assert_eq!(public_hostname(None), None);
    }

    #[test]
    fn forwarded_for_is_trusted_only_from_a_trusted_peer() {
        let trusted = TrustedProxies::default_list();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "203.0.113.9, 198.51.100.7".parse().unwrap(),
        );
        assert_eq!(
            remote_address("10.0.0.5".parse().unwrap(), &headers, &trusted),
            "198.51.100.7"
        );
        assert_eq!(
            remote_address("8.8.8.8".parse().unwrap(), &headers, &trusted),
            "8.8.8.8"
        );
        headers.insert(
            "x-forwarded-for",
            "198.51.100.7, not-an-ip".parse().unwrap(),
        );
        assert_eq!(
            remote_address("10.0.0.5".parse().unwrap(), &headers, &trusted),
            "10.0.0.5"
        );
        assert_eq!(
            remote_address("10.0.0.5".parse().unwrap(), &HeaderMap::new(), &trusted),
            "10.0.0.5"
        );
        assert!(is_ip_literal("2001:db8::1") && is_ip_literal("1.2.3.4"));
        assert!(!is_ip_literal("256.1.1.1") && !is_ip_literal("example.com"));
    }
}

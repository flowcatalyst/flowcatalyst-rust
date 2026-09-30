//! Keeps outbound deliveries away from addresses that should never be a
//! customer webhook (Go: `internal/netguard`): the machine's own loopback,
//! cloud metadata (169.254.169.254 and its IPv6 sibling), link-local ranges,
//! and, unless allowed, private networks such as the cluster's own pod and
//! service addresses.
//!
//! A subscription endpoint or a dispatch job's target is attacker-influenced
//! input: whoever can create one chooses a URL the platform will POST to from
//! inside the cluster, carrying the platform's credentials. Without a guard
//! that is a server-side request forgery primitive against whatever the
//! cluster can reach.
//!
//! This module is the **write-time** check, [`Policy::validate_url`]: it
//! rejects a bad URL when it is written, so the operator gets an immediate,
//! readable error instead of a delivery that fails for ever. It looks at the
//! URL's literal host only: a host *name* that resolves to a private address
//! passes it, and so does a row written before the check existed.
//!
//! The check on the address actually dialled is the authoritative one, and
//! lives in [`http`] (feature `guarded-http`): a resolver that drops every
//! address the policy forbids, and a redirect policy that checks each hop. A
//! client that delivers to customer URLs uses both, and also calls
//! [`Policy::validate_url`] on the target before it sends, because a URL that
//! names an IP address never reaches the resolver.
//!
//! The policy comes from the environment, so every binary is strict unless
//! told otherwise:
//!
//! - `FC_DELIVERY_ALLOW_LOOPBACK`: permit loopback destinations (dev only)
//! - `FC_DELIVERY_ALLOW_PRIVATE`: permit private-network destinations
//! - `FC_DELIVERY_ALLOW_HOSTS`: comma-separated `host:port` patterns exempt
//!   from the checks, e.g. `runner.internal:8095,*.fn.svc:8095`
//!
//! Cloud metadata and link-local addresses stay blocked whatever is allowed.

use std::env;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use thiserror::Error;
use url::{Host, ParseError, Url};

/// Why a URL or address was refused. The `Display` text is what the API
/// returns after `"endpoint "` or `"targetUrl "`, worded as Go words it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Rejected {
    /// The policy said no (Go: wraps `ErrBlocked`).
    #[error("destination not allowed: {0}")]
    Blocked(String),
    /// The text is not a usable delivery URL.
    #[error("{0}")]
    Invalid(String),
}

/// The AWS IPv6 instance-metadata address. It sits inside `fc00::/7`, so
/// allowing private ranges must not let it through.
const METADATA_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254);

/// Decides which destinations outbound deliveries may reach. The default is
/// the strict policy: public addresses only.
#[derive(Debug, Default)]
pub struct Policy {
    allow_loopback: AtomicBool,
    allow_private: AtomicBool,
    /// `host:port` patterns, lower-cased; `*` matches any run of characters.
    allow_hosts: RwLock<Vec<String>>,
}

impl Policy {
    /// The strict policy.
    pub fn strict() -> Self {
        Self::default()
    }

    /// The policy named by the environment (see the module docs). Unset or
    /// unparseable values leave the strict default.
    pub fn from_env() -> Self {
        let policy = Self::strict();
        policy.set_allow_loopback(env_bool("FC_DELIVERY_ALLOW_LOOPBACK"));
        policy.set_allow_private(env_bool("FC_DELIVERY_ALLOW_PRIVATE"));
        if let Ok(hosts) = env::var("FC_DELIVERY_ALLOW_HOSTS") {
            for host in hosts.split(',').map(str::trim).filter(|h| !h.is_empty()) {
                policy.allow_host(host);
            }
        }
        policy
    }

    /// Permit 127.0.0.0/8, `::1` and the name `localhost` (development,
    /// where webhook receivers run on the same machine).
    pub fn set_allow_loopback(&self, allow: bool) {
        self.allow_loopback.store(allow, Ordering::Relaxed);
    }

    /// Permit RFC 1918, unique-local (`fc00::/7`) and other non-public
    /// unicast ranges. Off by default: in a cluster these are the pods,
    /// services and internal APIs a forged request could reach.
    pub fn set_allow_private(&self, allow: bool) {
        self.allow_private.store(allow, Ordering::Relaxed);
    }

    /// Exempt a `host:port` pattern (`*` is a wildcard) from the address
    /// checks. For the platform's own internal endpoints, which legitimately
    /// live on a loopback or private address. Not for customer input.
    pub fn allow_host(&self, host_port: &str) {
        self.allow_hosts
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(host_port.to_lowercase());
    }

    /// [`allow_host`](Self::allow_host) for a URL's authority, filling in the
    /// scheme's default port. A `{pool}` placeholder (the function runner's
    /// URL template) becomes a wildcard. Unparseable input is ignored.
    pub fn allow_url(&self, raw: &str) {
        let wildcard = raw.contains("{pool}");
        let Ok(url) = Url::parse(&raw.replace("{pool}", "wildcard-pool")) else {
            return;
        };
        let Some(mut host_port) = host_port_of(&url) else {
            return;
        };
        if wildcard {
            host_port = host_port.replace("wildcard-pool", "*");
        }
        self.allow_host(&host_port);
    }

    /// Why `ip` may not be dialled, or `Ok`.
    pub fn check_ip(&self, ip: IpAddr) -> Result<(), Rejected> {
        let ip = unmap(ip);
        let blocked = |what: &str| Err(Rejected::Blocked(format!("{ip} is {what}")));
        if ip.is_unspecified() {
            return blocked("the unspecified address");
        }
        if ip.is_multicast() {
            return blocked("a multicast address");
        }
        // 169.254.0.0/16, which includes the cloud metadata service, and
        // fe80::/10.
        if is_link_local(ip) {
            return blocked("a link-local address");
        }
        if ip == IpAddr::V6(METADATA_V6) {
            return blocked("the cloud metadata address");
        }
        if ip.is_loopback() {
            return if self.allow_loopback.load(Ordering::Relaxed) {
                Ok(())
            } else {
                blocked("a loopback address")
            };
        }
        if (is_private(ip) || is_cgnat(ip) || !is_global_unicast(ip))
            && !self.allow_private.load(Ordering::Relaxed)
        {
            return blocked("a private or reserved address");
        }
        Ok(())
    }

    /// The write-time check for a delivery URL: an absolute http or https
    /// URL, without embedded credentials, whose host is not an address (or
    /// the name `localhost`) the policy forbids. Host names are not resolved
    /// here.
    pub fn validate_url(&self, raw: &str) -> Result<(), Rejected> {
        let invalid = |what: &str| Err(Rejected::Invalid(what.to_string()));
        let url = match Url::parse(raw.trim()) {
            Ok(url) => url,
            Err(ParseError::RelativeUrlWithoutBase) => {
                return invalid("must be an http or https URL")
            }
            Err(ParseError::EmptyHost) => return invalid("must include a host"),
            Err(e) => return Err(Rejected::Invalid(format!("not a valid URL: {e}"))),
        };
        if url.scheme() != "http" && url.scheme() != "https" {
            return invalid("must be an http or https URL");
        }
        let Some(host) = url.host() else {
            return invalid("must include a host");
        };
        if !url.username().is_empty() || url.password().is_some() {
            return invalid("must not embed credentials");
        }
        if host_port_of(&url).is_some_and(|hp| self.host_allowed(&hp)) {
            return Ok(());
        }
        match host {
            Host::Domain(name) => {
                let name = name.to_lowercase();
                if name == "localhost" || name.ends_with(".localhost") {
                    return if self.allow_loopback.load(Ordering::Relaxed) {
                        Ok(())
                    } else {
                        Err(Rejected::Blocked(format!("{name} is a loopback name")))
                    };
                }
                Ok(())
            }
            Host::Ipv4(ip) => self.check_ip(IpAddr::V4(ip)),
            Host::Ipv6(ip) => self.check_ip(IpAddr::V6(ip)),
        }
    }

    /// Whether `host` alone is exempt: some allowed pattern's host part
    /// matches it. Name resolution knows the name but not the port, so the
    /// port cannot be compared there (the URL check, which knows it, does).
    pub fn host_name_allowed(&self, host: &str) -> bool {
        let host = host.to_lowercase();
        self.allow_hosts
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|pattern| {
                let pattern_host = pattern
                    .rsplit_once(':')
                    .map_or(pattern.as_str(), |(h, _)| h);
                glob_match(pattern_host, &host)
            })
    }

    fn host_allowed(&self, host_port: &str) -> bool {
        let host_port = host_port.to_lowercase();
        self.allow_hosts
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|pattern| glob_match(pattern, &host_port))
    }
}

/// The policy the platform's delivery paths consult. It starts from the
/// environment, so every binary is strict unless told otherwise; startup
/// code adds the platform's own internal endpoints with
/// [`Policy::allow_url`], and the dev binary switches loopback and private
/// targets on.
pub fn default_policy() -> &'static Policy {
    static DEFAULT: OnceLock<Policy> = OnceLock::new();
    DEFAULT.get_or_init(Policy::from_env)
}

/// A policy for development and tests: loopback and private targets allowed,
/// so webhook receivers on the developer's own machine work. Cloud metadata
/// and link-local addresses stay blocked. Independent of the environment.
pub fn dev_policy() -> &'static Policy {
    static DEV: OnceLock<Policy> = OnceLock::new();
    DEV.get_or_init(|| {
        let policy = Policy::strict();
        policy.set_allow_loopback(true);
        policy.set_allow_private(true);
        policy
    })
}

/// Go's `strconv.ParseBool` for an env var: true for `1`, `t`, `T`, `TRUE`,
/// `true`, `True`; anything else, or unset, is false.
fn env_bool(name: &str) -> bool {
    matches!(
        env::var(name).as_deref(),
        Ok("1" | "t" | "T" | "TRUE" | "true" | "True")
    )
}

/// `host:port` for a URL, with the scheme's default port and IPv6 hosts in
/// brackets (Go: `hostPortOf`).
fn host_port_of(url: &Url) -> Option<String> {
    let host = match url.host()? {
        Host::Domain(name) => name.to_lowercase(),
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => format!("[{ip}]"),
    };
    let port = url.port_or_known_default()?;
    Some(format!("{host}:{port}"))
}

fn unmap(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unicast_link_local(),
    }
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => v6.is_unique_local(),
    }
}

/// 100.64.0.0/10, carrier-grade NAT.
fn is_cgnat(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(v4) if v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]))
}

/// Not unspecified, loopback, multicast, link-local or the IPv4 broadcast.
fn is_global_unicast(ip: IpAddr) -> bool {
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || is_link_local(ip)
        || matches!(ip, IpAddr::V4(v4) if v4 == Ipv4Addr::BROADCAST))
}

/// Match `text` against `pattern`, where `*` matches any run of characters
/// (Go's `path.Match` on a `host:port`, which contains no `/`).
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || !text[first.len()..].ends_with(last) {
        return false;
    }
    if text.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for middle in &parts[1..parts.len() - 1] {
        match rest.find(middle) {
            Some(i) => rest = &rest[i + middle.len()..],
            None => return false,
        }
    }
    true
}

/// The dial-time half of the policy for `reqwest` clients.
#[cfg(feature = "guarded-http")]
pub mod http {
    use std::io;
    use std::net::SocketAddr;
    use std::sync::Arc;

    use reqwest::dns::{Addrs, Name, Resolve, Resolving};
    use reqwest::redirect;
    use reqwest::ClientBuilder;
    use tokio::net::lookup_host;

    use super::Policy;

    /// Resolves names through the system resolver, then drops every address
    /// the policy forbids: a name that resolves to a private address, or is
    /// changed to one after the URL was validated, never gets a connection.
    /// Only the address actually connected to is seen here, so this is the
    /// authoritative check for names. If nothing is left, the lookup fails
    /// with the policy's reason.
    pub struct GuardedResolver {
        policy: &'static Policy,
    }

    impl GuardedResolver {
        pub fn new(policy: &'static Policy) -> Self {
            Self { policy }
        }
    }

    impl Resolve for GuardedResolver {
        fn resolve(&self, name: Name) -> Resolving {
            let policy = self.policy;
            Box::pin(async move {
                let host = name.as_str().to_string();
                let resolved: Vec<SocketAddr> = lookup_host((host.as_str(), 0)).await?.collect();
                if policy.host_name_allowed(&host) {
                    return Ok(Box::new(resolved.into_iter()) as Addrs);
                }
                let mut refusal = None;
                let allowed: Vec<SocketAddr> = resolved
                    .into_iter()
                    .filter(|addr| match policy.check_ip(addr.ip()) {
                        Ok(()) => true,
                        Err(e) => {
                            refusal.get_or_insert(e);
                            false
                        }
                    })
                    .collect();
                if allowed.is_empty() {
                    return Err(match refusal {
                        Some(e) => e.into(),
                        None => io::Error::new(io::ErrorKind::NotFound, "no addresses").into(),
                    });
                }
                Ok(Box::new(allowed.into_iter()) as Addrs)
            })
        }
    }

    /// Send `builder`'s name lookups through `policy`.
    pub fn guard(builder: ClientBuilder, policy: &'static Policy) -> ClientBuilder {
        builder.dns_resolver(Arc::new(GuardedResolver::new(policy)))
    }

    /// Follow up to `max` redirects, refusing any hop `policy` forbids. A
    /// redirect to an IP address skips the resolver, so each hop's URL is
    /// checked as a written one is.
    pub fn guarded_redirects(policy: &'static Policy, max: usize) -> redirect::Policy {
        redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= max {
                return attempt.error("too many redirects");
            }
            match policy.validate_url(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(e),
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        fn policy(loopback: bool) -> &'static Policy {
            let policy = Box::leak(Box::new(Policy::strict()));
            policy.set_allow_loopback(loopback);
            policy
        }

        /// A listener on 127.0.0.1 that answers one request with 204.
        fn serve_once() -> u16 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);
                    let _ =
                        stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                }
            });
            port
        }

        fn client(policy: &'static Policy) -> reqwest::Client {
            reqwest::Client::builder()
                .dns_resolver(Arc::new(GuardedResolver::new(policy)))
                .build()
                .unwrap()
        }

        #[tokio::test]
        async fn a_name_that_resolves_to_loopback_is_refused_by_a_strict_policy() {
            let port = serve_once();
            let err = client(policy(false))
                .get(format!("http://localhost:{port}/"))
                .send()
                .await
                .unwrap_err();
            assert!(
                format!("{err:?}").contains("is a loopback address"),
                "{err:?}"
            );
        }

        #[tokio::test]
        async fn the_same_name_connects_when_loopback_is_allowed() {
            let port = serve_once();
            let response = client(policy(true))
                .get(format!("http://localhost:{port}/"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 204);
        }

        #[tokio::test]
        async fn an_exempted_host_name_connects_under_a_strict_policy() {
            let port = serve_once();
            let exempt = policy(false);
            exempt.allow_host(&format!("localhost:{port}"));
            let response = client(exempt)
                .get(format!("http://localhost:{port}/"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 204);
        }

        /// The resolver never sees an IP address, which is why a client also
        /// checks the URL before it sends.
        #[tokio::test]
        async fn a_literal_ip_skips_the_resolver_and_needs_the_url_check() {
            let port = serve_once();
            let strict = policy(false);
            let url = format!("http://127.0.0.1:{port}/");
            assert!(
                strict.validate_url(&url).is_err(),
                "the URL check refuses it"
            );
            let response = client(strict).get(&url).send().await.unwrap();
            assert_eq!(response.status(), 204, "the resolver alone does not");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    /// (address, allowed by strict, by private, by loopback), as Go's table.
    #[test]
    fn check_ip_follows_the_policy() {
        let strict = Policy::strict();
        let private = Policy::strict();
        private.set_allow_private(true);
        let loopback = Policy::strict();
        loopback.set_allow_loopback(true);
        let cases = [
            ("8.8.8.8", true, true, true),
            ("2606:4700:4700::1111", true, true, true),
            ("127.0.0.1", false, false, true),
            ("::1", false, false, true),
            ("::ffff:127.0.0.1", false, false, true),
            ("10.1.2.3", false, true, false),
            ("172.16.0.1", false, true, false),
            ("192.168.1.1", false, true, false),
            ("100.64.0.1", false, true, false),
            ("fd12::1", false, true, false),
            ("169.254.169.254", false, false, false),
            ("::ffff:169.254.169.254", false, false, false),
            ("fe80::1", false, false, false),
            ("fd00:ec2::254", false, false, false),
            ("0.0.0.0", false, false, false),
            ("::", false, false, false),
            ("224.0.0.1", false, false, false),
        ];
        for (addr, want_strict, want_private, want_loopback) in cases {
            for (name, policy, want) in [
                ("strict", &strict, want_strict),
                ("private", &private, want_private),
                ("loopback", &loopback, want_loopback),
            ] {
                let result = policy.check_ip(ip(addr));
                assert_eq!(result.is_ok(), want, "{name} policy, {addr}: {result:?}");
                if let Err(e) = result {
                    assert!(matches!(e, Rejected::Blocked(_)), "{addr}: {e:?}");
                }
            }
        }
    }

    #[test]
    fn validate_url_accepts_public_and_rejects_the_rest() {
        let strict = Policy::strict();
        for ok in [
            "https://example.com/hook",
            "http://8.8.8.8:8080/x",
            "https://hooks.internal.example.com/a",
        ] {
            assert_eq!(strict.validate_url(ok), Ok(()), "{ok}");
        }
        for bad in [
            "",
            "ftp://example.com",
            "example.com/hook",
            "https://",
            "https://user:pw@example.com/",
            "http://localhost/x",
            "http://LOCALHOST:8080/x",
            "http://foo.localhost/x",
            "http://127.0.0.1/x",
            "http://[::1]/x",
            "http://169.254.169.254/latest/meta-data",
            "http://10.0.0.5/x",
            "http://0.0.0.0/",
            "http://[fd00:ec2::254]/",
            // Numeric forms of loopback normalise to 127.0.0.1.
            "http://2130706433/",
            "http://127.1/",
        ] {
            assert!(strict.validate_url(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn a_dev_policy_still_blocks_cloud_metadata() {
        let dev = Policy::strict();
        dev.set_allow_loopback(true);
        dev.set_allow_private(true);
        for ok in [
            "http://localhost:9000/x",
            "http://127.0.0.1/x",
            "http://10.0.0.5/x",
        ] {
            assert_eq!(dev.validate_url(ok), Ok(()), "{ok}");
        }
        assert!(dev.validate_url("http://169.254.169.254/").is_err());
    }

    #[test]
    fn messages_read_as_go_words_them() {
        let strict = Policy::strict();
        let text = |raw: &str| strict.validate_url(raw).unwrap_err().to_string();
        assert_eq!(text("ftp://example.com"), "must be an http or https URL");
        assert_eq!(text("example.com/hook"), "must be an http or https URL");
        assert_eq!(text("https://"), "must include a host");
        assert_eq!(
            text("https://u:p@example.com/"),
            "must not embed credentials"
        );
        assert_eq!(
            text("http://127.0.0.1/"),
            "destination not allowed: 127.0.0.1 is a loopback address"
        );
        assert_eq!(
            text("http://localhost/"),
            "destination not allowed: localhost is a loopback name"
        );
    }

    #[test]
    fn an_allowed_host_skips_the_checks_on_that_port_only() {
        let policy = Policy::strict();
        policy.allow_url("http://localhost:8095/fn");
        assert_eq!(policy.validate_url("http://localhost:8095/other"), Ok(()));
        assert!(policy.validate_url("http://localhost:9999/other").is_err());
    }

    #[test]
    fn allowed_host_patterns_use_wildcards() {
        let policy = Policy::strict();
        policy.allow_host("*.fn.svc:8095");
        policy.allow_url("http://{pool}.runners.svc:8095");
        assert_eq!(policy.validate_url("http://a.fn.svc:8095/x"), Ok(()));
        assert_eq!(
            policy.validate_url("http://default.runners.svc:8095/x"),
            Ok(())
        );
        // A name passes either way; what a pattern exempts is an address.
        let ip_policy = Policy::strict();
        ip_policy.allow_host("10.*:8095");
        assert_eq!(ip_policy.validate_url("http://10.1.2.3:8095/x"), Ok(()));
        assert!(ip_policy.validate_url("http://10.1.2.3:9000/x").is_err());
        assert!(ip_policy.validate_url("http://11.1.2.3:8095/x").is_ok());
    }

    #[test]
    fn glob_match_handles_star_positions() {
        assert!(glob_match("*.fn.svc:8095", "a.fn.svc:8095"));
        assert!(!glob_match("*.fn.svc:8095", "evil.example.com:8095"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXcYYb"));
        assert!(glob_match("*", "anything:1"));
        assert!(glob_match("exact:80", "exact:80"));
        assert!(!glob_match("exact:80", "exact:81"));
        assert!(!glob_match("ab*ba", "aba"), "the ends may not overlap");
    }

    #[test]
    fn from_env_reads_the_three_variables() {
        // The variables are process-wide, so this test only reads what it
        // sets and restores it.
        env::set_var("FC_DELIVERY_ALLOW_LOOPBACK", "true");
        env::set_var("FC_DELIVERY_ALLOW_HOSTS", "a.example:1, b.example:2 ,");
        let policy = Policy::from_env();
        env::remove_var("FC_DELIVERY_ALLOW_LOOPBACK");
        env::remove_var("FC_DELIVERY_ALLOW_HOSTS");
        assert!(policy.allow_loopback.load(Ordering::Relaxed));
        assert!(!policy.allow_private.load(Ordering::Relaxed));
        assert!(policy.host_allowed("a.example:1"));
        assert!(policy.host_allowed("b.example:2"));
        assert!(!policy.host_allowed("c.example:3"));
    }
}

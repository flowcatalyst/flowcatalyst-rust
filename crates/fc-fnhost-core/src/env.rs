//! The host process's environment (Java `fnhost/reconcile/HostEnv.java`,
//! read through `server/EnvReader.java`'s lookup rules).

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;

use rand::Rng;

use crate::signature::{Signatures, SignaturesMode};

/// A read-only view over an environment map with Java `EnvReader`'s rules:
/// unset and empty are the same thing, and an unparseable value silently
/// falls back to the default.
#[derive(Debug, Clone, Default)]
pub struct EnvReader {
    vars: HashMap<String, String>,
}

impl EnvReader {
    /// The process environment (non-UTF-8 values are read lossily).
    pub fn system() -> Self {
        Self {
            vars: std::env::vars_os()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
        }
    }

    pub fn from_pairs<K: Into<String>, V: Into<String>>(
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self {
            vars: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// `os.Getenv`: the value, or `""` when unset.
    pub fn get(&self, key: &str) -> &str {
        self.vars.get(key).map(String::as_str).unwrap_or("")
    }

    /// The value, or `default` when unset or empty.
    pub fn or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        match self.get(key) {
            "" => default,
            v => v,
        }
    }

    /// The first non-empty value among `keys`.
    pub fn first_set(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().map(|k| self.get(k)).find(|v| !v.is_empty())
    }

    /// A base-10 integer with an optional sign; `default` when unset or
    /// unparseable (Java `Integer.parseInt`).
    pub fn integer(&self, key: &str, default: i32) -> i32 {
        self.get(key).parse().unwrap_or(default)
    }

    /// `1/true/yes/on` and `0/false/no/off`, trimmed and case-insensitive;
    /// anything else is `default`.
    pub fn bool(&self, key: &str, default: bool) -> bool {
        parse_bool(self.get(key)).unwrap_or(default)
    }
}

pub(crate) fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// A DNS label: 1-63 characters of `a-z`, `0-9` and `-`, not starting or
/// ending with `-`. No normalisation.
pub fn is_dns_label(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

/// The heartbeat's host-id rule: 1-100 characters of `[A-Za-z0-9._:-]`.
pub fn is_host_id(raw: &str) -> bool {
    !raw.is_empty() && raw.len() <= 100 && raw.bytes().all(host_id_byte)
}

fn host_id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-')
}

/// `FC_FN_PUBLIC_PORT`: a port, or no public listener at all (`off`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicPort {
    Port(u16),
    Disabled,
}

/// The CIDR allow-list the public listener trusts an `X-Forwarded-For` from
/// (Java `fnhost/route/TrustedProxies.java`). Entries are IP literals with
/// an optional prefix length; an address alone is a full-length prefix.
#[derive(Clone, PartialEq, Eq)]
pub struct TrustedProxies {
    cidrs: Vec<(IpAddr, u8)>,
}

impl TrustedProxies {
    /// RFC 1918 + loopback + IPv6 ULA/loopback.
    pub fn default_list() -> Self {
        Self::parse_csv("127.0.0.0/8,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,::1/128,fc00::/7")
            .expect("the default list parses")
    }

    /// Comma-separated; blank means the default list, not "trust nobody".
    pub fn parse_csv(raw: &str) -> Result<Self, String> {
        if crate::java::is_blank(raw) {
            return Ok(Self::default_list());
        }
        let mut cidrs = Vec::new();
        for entry in raw.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let (host, prefix) = match entry.split_once('/') {
                Some((host, prefix)) => (host, Some(prefix.trim())),
                None => (entry, None),
            };
            let address: IpAddr = host
                .parse()
                .map_err(|_| format!("not a valid CIDR/address: '{entry}'"))?;
            let max = if address.is_ipv4() { 32 } else { 128 };
            let prefix = match prefix {
                Some(p) => p
                    .parse::<i32>()
                    .map_err(|_| format!("not a valid CIDR prefix length: '{entry}'"))?,
                None => max,
            };
            if !(0..=max).contains(&prefix) {
                return Err(format!(
                    "prefix length out of range for '{entry}' (0..{max})"
                ));
            }
            cidrs.push((address, prefix as u8));
        }
        Ok(Self { cidrs })
    }

    /// Whether `address` falls inside any configured network (IPv4 against
    /// IPv4, IPv6 against IPv6; an IPv4-mapped IPv6 address is unwrapped
    /// first, as the JDK's `InetAddress` does).
    pub fn is_trusted(&self, address: IpAddr) -> bool {
        let address = match address {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(address),
            v4 => v4,
        };
        self.cidrs
            .iter()
            .any(|(network, prefix)| match (network, address) {
                (IpAddr::V4(n), IpAddr::V4(a)) => prefix_matches(&n.octets(), &a.octets(), *prefix),
                (IpAddr::V6(n), IpAddr::V6(a)) => prefix_matches(&n.octets(), &a.octets(), *prefix),
                _ => false,
            })
    }
}

fn prefix_matches(network: &[u8], candidate: &[u8], prefix: u8) -> bool {
    let full = usize::from(prefix / 8);
    let rest = prefix % 8;
    if network[..full] != candidate[..full] {
        return false;
    }
    if rest > 0 {
        let mask = 0xFFu8 << (8 - rest);
        return network[full] & mask == candidate[full] & mask;
    }
    true
}

impl fmt::Debug for TrustedProxies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TrustedProxies[{} network(s)]", self.cidrs.len())
    }
}

/// Every `FC_FN_*` variable the host reads, with Java's defaults. The
/// client secret is masked out of `Debug`.
#[derive(Clone)]
pub struct HostEnv {
    /// `FC_FN_POOL` (default `default`, a DNS label).
    pub pool: String,
    /// `FC_FN_PLATFORM_URL` (required).
    pub platform_url: String,
    /// `FC_FN_CLIENT_ID` (required).
    pub client_id: String,
    /// `FC_FN_CLIENT_SECRET` (required).
    pub client_secret: String,
    /// `FC_FN_HOST_ID` (default `<hostname>-<6 random base32>`).
    pub host_id: String,
    /// `FC_FN_SIGNATURES` + `FLOWCATALYST_DEV_MODE` + `FC_FN_TRUST_ROOT`.
    pub signatures: Signatures,
    /// `FC_FN_MAX_LOADED` (default 200).
    pub max_loaded: usize,
    /// `FC_FN_CACHE_DIR` (default `<tmp>/fc-fn-cache`).
    pub cache_dir: PathBuf,
    /// `FC_FN_PORT` (default 8080): the private function listener.
    pub port: u16,
    /// `FC_FN_MAX_CONCURRENCY` (default 512): the host-wide permit ceiling.
    pub max_concurrency: i32,
    /// `FC_DRAIN_TIMEOUT_SECONDS` (default 60).
    pub drain_timeout_seconds: u64,
    /// `FC_METRICS_PORT` (default 9090): `/health`, `/ready`, `/metrics`.
    pub metrics_port: u16,
    /// `FC_EXIT_AFTER_START` (default false): exit 0 right after start-up.
    pub exit_after_start: bool,
    /// `FC_FN_MAX_DB_POOLS` (default 16). Read for parity; the Rust host has
    /// no database access yet (Java W4, owner decision 3).
    pub max_db_pools: i32,
    /// `FC_FN_PUBLIC_PORT` (default 8081, `off` disables, `0` ephemeral).
    pub public_port: PublicPort,
    /// `FC_FN_TRUSTED_PROXIES` (default RFC 1918 + loopback + ULA).
    pub trusted_proxies: TrustedProxies,
}

impl fmt::Debug for HostEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostEnv")
            .field("pool", &self.pool)
            .field("platform_url", &self.platform_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("host_id", &self.host_id)
            .field("signatures", &self.signatures)
            .field("max_loaded", &self.max_loaded)
            .field("cache_dir", &self.cache_dir)
            .field("port", &self.port)
            .field("max_concurrency", &self.max_concurrency)
            .field("drain_timeout_seconds", &self.drain_timeout_seconds)
            .field("metrics_port", &self.metrics_port)
            .field("exit_after_start", &self.exit_after_start)
            .field("max_db_pools", &self.max_db_pools)
            .field("public_port", &self.public_port)
            .field("trusted_proxies", &self.trusted_proxies)
            .finish()
    }
}

/// A start-up environment error: one line, printed to stderr before the
/// process exits 2.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HostEnvError(pub String);

pub const DEFAULT_PUBLIC_PORT: u16 = 8081;

impl HostEnv {
    pub fn load(env: &EnvReader) -> Result<Self, HostEnvError> {
        Self::load_with(env, default_host_id)
    }

    /// `default_host_id` supplies the id when `FC_FN_HOST_ID` is unset: a
    /// seam so a test can pin it.
    pub fn load_with(
        env: &EnvReader,
        default_host_id: impl FnOnce() -> String,
    ) -> Result<Self, HostEnvError> {
        let mut bad: Vec<String> = Vec::new();

        let pool_raw = env.or("FC_FN_POOL", "default");
        if !is_dns_label(pool_raw) {
            bad.push(format!("FC_FN_POOL (not a valid DNS label: '{pool_raw}')"));
        }
        let platform_url = required(env, "FC_FN_PLATFORM_URL", &mut bad);
        let client_id = required(env, "FC_FN_CLIENT_ID", &mut bad);
        let client_secret = required(env, "FC_FN_CLIENT_SECRET", &mut bad);

        let host_id_raw = env.get("FC_FN_HOST_ID");
        let host_id = if crate::java::is_blank(host_id_raw) {
            default_host_id()
        } else {
            if !is_host_id(host_id_raw) {
                bad.push(format!(
                    "FC_FN_HOST_ID (not 1-100 characters of [A-Za-z0-9._:-]: '{host_id_raw}')"
                ));
            }
            host_id_raw.to_owned()
        };

        let max_loaded = env.integer("FC_FN_MAX_LOADED", 200);
        if max_loaded < 1 {
            bad.push(format!(
                "FC_FN_MAX_LOADED (must be at least 1: '{}')",
                env.get("FC_FN_MAX_LOADED")
            ));
        }
        let function_port = port(env, "FC_FN_PORT", 8080, &mut bad);
        let metrics_port = port(env, "FC_METRICS_PORT", 9090, &mut bad);
        let drain_timeout = env.integer("FC_DRAIN_TIMEOUT_SECONDS", 60).max(0);
        let public_port = public_port(env, &mut bad);
        let trusted_proxies = match TrustedProxies::parse_csv(env.get("FC_FN_TRUSTED_PROXIES")) {
            Ok(list) => list,
            Err(_) => {
                bad.push(format!(
                    "FC_FN_TRUSTED_PROXIES (not a valid CIDR list: '{}')",
                    env.get("FC_FN_TRUSTED_PROXIES")
                ));
                TrustedProxies::default_list()
            }
        };

        if !bad.is_empty() {
            return Err(HostEnvError(format!(
                "missing or invalid required environment variable(s): {}",
                bad.join(", ")
            )));
        }

        let signatures = Signatures::resolve(
            SignaturesMode::parse(env.or("FC_FN_SIGNATURES", "required")),
            env.bool("FLOWCATALYST_DEV_MODE", false),
            env.get("FC_FN_TRUST_ROOT"),
        )
        .map_err(HostEnvError)?;

        let cache_dir = match env.get("FC_FN_CACHE_DIR") {
            "" => std::env::temp_dir().join("fc-fn-cache"),
            dir => PathBuf::from(dir),
        };

        Ok(Self {
            pool: pool_raw.to_owned(),
            platform_url,
            client_id,
            client_secret,
            host_id,
            signatures,
            max_loaded: max_loaded.max(1) as usize,
            cache_dir,
            port: function_port,
            max_concurrency: env.integer("FC_FN_MAX_CONCURRENCY", 512),
            drain_timeout_seconds: drain_timeout as u64,
            metrics_port,
            exit_after_start: env.bool("FC_EXIT_AFTER_START", false),
            max_db_pools: env.integer("FC_FN_MAX_DB_POOLS", 16),
            public_port,
            trusted_proxies,
        })
    }
}

fn required(env: &EnvReader, key: &str, bad: &mut Vec<String>) -> String {
    let value = env.get(key);
    if crate::java::is_blank(value) {
        bad.push(key.to_owned());
    }
    value.to_owned()
}

fn port(env: &EnvReader, key: &str, default: i32, bad: &mut Vec<String>) -> u16 {
    let value = env.integer(key, default);
    u16::try_from(value).unwrap_or_else(|_| {
        bad.push(format!("{key} (not a port number: '{}')", env.get(key)));
        0
    })
}

fn public_port(env: &EnvReader, bad: &mut Vec<String>) -> PublicPort {
    let raw = env.get("FC_FN_PUBLIC_PORT").trim();
    if raw.is_empty() {
        return PublicPort::Port(DEFAULT_PUBLIC_PORT);
    }
    if raw.eq_ignore_ascii_case("off") {
        return PublicPort::Disabled;
    }
    match raw.parse::<i32>().ok().and_then(|p| u16::try_from(p).ok()) {
        Some(port) => PublicPort::Port(port),
        None => {
            bad.push(format!(
                "FC_FN_PUBLIC_PORT (must be a non-negative port number or 'off': '{raw}')"
            ));
            PublicPort::Port(DEFAULT_PUBLIC_PORT)
        }
    }
}

const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// `<hostname>-<6 random base32>`, the hostname sanitised to the host-id
/// alphabet.
pub fn default_host_id() -> String {
    let raw = gethostname::gethostname().to_string_lossy().into_owned();
    let sanitised: String = raw
        .bytes()
        .map(|b| if host_id_byte(b) { b as char } else { '-' })
        .collect();
    let hostname = if crate::java::is_blank(&sanitised) {
        "fn-host".to_owned()
    } else {
        sanitised
    };
    let mut rng = rand::rng();
    let suffix: String = (0..6)
        .map(|_| BASE32_ALPHABET[rng.random_range(0..BASE32_ALPHABET.len())] as char)
        .collect();
    format!("{hostname}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Vec<(&'static str, &'static str)> {
        vec![
            ("FC_FN_PLATFORM_URL", "http://platform"),
            ("FC_FN_CLIENT_ID", "id"),
            ("FC_FN_CLIENT_SECRET", "secret"),
            ("FC_FN_SIGNATURES", "off"),
            ("FLOWCATALYST_DEV_MODE", "true"),
        ]
    }

    fn load(pairs: Vec<(&'static str, &'static str)>) -> Result<HostEnv, HostEnvError> {
        HostEnv::load_with(&EnvReader::from_pairs(pairs), || "host-ABCDEF".into())
    }

    #[test]
    fn defaults_match_java() {
        let env = load(base()).unwrap();
        assert_eq!(env.pool, "default");
        assert_eq!(env.host_id, "host-ABCDEF");
        assert_eq!(env.max_loaded, 200);
        assert_eq!(env.port, 8080);
        assert_eq!(env.max_concurrency, 512);
        assert_eq!(env.drain_timeout_seconds, 60);
        assert_eq!(env.metrics_port, 9090);
        assert!(!env.exit_after_start);
        assert_eq!(env.max_db_pools, 16);
        assert_eq!(env.public_port, PublicPort::Port(8081));
        assert_eq!(env.trusted_proxies, TrustedProxies::default_list());
        assert!(env.cache_dir.ends_with("fc-fn-cache"));
        assert!(matches!(env.signatures, Signatures::Off));
    }

    #[test]
    fn every_missing_required_variable_is_named_in_one_line() {
        let err = load(vec![
            ("FC_FN_POOL", "Bad_Pool"),
            ("FC_FN_HOST_ID", "has space"),
        ])
        .unwrap_err();
        let line = err.to_string();
        assert!(!line.contains('\n'));
        for name in [
            "FC_FN_POOL",
            "FC_FN_PLATFORM_URL",
            "FC_FN_CLIENT_ID",
            "FC_FN_CLIENT_SECRET",
            "FC_FN_HOST_ID",
        ] {
            assert!(line.contains(name), "{name} missing from {line}");
        }
        assert!(line.starts_with("missing or invalid required environment variable(s): "));
    }

    #[test]
    fn blank_required_values_count_as_missing() {
        let mut pairs = base();
        pairs.push(("FC_FN_CLIENT_SECRET", "   "));
        let err = load(pairs).unwrap_err();
        assert!(err.0.contains("FC_FN_CLIENT_SECRET"));
    }

    #[test]
    fn signatures_off_outside_dev_mode_is_refused() {
        let mut pairs = base();
        pairs.retain(|(k, _)| *k != "FLOWCATALYST_DEV_MODE");
        let err = load(pairs).unwrap_err();
        assert!(err
            .0
            .contains("FC_FN_SIGNATURES=off requires FLOWCATALYST_DEV_MODE=true"));
    }

    #[test]
    fn signatures_default_to_required() {
        let mut pairs = base();
        pairs.retain(|(k, _)| *k != "FC_FN_SIGNATURES");
        assert!(matches!(
            load(pairs).unwrap().signatures,
            Signatures::Required(_)
        ));
    }

    #[test]
    fn unparseable_numbers_fall_back_to_defaults() {
        let mut pairs = base();
        pairs.push(("FC_FN_MAX_LOADED", "lots"));
        pairs.push(("FC_EXIT_AFTER_START", "YES"));
        let env = load(pairs).unwrap();
        assert_eq!(env.max_loaded, 200);
        assert!(env.exit_after_start);
    }

    #[test]
    fn public_port_and_trusted_proxies() {
        let mut pairs = base();
        pairs.push(("FC_FN_PUBLIC_PORT", "OFF"));
        pairs.push(("FC_FN_TRUSTED_PROXIES", "10.1.0.0/16, 2001:db8::/32"));
        let env = load(pairs).unwrap();
        assert_eq!(env.public_port, PublicPort::Disabled);
        assert!(env.trusted_proxies.is_trusted("10.1.2.3".parse().unwrap()));
        assert!(!env.trusted_proxies.is_trusted("10.2.0.1".parse().unwrap()));
        assert!(env
            .trusted_proxies
            .is_trusted("2001:db8::1".parse().unwrap()));

        let mut pairs = base();
        pairs.push(("FC_FN_PUBLIC_PORT", "-1"));
        pairs.push(("FC_FN_TRUSTED_PROXIES", "10.0.0.0/40"));
        let err = load(pairs).unwrap_err().0;
        assert!(err.contains("FC_FN_PUBLIC_PORT") && err.contains("FC_FN_TRUSTED_PROXIES"));
    }

    #[test]
    fn default_host_id_has_the_right_shape() {
        let id = default_host_id();
        assert!(is_host_id(&id), "{id}");
        let suffix = id.rsplit('-').next().unwrap();
        assert_eq!(suffix.len(), 6);
    }

    #[test]
    fn debug_masks_the_secret() {
        let env = load(base()).unwrap();
        let text = format!("{env:?}");
        assert!(!text.contains("secret\""));
        assert!(text.contains("<redacted>"));
    }
}

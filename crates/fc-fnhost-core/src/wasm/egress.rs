//! Outbound HTTP from a guest (`wasi:http/outgoing-handler`) under the
//! manifest's `httpAllow` (Java `context/{AllowlistHttpCaller,
//! HttpAllowlist}.java`, spec `function-context.md` §2):
//!
//! - the host must be on the allow list: an exact host, or `*.suffix`
//!   matching subdomains only (never the apex, never a host that merely ends
//!   with the suffix string);
//! - `https` only, except to loopback (`localhost`, `127.0.0.1`, `::1`);
//! - redirects are never followed (hyper does not follow them): the guest
//!   sees the 3xx;
//! - every timeout the guest set is capped at the smaller of the time left
//!   before the invocation's deadline and 30 s.
//!
//! A refusal is the typed `wasi:http` error `HTTP-request-denied`, not
//! Java's status-0 reply: the guest's HTTP client reports it as the error it
//! is.

use std::future::Future;
use std::time::{Duration, Instant};

use http_body_util::BodyExt;
use wasmtime_wasi_http::{RequestOptions, WasiBody, WasiHttpHooks};

/// Java `AllowlistHttpCaller.DEFAULT_CALL_TIMEOUT`.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// The `manifest.httpAllow` matcher (Java `HttpAllowlist`).
#[derive(Debug, Clone, Default)]
pub struct HttpAllowlist {
    exact: Vec<String>,
    suffixes: Vec<String>,
}

impl HttpAllowlist {
    pub fn new<'a>(entries: impl IntoIterator<Item = &'a str>) -> Self {
        let mut list = Self::default();
        for entry in entries {
            match entry.strip_prefix("*.") {
                Some(suffix) => list.suffixes.push(suffix.to_lowercase()),
                None => list.exact.push(entry.to_lowercase()),
            }
        }
        list
    }

    pub fn allows(&self, host: &str) -> bool {
        if host.trim().is_empty() {
            return false;
        }
        let host = host.to_lowercase();
        self.exact.contains(&host)
            || self.suffixes.iter().any(|suffix| {
                host.len() > suffix.len() + 1
                    && host.ends_with(suffix.as_str())
                    && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
            })
    }
}

/// Java `HttpAllowlist.isLoopback`: the spellings, not a range.
pub fn is_loopback(host: &str) -> bool {
    matches!(
        host.to_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1" | "[::1]"
    )
}

/// Why a call was refused (for the host's DEBUG line; the guest sees only
/// `HTTP-request-denied`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow { timeout: Duration },
    Deny(String),
}

/// The policy check, before anything leaves the host.
pub fn decide(allow: &HttpAllowlist, scheme: &str, host: &str, deadline: Instant) -> Decision {
    if !scheme.eq_ignore_ascii_case("https") && !is_loopback(host) {
        return Decision::Deny(format!(
            "scheme '{scheme}' is not permitted (https only, except loopback)"
        ));
    }
    if !allow.allows(host) {
        return Decision::Deny(format!("{host}: not on this function's httpAllow list"));
    }
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .min(DEFAULT_CALL_TIMEOUT);
    if timeout.is_zero() {
        return Decision::Deny("no time left before the invocation deadline".into());
    }
    Decision::Allow { timeout }
}

/// The per-store `wasi:http` hooks: the policy, and the invocation's
/// deadline.
pub struct EgressHooks {
    pub allow: std::sync::Arc<HttpAllowlist>,
    pub deadline: Instant,
}

type SendResult = wasmtime_wasi_http::Result<(
    http::Response<WasiBody>,
    Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
)>;

impl WasiHttpHooks for EgressHooks {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        fut: Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
    ) -> Box<dyn Future<Output = SendResult> + Send> {
        drop(fut);
        let scheme = request.uri().scheme_str().unwrap_or("").to_owned();
        let host = request.uri().host().unwrap_or("").to_owned();
        match decide(&self.allow, &scheme, &host, self.deadline) {
            Decision::Deny(why) => {
                tracing::debug!(host = %host, reason = %why, "outbound call refused");
                Box::new(async { Err(wasmtime_wasi_http::Error::HttpRequestDenied) })
            }
            Decision::Allow { timeout } => {
                let cap = |requested: Option<Duration>| {
                    Some(requested.map_or(timeout, |t| t.min(timeout)))
                };
                let mut options = options.unwrap_or_default();
                options.connect_timeout = cap(options.connect_timeout);
                options.first_byte_timeout = cap(options.first_byte_timeout);
                options.between_bytes_timeout = cap(options.between_bytes_timeout);
                Box::new(async move {
                    let sent = tokio::time::timeout(
                        timeout,
                        wasmtime_wasi_http::default_send_request(request, Some(options)),
                    );
                    let (response, io) = match sent.await {
                        Ok(result) => result?,
                        Err(_) => return Err(wasmtime_wasi_http::Error::HttpResponseTimeout),
                    };
                    Ok((
                        response.map(BodyExt::boxed_unsync),
                        Box::new(io) as Box<dyn Future<Output = _> + Send>,
                    ))
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Java HttpAllowlistTest
    #[test]
    fn a_suffix_matches_subdomains_only() {
        let allow = HttpAllowlist::new(["api.example.com", "*.suffix.test"]);
        assert!(allow.allows("api.example.com"));
        assert!(allow.allows("API.Example.com"));
        assert!(allow.allows("a.suffix.test"));
        assert!(allow.allows("a.b.suffix.test"));
        assert!(!allow.allows("suffix.test"), "never the apex");
        assert!(
            !allow.allows("evilsuffix.test"),
            "a label boundary, not endsWith"
        );
        assert!(!allow.allows("example.com"));
        assert!(!allow.allows(""));
    }

    #[test]
    fn https_only_except_loopback_and_the_deadline_caps_the_timeout() {
        let allow = HttpAllowlist::new(["127.0.0.1", "api.example.com", "localhost"]);
        let later = Instant::now() + Duration::from_secs(120);
        assert!(matches!(
            decide(&allow, "http", "api.example.com", later),
            Decision::Deny(why) if why.contains("https only")
        ));
        assert_eq!(
            decide(&allow, "https", "api.example.com", later),
            Decision::Allow {
                timeout: DEFAULT_CALL_TIMEOUT
            }
        );
        assert!(matches!(
            decide(&allow, "http", "127.0.0.1", Instant::now() + Duration::from_secs(2)),
            Decision::Allow { timeout } if timeout <= Duration::from_secs(2)
        ));
        assert!(matches!(
            decide(&allow, "https", "other.example.com", later),
            Decision::Deny(why) if why.contains("httpAllow")
        ));
        assert!(matches!(
            decide(&allow, "https", "api.example.com", Instant::now()),
            Decision::Deny(why) if why.contains("no time left")
        ));
    }
}

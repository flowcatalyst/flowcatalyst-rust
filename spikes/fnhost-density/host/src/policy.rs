//! Outbound HTTP policy (Java `AllowlistHttpCaller`), shared by every engine variant:
//! the host must be on the function's allowlist; `https` only, except loopback; redirects are
//! never followed; timeout = min(call timeout or 30 s, time left before the invocation
//! deadline). Plus a blocking client for (a)/(b) and a loopback test server.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default)]
pub struct EgressPolicy {
    /// Exact hosts, or `*.suffix`.
    pub allow: Vec<String>,
}

#[derive(Debug)]
pub enum Decision {
    Allow { timeout: Duration },
    Deny(String),
}

pub fn is_loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost")
        || h.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

impl EgressPolicy {
    pub fn allows_host(&self, host: &str) -> bool {
        self.allow.iter().any(|p| match p.strip_prefix("*.") {
            Some(suffix) => host.len() > suffix.len() + 1 && host.ends_with(&format!(".{suffix}")),
            None => p.eq_ignore_ascii_case(host),
        })
    }

    /// `scheme` and `host` as parsed from the request URL.
    pub fn check(&self, scheme: &str, host: &str, call_timeout: Option<Duration>, deadline: Instant) -> Decision {
        if !scheme.eq_ignore_ascii_case("https") && !is_loopback(host) {
            return Decision::Deny(format!("scheme '{scheme}' is not permitted (https only, except loopback)"));
        }
        if !self.allows_host(host) {
            return Decision::Deny(format!("{host}: not on this function's httpAllow list"));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = remaining.min(call_timeout.unwrap_or(DEFAULT_CALL_TIMEOUT));
        if timeout.is_zero() {
            return Decision::Deny("no time left before the invocation deadline".into());
        }
        Decision::Allow { timeout }
    }
}

/// Split `scheme://host[:port]/...` without a URL crate.
pub fn scheme_host(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map(|(_, a)| a).unwrap_or(authority);
    let host = if authority.starts_with('[') {
        authority.split_once(']').map(|(h, _)| format!("{h}]"))?
    } else {
        authority.split(':').next()?.to_string()
    };
    Some((scheme.to_string(), host))
}

pub struct Reply {
    pub status: u16,
    /// Repeated headers flattened with ", " (RFC 9110 §5.3), first-seen order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// One outbound call under the policy, blocking. `Err` = no HTTP response (denied, timed out,
/// transport failure) — the caller turns it into status 0 + `{"error": …}`.
pub fn send_blocking(
    policy: &EgressPolicy,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    deadline: Instant,
) -> Result<Reply, String> {
    let (scheme, host) = scheme_host(url).ok_or_else(|| "not a valid URL".to_string())?;
    let timeout = match policy.check(&scheme, &host, None, deadline) {
        Decision::Allow { timeout } => timeout,
        Decision::Deny(why) => return Err(why),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut req = ureq::http::Request::builder().method(method).uri(url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let req = req.body(body.to_vec()).map_err(|e| format!("bad request: {e}"))?;
    let mut res = match agent.run(req) {
        Ok(r) => r,
        Err(ureq::Error::Timeout(_)) => return Err("outbound call timed out".into()),
        Err(e) => return Err(format!("outbound call failed: {e}")),
    };
    let mut flat: Vec<(String, String)> = Vec::new();
    for (name, value) in res.headers() {
        let v = value.to_str().unwrap_or("").to_string();
        match flat.iter_mut().find(|(n, _)| n == name.as_str()) {
            Some((_, existing)) => {
                existing.push_str(", ");
                existing.push_str(&v);
            }
            None => flat.push((name.as_str().to_string(), v)),
        }
    }
    let status = res.status().as_u16();
    let body = res.body_mut().read_to_vec().map_err(|e| format!("reading the response failed: {e}"))?;
    Ok(Reply { status, headers: flat, body })
}

/// `{"error": why}` — the body a guest sees with status 0.
pub fn denial_body(why: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "error": why })).unwrap()
}

/// Headers as the Extism PDK reads them: a flat JSON object of strings.
pub fn headers_json(h: &[(String, String)]) -> Vec<u8> {
    let m: serde_json::Map<String, serde_json::Value> =
        h.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect();
    serde_json::to_vec(&m).unwrap()
}

/// A loopback HTTP/1.1 server for the egress tests:
/// `/ok` 200 with two `x-upstream` headers; `/redirect` 302 → `/ok`; `/slow` answers after 2 s.
pub fn start_test_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let from_guest = head.lines().any(|l| l.to_ascii_lowercase().starts_with("x-from-guest: yes"));
                let resp = match path.as_str() {
                    "/ok" => format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\nx-upstream: a\r\nx-upstream: b\r\n\
                         content-length: 5\r\nconnection: close\r\n\r\n{}",
                        if from_guest { "hello" } else { "nohdr" }
                    ),
                    "/redirect" => "HTTP/1.1 302 Found\r\nlocation: /ok\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
                    "/slow" => {
                        std::thread::sleep(Duration::from_secs(2));
                        "HTTP/1.1 200 OK\r\ncontent-length: 4\r\nconnection: close\r\n\r\nslow".into()
                    }
                    _ => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
                };
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    port
}

/// Per-function host context: what the manifest declares.
#[derive(Clone, Default)]
pub struct FnCtx {
    pub address: String,
    pub config: HashMap<String, String>,
    pub secrets: HashMap<String, String>,
    pub policy: EgressPolicy,
    pub logs: crate::util::LineSink,
}

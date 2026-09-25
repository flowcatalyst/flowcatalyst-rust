//! Outbound HTTP (Java's `HttpCaller`, `HttpCall`, `HttpReply`,
//! `HttpCallRefusedException`), over `wasi:http/outgoing-handler`.

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use fc_function_abi::MultiMap;

use crate::backend::Backend;

/// The outbound HTTP client ([`Context::http`](crate::Context::http)). The
/// host lets a call through only to a host on the manifest's `httpAllow`
/// (exact, or `*.suffix` for subdomains), over `https` (plain `http` only to
/// loopback); it never follows a redirect, and caps the timeout at the time
/// left before the invocation's deadline and 30 s.
#[derive(Clone)]
pub struct Http {
    backend: Rc<dyn Backend>,
}

impl Http {
    pub(crate) fn new(backend: Rc<dyn Backend>) -> Self {
        Self { backend }
    }

    /// Makes one call. A refusal by the host's policy is
    /// [`HttpError::Denied`]; any status, 4xx and 5xx included, is an
    /// `Ok` reply.
    pub async fn send(&self, call: HttpCall) -> Result<HttpReply, HttpError> {
        self.backend.send(call).await
    }

    /// `GET url`.
    pub async fn get(&self, url: impl Into<String>) -> Result<HttpReply, HttpError> {
        self.send(HttpCall::get(url)).await
    }
}

impl fmt::Debug for Http {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http").finish_non_exhaustive()
    }
}

/// An outbound request (Java's `HttpCall`): method, absolute URL, headers,
/// body and an optional timeout of its own.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpCall {
    method: String,
    url: String,
    headers: MultiMap,
    body: Vec<u8>,
    timeout: Option<Duration>,
}

impl HttpCall {
    /// `method url`, with no headers and no body.
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: MultiMap::new(),
            body: Vec::new(),
            timeout: None,
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new("GET", url)
    }

    pub fn post(url: impl Into<String>) -> Self {
        Self::new("POST", url)
    }

    pub fn put(url: impl Into<String>) -> Self {
        Self::new("PUT", url)
    }

    pub fn patch(url: impl Into<String>) -> Self {
        Self::new("PATCH", url)
    }

    pub fn delete(url: impl Into<String>) -> Self {
        Self::new("DELETE", url)
    }

    /// Adds a header value (a repeated name keeps every value).
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .entry(name.into())
            .or_default()
            .push(value.into());
        self
    }

    /// `Authorization: Bearer <token>`.
    pub fn with_bearer(self, token: impl fmt::Display) -> Self {
        self.with_header("authorization", format!("Bearer {token}"))
    }

    /// The request body.
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// `value` as the JSON body, with `content-type: application/json`.
    #[cfg(feature = "json")]
    pub fn with_json<T: serde::Serialize + ?Sized>(
        self,
        value: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(self
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_vec(value)?))
    }

    /// This call's own timeout, applied to connecting and to the first byte
    /// of the response and between bytes. The host still caps it. Must be
    /// positive.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn headers(&self) -> &MultiMap {
        &self.headers
    }

    /// The first value of the header `name`, matched case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        first(&self.headers, name)
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

/// The body's length, never its bytes (Java's `toString`).
impl fmt::Debug for HttpCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpCall")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field("body.length", &self.body.len())
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// An outbound call's response (Java's `HttpReply`).
#[derive(Clone, PartialEq, Eq)]
pub struct HttpReply {
    status: u16,
    headers: MultiMap,
    body: Vec<u8>,
}

impl HttpReply {
    pub fn new(status: u16, headers: MultiMap, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers,
            body: body.into(),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    /// A 2xx status.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The headers as the server sent them (lower-case names over HTTP/2).
    pub fn headers(&self) -> &MultiMap {
        &self.headers
    }

    /// The first value of the header `name`, matched case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        first(&self.headers, name)
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    /// The body as UTF-8.
    pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }

    /// The body as JSON.
    #[cfg(feature = "json")]
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }
}

/// The body's length, never its bytes (Java's `toString`).
impl fmt::Debug for HttpReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpReply")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body.length", &self.body.len())
            .finish()
    }
}

/// Why an outbound call produced no reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The host's policy refused the call: the target is not on `httpAllow`,
    /// or the scheme is not `https` to a non-loopback host. Nothing left the
    /// host. Java's `HttpCallRefusedException`.
    Denied(HttpDenied),
    /// The call could not be built: a malformed URL, method or header, or a
    /// non-positive timeout.
    InvalidRequest(String),
    /// A connect, first-byte or between-bytes timeout, the guest's own or
    /// the host's cap.
    Timeout,
    /// Anything else on the way (DNS, connection, TLS, protocol), as the
    /// `wasi:http` error code.
    Failed(String),
}

impl HttpError {
    /// Whether the host's policy refused the call.
    pub fn is_denied(&self) -> bool {
        matches!(self, HttpError::Denied(_))
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::Denied(denied) => denied.fmt(f),
            HttpError::InvalidRequest(why) => write!(f, "invalid outbound request: {why}"),
            HttpError::Timeout => f.write_str("outbound call timed out"),
            HttpError::Failed(code) => write!(f, "outbound call failed: {code}"),
        }
    }
}

impl std::error::Error for HttpError {}

/// The host refused an outbound call (the `wasi:http` error
/// `HTTP-request-denied`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpDenied {
    host: String,
}

impl HttpDenied {
    pub fn new(host: impl Into<String>) -> Self {
        Self { host: host.into() }
    }

    /// The target host the call was refused for.
    pub fn host(&self) -> &str {
        &self.host
    }
}

/// Java's `HttpCallRefusedException` message (the host does not say which
/// rule refused the call).
impl fmt::Display for HttpDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "outbound call to '{}' refused: not permitted by this function's httpAllow list (https only, except loopback)",
            self.host
        )
    }
}

impl std::error::Error for HttpDenied {}

/// A URL taken apart for `wasi:http`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target<'a> {
    pub scheme: &'a str,
    pub authority: &'a str,
    /// The host of the authority: no user info, no port, IPv6 brackets kept.
    pub host: &'a str,
    pub path_with_query: &'a str,
}

/// `scheme://authority[/path][?query][#fragment]`; the fragment is dropped
/// (it never goes on the wire), an empty path is `/`.
pub(crate) fn target(url: &str) -> Result<Target<'_>, HttpError> {
    let invalid = |why: &str| HttpError::InvalidRequest(format!("{why}: {url}"));
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| invalid("not an absolute URL"))?;
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return Err(invalid("not a URL scheme"));
    }
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, path_with_query) = rest.split_at(split);
    if authority.is_empty() {
        return Err(invalid("no host"));
    }
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
    let host = if host_port.starts_with('[') {
        host_port
            .find(']')
            .map(|end| &host_port[..=end])
            .ok_or_else(|| invalid("unclosed IPv6 host"))?
    } else {
        host_port.split_once(':').map_or(host_port, |(h, _)| h)
    };
    if host.is_empty() {
        return Err(invalid("no host"));
    }
    let path_with_query = if path_with_query.is_empty() {
        "/"
    } else {
        path_with_query
    };
    Ok(Target {
        scheme,
        authority,
        host,
        path_with_query,
    })
}

/// The first value of `name` over every key that matches it
/// case-insensitively.
pub(crate) fn first<'a>(headers: &'a MultiMap, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .flat_map(|(_, v)| v)
        .map(String::as_str)
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_splits_into_what_wasi_http_wants() {
        assert_eq!(
            target("https://api.example.com/v1/x?y=1#frag").unwrap(),
            Target {
                scheme: "https",
                authority: "api.example.com",
                host: "api.example.com",
                path_with_query: "/v1/x?y=1",
            }
        );
        let t = target("http://user:pw@127.0.0.1:8080").unwrap();
        assert_eq!(
            (t.authority, t.host, t.path_with_query),
            ("user:pw@127.0.0.1:8080", "127.0.0.1", "/")
        );
        let t = target("https://[::1]:443?q").unwrap();
        assert_eq!((t.host, t.path_with_query), ("[::1]", "?q"));
    }

    #[test]
    fn a_url_that_is_not_absolute_is_an_invalid_request() {
        for url in [
            "/relative",
            "example.com/x",
            "https://",
            "https:///x",
            "ht tp://x",
            "https://[::1",
        ] {
            assert!(
                matches!(target(url), Err(HttpError::InvalidRequest(_))),
                "{url}"
            );
        }
    }

    #[test]
    fn a_call_builds_up_and_debug_hides_the_body() {
        let call = HttpCall::post("https://x.test/a")
            .with_header("X-A", "1")
            .with_header("X-A", "2")
            .with_bearer("t0k")
            .with_body("secret body")
            .with_timeout(Duration::from_secs(2));
        assert_eq!(call.method(), "POST");
        assert_eq!(call.headers()["X-A"], ["1", "2"]);
        assert_eq!(call.header("x-a"), Some("1"));
        assert_eq!(call.header("Authorization"), Some("Bearer t0k"));
        assert_eq!(call.timeout(), Some(Duration::from_secs(2)));
        let debug = format!("{call:?}");
        assert!(
            debug.contains("body.length: 11") && !debug.contains("secret"),
            "{debug}"
        );
    }

    #[test]
    fn a_reply_reads_as_text_and_its_headers_case_insensitively() {
        let mut headers = MultiMap::new();
        headers.insert("Content-Type".into(), vec!["text/plain".into()]);
        let reply = HttpReply::new(201, headers, "ok");
        assert!(reply.is_success());
        assert_eq!(reply.header("content-type"), Some("text/plain"));
        assert_eq!(reply.text().unwrap(), "ok");
        assert!(!HttpReply::new(302, MultiMap::new(), "").is_success());
    }

    #[test]
    fn a_denial_reads_as_javas_refusal() {
        let e = HttpError::Denied(HttpDenied::new("evil.test"));
        assert!(e.is_denied());
        assert!(e.to_string().starts_with(
            "outbound call to 'evil.test' refused: not permitted by this function's httpAllow list"
        ));
    }
}

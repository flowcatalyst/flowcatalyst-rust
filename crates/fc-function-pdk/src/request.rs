use std::fmt;
use std::rc::Rc;

#[cfg(feature = "flowcatalyst")]
use fc_function_abi::Caller;
use fc_function_abi::MultiMap;

use crate::backend::Backend;
use crate::http::first;

/// The HTTP request an invocation arrived as: the HTTP half of Java's
/// `Request` (the invocation half is [`Context::invocation`](crate::Context::invocation)).
///
/// The path is the function's own: the host has stripped its
/// `/functions/{address}[:{version}]` or public-route prefix. The body is
/// buffered in full (capped by the endpoint's `maxBodyBytes`).
///
/// For a `webhook` endpoint, parse the body with
/// [`Webhook::event`](crate::Webhook::event)`(&req)` or
/// [`Webhook::schedule`](crate::Webhook::schedule)`(&req)`.
pub struct Request {
    method: String,
    path: String,
    raw_query: Option<String>,
    query: MultiMap,
    headers: MultiMap,
    body: Vec<u8>,
    authority: Option<String>,
    #[cfg_attr(not(feature = "flowcatalyst"), allow(dead_code))]
    backend: Rc<dyn Backend>,
}

impl Request {
    /// `path_with_query` is the path plus the raw query, as on the wire.
    pub(crate) fn new(
        method: String,
        path_with_query: &str,
        headers: MultiMap,
        body: Vec<u8>,
        authority: Option<String>,
        backend: Rc<dyn Backend>,
    ) -> Self {
        let (path, raw_query) = match path_with_query.split_once('?') {
            Some((path, query)) => (path, Some(query.to_owned())),
            None => (path_with_query, None),
        };
        Self {
            method,
            path: if path.is_empty() {
                "/".into()
            } else {
                path.into()
            },
            query: raw_query
                .as_deref()
                .map(crate::query::parse)
                .unwrap_or_default(),
            raw_query,
            headers,
            body,
            authority,
            backend,
        }
    }

    /// The method, upper-case for the standard ones (`GET`, `POST`, …).
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The function's path, without the query.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The query exactly as sent, without the `?`.
    pub fn raw_query(&self) -> Option<&str> {
        self.raw_query.as_deref()
    }

    /// Every query parameter, decoded (`+` is a space) and in order;
    /// repeated keys keep every value.
    pub fn query(&self) -> &MultiMap {
        &self.query
    }

    /// The first value of the query parameter `name`.
    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query
            .get(name)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    /// Every value of the query parameter `name`.
    pub fn query_params(&self, name: &str) -> &[String] {
        self.query.get(name).map_or(&[], Vec::as_slice)
    }

    /// Every header, names as delivered (lower-case from the FlowCatalyst
    /// host), values read as UTF-8 (lossy).
    pub fn headers(&self) -> &MultiMap {
        &self.headers
    }

    /// The first value of the header `name`, matched case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        first(&self.headers, name)
    }

    /// Every value of the header `name`, matched case-insensitively, in
    /// header order.
    pub fn header_all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .flat_map(|(_, v)| v)
            .map(String::as_str)
            .collect()
    }

    /// The `Host` the request was addressed to.
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// The body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The body, by value.
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

    /// The value the matched endpoint's pattern bound to `{name}`,
    /// percent-decoded (`/orders/{id}` → `path_param("id")`).
    #[cfg(feature = "flowcatalyst")]
    pub fn path_param(&self, name: &str) -> Option<&str> {
        self.backend.invocation().path_param(name)
    }

    /// Every bound path parameter, in pattern order.
    #[cfg(feature = "flowcatalyst")]
    pub fn path_params(&self) -> &[(String, String)] {
        &self.backend.invocation().path_params
    }

    /// Who made the call (a shortcut for `ctx.invocation().caller`).
    #[cfg(feature = "flowcatalyst")]
    pub fn caller(&self) -> &Caller {
        &self.backend.invocation().caller
    }

    /// Adds a header value. For building requests in tests.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .entry(name.into())
            .or_default()
            .push(value.into());
        self
    }

    /// Replaces the body. For building requests in tests.
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// `value` as the JSON body, with `content-type: application/json`. For
    /// building requests in tests.
    #[cfg(feature = "json")]
    pub fn with_json<T: serde::Serialize + ?Sized>(
        self,
        value: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(self
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_vec(value)?))
    }
}

/// The body: so `Webhook::event(&req)` parses it.
impl AsRef<[u8]> for Request {
    fn as_ref(&self) -> &[u8] {
        &self.body
    }
}

/// The body's length, never its bytes (Java's `toString`).
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("raw_query", &self.raw_query)
            .field("headers", &self.headers)
            .field("body.length", &self.body.len())
            .field("authority", &self.authority)
            .finish_non_exhaustive()
    }
}

/// Header entries as a multi-map: one key per distinct name as spelled,
/// values in order, read as UTF-8 (lossy).
pub(crate) fn header_map(entries: Vec<(String, Vec<u8>)>) -> MultiMap {
    let mut headers = MultiMap::new();
    for (name, value) in entries {
        headers
            .entry(name)
            .or_default()
            .push(String::from_utf8_lossy(&value).into_owned());
    }
    headers
}

#[cfg(test)]
mod tests {
    use crate::testing::TestHost;
    use crate::Webhook;

    #[test]
    fn the_path_and_query_split_and_the_query_decodes() {
        let host = TestHost::new();
        let req = host.request("GET", "/orders/7?y=hello+world&y=again&z=%2Fa");
        assert_eq!(req.method(), "GET");
        assert_eq!(req.path(), "/orders/7");
        assert_eq!(req.raw_query(), Some("y=hello+world&y=again&z=%2Fa"));
        assert_eq!(req.query_param("y"), Some("hello world"));
        assert_eq!(req.query_params("y"), ["hello world", "again"]);
        assert_eq!(req.query_param("z"), Some("/a"));
        assert!(req.query_params("nope").is_empty());
        let bare = host.request("GET", "");
        assert_eq!((bare.path(), bare.raw_query()), ("/", None));
    }

    #[test]
    fn headers_are_found_case_insensitively_across_spellings() {
        let req = TestHost::new()
            .request("GET", "/")
            .with_header("X-Thing", "a")
            .with_header("x-thing", "b")
            .with_header("X-Thing", "c");
        assert_eq!(req.header("X-THING"), Some("a"));
        assert_eq!(req.header_all("x-thing"), ["a", "c", "b"]);
        assert_eq!(req.header("missing"), None);
    }

    #[test]
    fn the_body_reads_as_bytes_text_json_and_a_webhook() {
        let body = br#"{"id":"evt-1","type":"a:b:c:d","attemptNumber":2,"data":{"n":1}}"#;
        let req = TestHost::new()
            .request("POST", "/events")
            .with_body(&body[..]);
        assert_eq!(req.body(), body);
        assert!(req.text().unwrap().starts_with("{\"id\""));
        #[cfg(feature = "json")]
        assert_eq!(req.json::<serde_json::Value>().unwrap()["attemptNumber"], 2);
        let event = Webhook::event(&req).unwrap();
        assert_eq!(
            (
                event.id.as_str(),
                event.attempt_number,
                event.data_json.as_deref()
            ),
            ("evt-1", 2, Some(r#"{"n":1}"#))
        );
        assert!(Webhook::schedule(&req).is_err());
        assert!(TestHost::new()
            .request("POST", "/")
            .with_body(vec![0xff])
            .text()
            .is_err());
    }

    #[test]
    fn debug_shows_the_body_length_not_the_body() {
        let req = TestHost::new().request("POST", "/").with_body("s3cret");
        let debug = format!("{req:?}");
        assert!(
            debug.contains("body.length: 6") && !debug.contains("s3cret"),
            "{debug}"
        );
    }

    #[cfg(feature = "flowcatalyst")]
    #[test]
    fn path_params_and_the_caller_come_from_the_invocation() {
        let host = TestHost::new()
            .path_param("id", "42")
            .caller(crate::Caller::Platform);
        let req = host.request("GET", "/orders/42");
        assert_eq!(req.path_param("id"), Some("42"));
        assert_eq!(req.path_param("nope"), None);
        assert_eq!(req.path_params(), [("id".to_string(), "42".to_string())]);
        assert_eq!(req.caller(), &crate::Caller::Platform);
    }
}

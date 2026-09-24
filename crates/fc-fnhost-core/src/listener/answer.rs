//! What the listener writes back (Java `FnHttpServer.HttpAnswer`, `write`
//! and `ErrorBody`).

use bytes::Bytes;
use fc_function_abi::{MultiMap, Response};
use http::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use http_body_util::Full;

/// The function's own hop-by-hop and length headers, dropped from its
/// response (the host sets the length).
const HOP_BY_HOP: [&str; 9] = [
    "connection",
    "transfer-encoding",
    "keep-alive",
    "upgrade",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "content-length",
];

/// A status, headers (in order, multi-valued) and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpAnswer {
    pub status: u16,
    pub headers: MultiMap,
    pub body: Bytes,
}

impl HttpAnswer {
    pub fn new(status: u16, headers: MultiMap, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            headers,
            body: body.into(),
        }
    }

    /// A host-generated error: `{"error":CODE,"message":…}`.
    pub fn error(status: u16, code: &str, message: &str) -> Self {
        Self::new(status, MultiMap::new(), error_body(code, message))
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_owned(), vec![value.to_owned()]);
        self
    }

    /// A function's response, minus hop-by-hop headers and its
    /// `Content-Length` (Java `toAnswer`).
    pub fn from_response(response: Response) -> Self {
        let (status, headers, body) = response.into_parts();
        let headers = headers
            .into_iter()
            .filter(|(name, _)| !HOP_BY_HOP.contains(&name.to_ascii_lowercase().as_str()))
            .collect();
        Self::new(status, headers, body)
    }

    /// The HTTP response (Java `write`). Each header name's first value
    /// replaces whatever an earlier spelling of the same name set, the rest
    /// append (Vert.x `putHeader` then `add`); `Content-Type:
    /// application/json` is added when the body is non-empty and none was
    /// set. A name or value that is not legal on the wire is dropped and
    /// logged.
    pub fn into_response(self) -> http::Response<Full<Bytes>> {
        let mut response = http::Response::new(Full::new(self.body.clone()));
        *response.status_mut() = http::StatusCode::from_u16(self.status)
            .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
        let headers = response.headers_mut();
        for (name, values) in &self.headers {
            let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) else {
                tracing::warn!(header = %name, "dropping a response header with an illegal name");
                continue;
            };
            let mut first = true;
            for value in values {
                let Some(header_value) = header_value(value) else {
                    tracing::warn!(header = %name, "dropping a response header value that is not legal on the wire");
                    continue;
                };
                if first {
                    headers.insert(header_name.clone(), header_value);
                    first = false;
                } else {
                    headers.append(header_name.clone(), header_value);
                }
            }
        }
        if !headers.contains_key(CONTENT_TYPE) && !self.body.is_empty() {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        response
    }
}

/// ISO-8859-1 when every character fits (Netty's header encoding), UTF-8
/// otherwise.
fn header_value(value: &str) -> Option<HeaderValue> {
    if value.chars().all(|c| (c as u32) <= 0xFF) {
        let bytes: Vec<u8> = value.chars().map(|c| c as u32 as u8).collect();
        HeaderValue::from_bytes(&bytes).ok()
    } else {
        HeaderValue::from_str(value).ok()
    }
}

/// Java `ErrorBody.json`: hand-escaped, non-ASCII passed through.
pub(crate) fn error_body(code: &str, message: &str) -> Vec<u8> {
    let mut out = String::with_capacity(code.len() + message.len() + 24);
    out.push_str("{\"error\":\"");
    escape(code, &mut out);
    out.push_str("\",\"message\":\"");
    escape(message, &mut out);
    out.push_str("\"}");
    out.into_bytes()
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

/// Spec `function-host-listener.md` §2's outcome for an entered invocation
/// (Java `FnHttpServer.outcomeFor`).
pub(crate) fn outcome_for(status: u16) -> &'static str {
    match status {
        s if s < 400 => "ok",
        429 => "retry",
        400..=499 => "client_error",
        _ => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_bodies_escape_as_java() {
        assert_eq!(
            String::from_utf8(error_body("UNAUTHORIZED", "a \"b\"\\\n\u{1}é")).unwrap(),
            "{\"error\":\"UNAUTHORIZED\",\"message\":\"a \\\"b\\\"\\\\\\n\\u0001é\"}"
        );
    }

    // Java OutcomeForTest
    #[test]
    fn status_maps_to_outcome() {
        for (status, outcome) in [
            (200, "ok"),
            (299, "ok"),
            (302, "ok"),
            (399, "ok"),
            (400, "client_error"),
            (428, "client_error"),
            (429, "retry"),
            (430, "client_error"),
            (499, "client_error"),
            (500, "error"),
            (503, "error"),
            (599, "error"),
        ] {
            assert_eq!(outcome_for(status), outcome, "{status}");
        }
    }

    #[test]
    fn a_later_spelling_of_a_name_replaces_an_earlier_one() {
        let mut headers = MultiMap::new();
        headers.insert("X-A".into(), vec!["1".into(), "2".into()]);
        headers.insert("x-a".into(), vec!["3".into()]);
        headers.insert("X-Bad".into(), vec!["line\nbreak".into(), "ok".into()]);
        let response = HttpAnswer::new(200, headers, "{}").into_response();
        let values: Vec<_> = response.headers().get_all("x-a").iter().collect();
        assert_eq!(values, ["3"]);
        assert_eq!(response.headers()["x-bad"], "ok");
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
    }

    #[test]
    fn hop_by_hop_headers_are_dropped_from_a_function_response() {
        let mut headers = MultiMap::new();
        headers.insert("Connection".into(), vec!["keep-alive".into()]);
        headers.insert("Content-Length".into(), vec!["999".into()]);
        headers.insert("X-Multi".into(), vec!["a".into(), "b".into()]);
        let answer =
            HttpAnswer::from_response(Response::http(201, headers, b"x".to_vec()).unwrap());
        assert_eq!(answer.status, 201);
        assert_eq!(answer.headers.keys().collect::<Vec<_>>(), ["X-Multi"]);
    }
}

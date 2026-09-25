use std::time::Duration;

use crate::{java, InvalidArgument, MultiMap};

/// The reason the Java host puts in the `500` it answers whenever a WASM guest
/// fails: a trap, an Extism error, output that is not the reply shape, or no
/// instance to run it on (Java `fnhost/wasm/WasmFunction.java`
/// `FAILURE_REASON`). [`Response::function_failed`] builds that answer.
pub const FAILURE_REASON: &str = "the function failed";

/// The outcome of one invocation: the HTTP response the host sends back.
/// Mirrors Java `function-api/src/main/java/io/flowcatalyst/function/Result.java`
/// (renamed: `Result` means something else in Rust).
///
/// What the platform does with each builder's answer (from `Result.java`'s
/// class doc):
///
/// | Builder | Response | Dispatch-job delivery | Scheduled-job delivery |
/// |---|---|---|---|
/// | [`ack`](Self::ack) | `200`, empty | delivered | delivered |
/// | [`retry`](Self::retry) | `429` + `Retry-After` | deferred by `Retry-After`, no retry budget spent | **not honoured**: any non-2xx is a failed attempt |
/// | [`fail`](Self::fail) | `500` + `{"error":…}` | a failed attempt, like any non-2xx non-429 | a failed attempt |
///
/// There is no way for a function to force an immediate, non-retryable
/// failure, and no way to delay a scheduled job's next attempt.
///
/// The status is always 100-599; every constructor enforces it. `Debug`
/// shows the body's length, never its bytes, as Java's `toString` does.
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    status: u16,
    headers: MultiMap,
    body: Vec<u8>,
}

impl Response {
    /// The invocation succeeded: `200`, no headers, empty body.
    pub fn ack() -> Self {
        Self {
            status: 200,
            headers: MultiMap::new(),
            body: Vec::new(),
        }
    }

    /// Redeliver after `after`: `429` with `Retry-After` in whole seconds,
    /// rounded **up** (rounding down would ask for less delay than
    /// requested) and clamped to `i32::MAX`. Zero is a legal "retry now".
    /// Advisory only for scheduled jobs (see the type docs).
    pub fn retry(after: Duration) -> Self {
        let mut seconds = after.as_secs();
        if after.subsec_nanos() > 0 {
            seconds += 1;
        }
        let seconds = seconds.min(i32::MAX as u64);
        let mut headers = MultiMap::new();
        headers.insert("Retry-After".to_string(), vec![seconds.to_string()]);
        Self {
            status: 429,
            headers,
            body: Vec::new(),
        }
    }

    /// The invocation failed: `500`, `Content-Type: application/json` and
    /// `{"error":"<reason>"}`.
    ///
    /// Errors when `reason` is blank (Java's `String.isBlank`).
    pub fn fail(reason: &str) -> Result<Self, InvalidArgument> {
        if java::is_blank(reason) {
            return Err(InvalidArgument("reason must not be blank".into()));
        }
        #[derive(serde::Serialize)]
        struct Body<'a> {
            error: &'a str,
        }
        Ok(Self {
            status: 500,
            headers: content_type_json(),
            body: serde_json::to_vec(&Body { error: reason }).expect("a string always serialises"),
        })
    }

    /// The host's answer when a guest fails (see [`FAILURE_REASON`]).
    pub fn function_failed() -> Self {
        Self {
            status: 500,
            headers: content_type_json(),
            body: br#"{"error":"the function failed"}"#.to_vec(),
        }
    }

    /// A direct HTTP answer, verbatim. Errors when `status` is outside 100-599.
    pub fn http(status: u16, headers: MultiMap, body: Vec<u8>) -> Result<Self, InvalidArgument> {
        check_status(status)?;
        Ok(Self {
            status,
            headers,
            body,
        })
    }

    /// A direct HTTP answer whose body is `json` verbatim (not validated),
    /// with `Content-Type: application/json`. Errors when `status` is outside
    /// 100-599.
    pub fn json(status: u16, json: impl Into<String>) -> Result<Self, InvalidArgument> {
        check_status(status)?;
        Ok(Self {
            status,
            headers: content_type_json(),
            body: json.into().into_bytes(),
        })
    }

    /// The HTTP status, 100-599.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The response headers, in insertion order.
    pub fn headers(&self) -> &MultiMap {
        &self.headers
    }

    /// The response body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Status, headers and body, by value.
    pub fn into_parts(self) -> (u16, MultiMap, Vec<u8>) {
        (self.status, self.headers, self.body)
    }
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body.length", &self.body.len())
            .finish()
    }
}

fn check_status(status: u16) -> Result<(), InvalidArgument> {
    if (100..=599).contains(&status) {
        Ok(())
    } else {
        Err(InvalidArgument(format!(
            "status must be 100-599, was {status}"
        )))
    }
}

fn content_type_json() -> MultiMap {
    let mut headers = MultiMap::new();
    headers.insert(
        "Content-Type".to_string(),
        vec!["application/json".to_string()],
    );
    headers
}

/// Java `function-api/src/test/java/io/flowcatalyst/function/ResultTest.java`.
#[cfg(test)]
mod tests {
    use super::*;

    fn header<'a>(r: &'a Response, name: &str) -> Option<&'a [String]> {
        r.headers().get(name).map(Vec::as_slice)
    }

    fn body(r: &Response) -> &str {
        std::str::from_utf8(r.body()).unwrap()
    }

    #[test]
    fn ack_is_status_200_with_an_empty_body() {
        let r = Response::ack();
        assert_eq!(r.status(), 200);
        assert!(r.body().is_empty());
        assert!(r.headers().is_empty());
    }

    // retryRejectsNullDuration / retryRejectsNegativeDuration: `Duration` is
    // never null or negative in Rust.

    #[test]
    fn retry_is_status_429_with_a_retry_after_header() {
        let r = Response::retry(Duration::from_secs(45));
        assert_eq!(r.status(), 429);
        assert_eq!(header(&r, "Retry-After"), Some(&["45".to_string()][..]));
    }

    #[test]
    fn retry_accepts_zero_duration_as_an_immediate_retry_request() {
        let r = Response::retry(Duration::ZERO);
        assert_eq!(header(&r, "Retry-After"), Some(&["0".to_string()][..]));
    }

    #[test]
    fn retry_rounds_a_sub_second_remainder_up_never_down() {
        let secs = |d| header(&Response::retry(d), "Retry-After").unwrap()[0].clone();
        assert_eq!(secs(Duration::from_millis(1500)), "2");
        assert_eq!(secs(Duration::from_secs(2)), "2");
        assert_eq!(secs(Duration::from_millis(1)), "1");
    }

    #[test]
    fn retry_clamps_to_int_max() {
        let r = Response::retry(Duration::from_secs(u64::MAX));
        assert_eq!(header(&r, "Retry-After").unwrap()[0], "2147483647");
    }

    #[test]
    fn fail_rejects_blank_reason() {
        assert!(Response::fail("").is_err());
        assert!(Response::fail("   ").is_err());
        assert!(Response::fail("\u{2003}\t").is_err());
        assert_eq!(
            Response::fail("").unwrap_err().message(),
            "reason must not be blank"
        );
    }

    #[test]
    fn fail_is_status_500_with_a_json_error_body() {
        let r = Response::fail("bad input").unwrap();
        assert_eq!(r.status(), 500);
        assert_eq!(
            header(&r, "Content-Type"),
            Some(&["application/json".to_string()][..])
        );
        assert_eq!(body(&r), r#"{"error":"bad input"}"#);
    }

    #[test]
    fn fail_escapes_quotes_and_backslashes_in_the_reason() {
        let r = Response::fail("said \"hi\" then \\ broke").unwrap();
        assert_eq!(body(&r), r#"{"error":"said \"hi\" then \\ broke"}"#);
    }

    #[test]
    fn fail_escapes_control_characters() {
        let r = Response::fail("line1\nline2\ttabbed").unwrap();
        assert_eq!(body(&r), r#"{"error":"line1\nline2\ttabbed"}"#);
    }

    #[test]
    fn fail_carries_non_ascii_reasons() {
        let r = Response::fail("caf\u{e9} \u{2603}").unwrap();
        let v: serde_json::Value = serde_json::from_slice(r.body()).unwrap();
        assert_eq!(v, serde_json::json!({"error": "caf\u{e9} \u{2603}"}));
    }

    #[test]
    fn function_failed_is_fail_of_the_failure_reason() {
        assert_eq!(
            Response::function_failed(),
            Response::fail(FAILURE_REASON).unwrap()
        );
    }

    #[test]
    fn http_response_rejects_status_outside_range() {
        assert!(Response::http(99, MultiMap::new(), vec![]).is_err());
        assert!(Response::http(600, MultiMap::new(), vec![]).is_err());
        assert_eq!(
            Response::json(600, "{}").unwrap_err().message(),
            "status must be 100-599, was 600"
        );
    }

    #[test]
    fn http_response_accepts_boundary_statuses() {
        assert_eq!(
            Response::http(100, MultiMap::new(), vec![])
                .unwrap()
                .status(),
            100
        );
        assert_eq!(
            Response::http(599, MultiMap::new(), vec![])
                .unwrap()
                .status(),
            599
        );
    }

    // httpResponseBodyIsIndependentOf..., httpResponseHeadersAreUnmodifiable,
    // httpResponseHeadersAreIndependentOfTheMapPassedIn: the response owns its
    // headers and body and exposes them by shared reference only.

    #[test]
    fn json_sets_content_type_and_body() {
        let r = Response::json(201, r#"{"a":1}"#).unwrap();
        assert_eq!(r.status(), 201);
        assert_eq!(
            header(&r, "Content-Type"),
            Some(&["application/json".to_string()][..])
        );
        assert_eq!(body(&r), r#"{"a":1}"#);
    }

    // jsonRejectsNullBody: a `String` is never null.

    #[test]
    fn equality_is_by_content() {
        let mut h = MultiMap::new();
        h.insert("X".into(), vec!["1".into()]);
        let a = Response::http(200, h.clone(), vec![1, 2]).unwrap();
        let b = Response::http(200, h.clone(), vec![1, 2]).unwrap();
        let c = Response::http(200, h, vec![1, 3]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn debug_does_not_dump_body_bytes_only_length() {
        let r = Response::http(200, MultiMap::new(), vec![1, 2, 3]).unwrap();
        let text = format!("{r:?}");
        assert!(text.contains("body.length: 3"), "{text}");
        assert!(!text.contains("[1, 2, 3]"), "{text}");
    }
}

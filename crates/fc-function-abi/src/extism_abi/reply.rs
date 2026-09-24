use super::base64;
use super::jackson::{self, Node};
use crate::java::utf16_to_string;
use crate::{MultiMap, Response};

/// The guest's output is not the reply shape. Mirrors Java `WasmAbi.Malformed`;
/// [`detail`](Self::detail) is Java's wording, for the one WARN the host logs.
///
/// Java's host answers every malformed reply with [`Response::function_failed`]
/// (`500`, `{"error":"the function failed"}`, `WasmFunction.java`), which
/// [`MalformedReply::response`] returns, and discards the instance.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("malformed guest reply: {detail}")]
pub struct MalformedReply {
    detail: String,
}

impl MalformedReply {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    /// How the output was malformed, in Java's words.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// What the host answers the caller: [`Response::function_failed`].
    pub fn response(&self) -> Response {
        Response::function_failed()
    }
}

/// Decodes a guest's reply (Java `fnhost/wasm/WasmAbi.java` `decode`):
/// `{"status": 100-599, "headers": {"k": ["v", …]}, "body" | "bodyBase64"}`.
///
/// - `status` must be a JSON integer that fits `i32` and is 100-599
///   (`200.0` and `"200"` are refused);
/// - `headers` is optional (absent or `null` = none); when present, an object
///   whose every value is an array of strings, order kept;
/// - `body` (text, sent as UTF-8) or `bodyBase64` (bytes, Java's lenient
///   decoder), not both; absent or `null` = empty;
/// - unknown keys are ignored; a repeated key takes its last value.
///
/// A string holding an unpaired escaped surrogate has it replaced by `?`,
/// which is what Java's `getBytes(UTF_8)` sends.
pub fn decode_reply(output: &[u8]) -> Result<Response, MalformedReply> {
    let root = match jackson::read_tree(output) {
        Err(_) => return Err(MalformedReply::new("output is not JSON")),
        Ok(Some(root @ Node::Obj(_))) => root,
        Ok(_) => return Err(MalformedReply::new("output is not a JSON object")),
    };
    let status = root
        .member("status")
        .and_then(Node::as_i32)
        .and_then(|s| u16::try_from(s).ok())
        .filter(|s| (100..=599).contains(s))
        .ok_or_else(|| MalformedReply::new("status must be an integer 100-599"))?;

    let mut headers = MultiMap::new();
    let headers_node = root.member("headers");
    if !Node::is_absent(headers_node) {
        let Some(Node::Obj(members)) = headers_node else {
            return Err(MalformedReply::new(
                "headers must be an object of string arrays",
            ));
        };
        for (key, value) in members {
            let name = utf16_to_string(key);
            let values = match value {
                Node::Arr(items) => items
                    .iter()
                    .map(|item| match item {
                        Node::Str(s) => Some(utf16_to_string(s)),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            };
            let Some(values) = values else {
                return Err(MalformedReply::new(format!(
                    "header '{name}' must be an array of strings"
                )));
            };
            headers.insert(name, values);
        }
    }

    let text = root.member("body");
    let base64 = root.member("bodyBase64");
    let body = match (Node::is_absent(text), Node::is_absent(base64)) {
        (false, false) => {
            return Err(MalformedReply::new(
                "give either body or bodyBase64, not both",
            ))
        }
        (false, true) => match text {
            Some(Node::Str(s)) => utf16_to_string(s).into_bytes(),
            _ => return Err(MalformedReply::new("body must be a string")),
        },
        (true, false) => match base64 {
            Some(Node::Str(s)) => base64::decode(&utf16_to_string(s))
                .ok_or_else(|| MalformedReply::new("bodyBase64 is not base64"))?,
            _ => return Err(MalformedReply::new("bodyBase64 must be a string")),
        },
        (true, true) => Vec::new(),
    };

    // The status was checked above, so this cannot fail.
    Response::http(status, headers, body)
        .map_err(|_| MalformedReply::new("status must be an integer 100-599"))
}

/// Encodes a reply the way a guest should send it, for the Rust PDK:
/// `{"status":…,"headers":{…},"bodyBase64":"…"}` (the body always as base64,
/// so any bytes survive).
pub fn encode_reply(response: &Response) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"status\":");
    out.extend_from_slice(response.status().to_string().as_bytes());
    out.extend_from_slice(b",\"headers\":{");
    for (i, (name, values)) in response.headers().iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        jackson::write_str(name, &mut out);
        out.extend_from_slice(b":[");
        for (j, value) in values.iter().enumerate() {
            if j > 0 {
                out.push(b',');
            }
            jackson::write_str(value, &mut out);
        }
        out.push(b']');
    }
    out.extend_from_slice(b"},\"bodyBase64\":");
    jackson::write_str(&base64::encode(response.body()), &mut out);
    out.push(b'}');
    out
}

/// Java `function-host/src/test/java/io/flowcatalyst/fnhost/wasm/WasmAbiTest.java`:
/// the 6 accepted and 12 malformed reply rows. Every other row Java answers
/// is in `tests/data/java-golden/decode.tsv`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_replies() {
        let rows: [(&str, &str, u16, &str); 6] = [
            (
                "text body",
                r#"{"status":201,"body":"héllo"}"#,
                201,
                "héllo",
            ),
            (
                "base64 body",
                r#"{"status":200,"bodyBase64":"aGk="}"#,
                200,
                "hi",
            ),
            ("no body at all", r#"{"status":204}"#, 204, ""),
            (
                "headers of string arrays",
                r#"{"status":200,"headers":{"a":["1","2"]},"body":"x"}"#,
                200,
                "x",
            ),
            (
                "unknown keys ignored",
                r#"{"status":200,"body":"x","extra":true}"#,
                200,
                "x",
            ),
            (
                "null body is no body",
                r#"{"status":200,"body":null}"#,
                200,
                "",
            ),
        ];
        for (rule, output, status, body) in rows {
            let reply = decode_reply(output.as_bytes()).unwrap_or_else(|e| panic!("{rule}: {e}"));
            assert_eq!(reply.status(), status, "{rule}");
            assert_eq!(reply.body(), body.as_bytes(), "{rule}");
        }
    }

    #[test]
    fn header_order_and_values_are_kept() {
        let reply =
            decode_reply(br#"{"status":200,"headers":{"a":["1","2"]},"body":"x"}"#).unwrap();
        assert_eq!(reply.headers()["a"], vec!["1".to_string(), "2".to_string()]);
    }

    #[test]
    fn malformed_replies() {
        let rows: [(&str, &str, &str); 12] = [
            (
                "not JSON",
                "this is not the result shape",
                "output is not JSON",
            ),
            ("not an object", "[200]", "output is not a JSON object"),
            (
                "no status",
                r#"{"body":"x"}"#,
                "status must be an integer 100-599",
            ),
            (
                "status not an integer",
                r#"{"status":"200"}"#,
                "status must be an integer 100-599",
            ),
            (
                "status below range",
                r#"{"status":99}"#,
                "status must be an integer 100-599",
            ),
            (
                "status above range",
                r#"{"status":600}"#,
                "status must be an integer 100-599",
            ),
            (
                "both bodies",
                r#"{"status":200,"body":"x","bodyBase64":"eA=="}"#,
                "give either body or bodyBase64, not both",
            ),
            (
                "body not a string",
                r#"{"status":200,"body":{"a":1}}"#,
                "body must be a string",
            ),
            (
                "bodyBase64 not base64",
                r#"{"status":200,"bodyBase64":"!!!"}"#,
                "bodyBase64 is not base64",
            ),
            (
                "headers not an object",
                r#"{"status":200,"headers":["a"]}"#,
                "headers must be an object of string arrays",
            ),
            (
                "header value not an array",
                r#"{"status":200,"headers":{"a":"1"}}"#,
                "header 'a' must be an array of strings",
            ),
            (
                "header array of non-strings",
                r#"{"status":200,"headers":{"a":[1]}}"#,
                "header 'a' must be an array of strings",
            ),
        ];
        for (rule, output, detail) in rows {
            let err = decode_reply(output.as_bytes()).expect_err(rule);
            assert_eq!(err.detail(), detail, "{rule}");
            assert_eq!(err.response(), Response::function_failed(), "{rule}");
        }
    }

    #[test]
    fn encode_reply_round_trips() {
        let mut headers = MultiMap::new();
        headers.insert("Content-Type".into(), vec!["text/plain".into()]);
        headers.insert("X-Multi".into(), vec!["1".into(), "2\u{1}".into()]);
        let response = Response::http(201, headers, vec![0, 0xFF, b'h']).unwrap();
        let bytes = encode_reply(&response);
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"status":201,"headers":{"Content-Type":["text/plain"],"X-Multi":["1","2\u0001"]},"bodyBase64":"AP9o"}"#
        );
        assert_eq!(decode_reply(&bytes).unwrap(), response);
    }
}

use std::collections::BTreeSet;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::jackson;
use crate::{Caller, FunctionAddress, MultiMap, Principal};

/// One invocation, as the host hands it to a WASM guest: the Extism input
/// JSON. Mirrors Java `function-api/.../Request.java` (the fields) and
/// `fnhost/wasm/WasmAbi.java` `encode` (the bytes).
///
/// [`Request::encode`] writes exactly the bytes Java's encoder writes (pinned
/// by `tests/data/java-golden/encode/`):
///
/// ```json
/// {"address":"a.s.n","version":3,"invocationId":"…","method":"GET","path":"/p",
///  "originalHost":null,"originalPath":null,"pathParams":{},"query":{"k":["v"]},
///  "headers":{"k":["v"]},"bodyBase64":"","remoteAddress":null,"caller":{"kind":"platform"}}
/// ```
///
/// Keys in that order; absent optional strings as `null`; maps in insertion
/// order; `caller` as `{"kind":"platform"}`, `{"kind":"anonymous"}` or
/// `{"kind":"principal","id","type","tier","clients","roles","applications",
/// "allApplications","permissions"}` with `permissions` sorted (by UTF-16
/// code unit, as Java's `String.compareTo`); strings escaped as Jackson does.
///
/// [`Request::decode`] is the guest side. It ignores unknown keys (so a host
/// may add fields) but refuses an unknown `caller.kind` (X-06).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The function this call targets.
    #[serde(with = "address_wire")]
    pub address: FunctionAddress,
    /// The loaded version handling this call.
    pub version: i32,
    /// The host-assigned id for this one call, unique per attempt.
    pub invocation_id: String,
    /// The HTTP method.
    pub method: String,
    /// The request path with the `/functions/{address}[:{version}]` prefix, or
    /// the public route's prefix, stripped.
    pub path: String,
    /// The `Host` the call arrived on, when applicable.
    pub original_host: Option<String>,
    /// The path before any prefix was stripped, when applicable.
    pub original_path: Option<String>,
    /// Path parameters bound by the matched endpoint pattern.
    pub path_params: IndexMap<String, String>,
    /// Query parameters, possibly multi-valued.
    pub query: MultiMap,
    /// Request headers, in their original spelling.
    pub headers: MultiMap,
    /// The request body (`bodyBase64` on the wire).
    #[serde(rename = "bodyBase64", with = "body_wire")]
    pub body: Vec<u8>,
    /// The caller's address.
    pub remote_address: Option<String>,
    /// Who the host believes this call came from.
    #[serde(with = "caller_wire")]
    pub caller: Caller,
}

impl Request {
    /// The guest input bytes, identical to Java's `WasmAbi.encode`.
    pub fn encode(&self) -> Vec<u8> {
        jackson::to_vec(self)
    }

    /// Reads the guest input (the guest side of [`Request::encode`]).
    pub fn decode(input: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(input)
    }

    /// The first value of the header named `name`, matched
    /// case-insensitively (Java `Request.header`).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.header_values(name).first().map(String::as_str)
    }

    /// The values of the first header whose name matches `name`
    /// case-insensitively, in the map's order (Java `Request.headers(String)`:
    /// the first matching key wins; the map keeps its original spelling).
    pub fn header_values(&self, name: &str) -> &[String] {
        self.headers
            .iter()
            .find(|(key, _)| equals_ignore_case(key, name))
            .map_or(&[], |(_, values)| values.as_slice())
    }
}

impl std::fmt::Debug for Request {
    /// The body's length, never its bytes, as Java's `toString`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("address", &self.address.to_string())
            .field("version", &self.version)
            .field("invocation_id", &self.invocation_id)
            .field("method", &self.method)
            .field("path", &self.path)
            .field("original_host", &self.original_host)
            .field("original_path", &self.original_path)
            .field("path_params", &self.path_params)
            .field("query", &self.query)
            .field("headers", &self.headers)
            .field("body.length", &self.body.len())
            .field("remote_address", &self.remote_address)
            .field("caller", &self.caller)
            .finish()
    }
}

/// Java `String.equalsIgnoreCase`: equal lengths in UTF-16, and each pair of
/// characters equal after simple upper-casing, or after upper- then
/// lower-casing.
fn equals_ignore_case(a: &str, b: &str) -> bool {
    if a.encode_utf16().count() != b.encode_utf16().count() {
        return false;
    }
    let simple = |c: char, upper: bool| {
        let mut mapped = if upper {
            c.to_uppercase().collect::<Vec<_>>()
        } else {
            c.to_lowercase().collect::<Vec<_>>()
        };
        match (mapped.len(), mapped.pop()) {
            (1, Some(m)) => m,
            _ => c,
        }
    };
    a.chars().zip(b.chars()).all(|(x, y)| {
        x == y || {
            let (ux, uy) = (simple(x, true), simple(y, true));
            ux == uy || simple(ux, false) == simple(uy, false)
        }
    })
}

mod address_wire {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::FunctionAddress;

    pub fn serialize<S: Serializer>(a: &FunctionAddress, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(a)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<FunctionAddress, D::Error> {
        let raw = String::deserialize(d)?;
        FunctionAddress::parse(&raw).map_err(serde::de::Error::custom)
    }
}

mod body_wire {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::extism_abi::base64;

    pub fn serialize<S: Serializer>(body: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::encode(body))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let raw = String::deserialize(d)?;
        base64::decode(&raw).ok_or_else(|| serde::de::Error::custom("bodyBase64 is not base64"))
    }
}

/// `caller`'s wire shape, kept here so the root [`Caller`] carries no
/// envelope-specific serde attributes.
mod caller_wire {
    use super::*;
    use serde::{Deserializer, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "lowercase")]
    enum Wire {
        Platform,
        Anonymous,
        Principal(PrincipalWire),
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PrincipalWire {
        id: String,
        #[serde(rename = "type")]
        principal_type: String,
        tier: Option<String>,
        clients: Vec<String>,
        roles: Vec<String>,
        applications: Vec<String>,
        all_applications: bool,
        permissions: Vec<String>,
    }

    pub fn serialize<S: Serializer>(caller: &Caller, s: S) -> Result<S::Ok, S::Error> {
        let wire = match caller {
            Caller::Platform => Wire::Platform,
            Caller::Anonymous => Wire::Anonymous,
            Caller::Principal(p) => {
                // Java: `permissions().stream().sorted()`, i.e. String.compareTo,
                // which orders by UTF-16 code unit, not by Rust's UTF-8 bytes.
                let mut permissions: Vec<String> = p.permissions.iter().cloned().collect();
                permissions.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                Wire::Principal(PrincipalWire {
                    id: p.id.clone(),
                    principal_type: p.principal_type.clone(),
                    tier: p.tier.clone(),
                    clients: p.clients.clone(),
                    roles: p.roles.clone(),
                    applications: p.applications.clone(),
                    all_applications: p.all_applications,
                    permissions,
                })
            }
        };
        wire.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Caller, D::Error> {
        Ok(match Wire::deserialize(d)? {
            Wire::Platform => Caller::Platform,
            Wire::Anonymous => Caller::Anonymous,
            Wire::Principal(p) => Caller::Principal(Principal {
                id: p.id,
                principal_type: p.principal_type,
                tier: p.tier,
                clients: p.clients,
                roles: p.roles,
                applications: p.applications,
                all_applications: p.all_applications,
                permissions: p.permissions.into_iter().collect::<BTreeSet<_>>(),
            }),
        })
    }
}

/// Java `function-api/src/test/java/io/flowcatalyst/function/RequestTest.java`
/// and `WasmAbiTest`'s two encoder cases. The byte-level golden comparisons
/// are in `tests/java_golden.rs`.
#[cfg(test)]
mod tests {
    use super::*;

    fn request(headers: &[(&str, &[&str])], body: &[u8]) -> Request {
        Request {
            address: FunctionAddress::parse("billing.invoices.api").unwrap(),
            version: 1,
            invocation_id: "invocation-1".into(),
            method: "GET".into(),
            path: "/x".into(),
            original_host: Some("api.acme.com".into()),
            original_path: Some("/x".into()),
            path_params: IndexMap::new(),
            query: MultiMap::new(),
            headers: headers
                .iter()
                .map(|(k, vs)| (k.to_string(), vs.iter().map(|v| v.to_string()).collect()))
                .collect(),
            body: body.to_vec(),
            remote_address: Some("127.0.0.1".into()),
            caller: Caller::Anonymous,
        }
    }

    // bodyIsIndependentOf..., pathParamsQueryAndHeadersAreUnmodifiable,
    // headersAreIndependentOfTheMapPassedIn, requiredComponentsRejectNull:
    // ownership and non-optional types make these compile-time facts.

    #[test]
    fn header_lookup_is_case_insensitive_but_the_map_keeps_original_spelling() {
        let r = request(&[("X-Request-Id", &["abc-123"])], b"");
        assert_eq!(r.header("x-request-id"), Some("abc-123"));
        assert_eq!(r.header("X-REQUEST-ID"), Some("abc-123"));
        assert_eq!(r.header_values("x-request-id"), ["abc-123".to_string()]);
        assert_eq!(r.header("x-missing"), None);
        assert_eq!(r.headers.keys().collect::<Vec<_>>(), vec!["X-Request-Id"]);
    }

    #[test]
    fn header_lookup_returns_the_first_matching_key() {
        let r = request(&[("x-a", &["1"]), ("X-A", &["2", "3"])], b"");
        assert_eq!(r.header_values("X-a"), ["1".to_string()]);
    }

    #[test]
    fn header_lookup_returns_none_not_empty_string() {
        assert_eq!(request(&[], b"").header("Absent"), None);
        assert!(request(&[], b"").header_values("Absent").is_empty());
    }

    #[test]
    fn equality_is_by_content_including_body() {
        let a = request(&[("X", &["1"])], &[1, 2]);
        let b = request(&[("X", &["1"])], &[1, 2]);
        let c = request(&[("X", &["1"])], &[1, 3]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn debug_reports_body_length_not_bytes() {
        let text = format!("{:?}", request(&[], &[1, 2, 3]));
        assert!(text.contains("body.length: 3"), "{text}");
        assert!(!text.contains("[1, 2, 3]"), "{text}");
    }

    // WasmAbiTest.platformAndAnonymousCallersAreJustTheirKind
    #[test]
    fn platform_and_anonymous_callers_are_just_their_kind() {
        let mut r = request(&[], b"");
        r.caller = Caller::Platform;
        let json = String::from_utf8(r.encode()).unwrap();
        assert!(
            json.ends_with(r#","caller":{"kind":"platform"}}"#),
            "{json}"
        );
        r.caller = Caller::Anonymous;
        let json = String::from_utf8(r.encode()).unwrap();
        assert!(
            json.ends_with(r#","caller":{"kind":"anonymous"}}"#),
            "{json}"
        );
    }

    #[test]
    fn decode_round_trips_and_is_strict_about_the_caller_kind() {
        let mut r = request(&[("A", &["1", "2"]), ("b", &[])], &[0, 0xFF]);
        r.caller = Caller::Principal(Principal {
            id: "prn_1".into(),
            principal_type: "user".into(),
            tier: None,
            clients: vec!["clt_1".into()],
            roles: vec![],
            applications: vec![],
            all_applications: true,
            permissions: ["b:x".to_string(), "a:y".to_string()].into(),
        });
        assert_eq!(Request::decode(&r.encode()).unwrap(), r);

        let json = String::from_utf8(r.encode()).unwrap();
        let unknown_kind = json.replace(r#""kind":"principal""#, r#""kind":"robot""#);
        assert!(Request::decode(unknown_kind.as_bytes()).is_err());
        let extra_field = json.replacen('{', r#"{"future":1,"#, 1);
        assert_eq!(Request::decode(extra_field.as_bytes()).unwrap(), r);
    }

    #[test]
    fn equals_ignore_case_is_javas() {
        assert!(equals_ignore_case("Content-Type", "content-TYPE"));
        assert!(!equals_ignore_case("a", "ab"));
        assert!(equals_ignore_case("\u{e9}", "\u{c9}"));
    }
}

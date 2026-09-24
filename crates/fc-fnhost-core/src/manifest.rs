//! The parts of a stored manifest the listener needs (Java
//! `platform/function/Manifest.java` `readStored`: `readEndpoints`,
//! `readLimits`), read just as tolerantly: unknown keys are ignored, and a
//! malformed endpoint is dropped rather than failing the manifest.

use serde_json::Value;

use crate::java;
use crate::route_pattern::RoutePattern;

/// `FunctionLimits.DEFAULT_MAX_DURATION_MS`.
pub const DEFAULT_MAX_DURATION_MS: i32 = 30_000;
/// `FunctionLimits.DEFAULT_MAX_CONCURRENCY`.
pub const DEFAULT_MAX_CONCURRENCY: i32 = 32;
/// `FunctionLimits.DEFAULT_WASM_MEMORY_MB`.
pub const DEFAULT_WASM_MEMORY_MB: i32 = 64;
/// `Manifest.Endpoint.DEFAULT_MAX_BODY_BYTES`: 1 MiB.
pub const DEFAULT_MAX_BODY_BYTES: i32 = 1_048_576;

/// How the host authenticates a call before it reaches the function (Java
/// `EndpointAuth`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointAuth {
    Webhook,
    Platform,
    None,
}

impl EndpointAuth {
    /// The wire reader: case-insensitive (`tryParseStrict`).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_lowercase().as_str() {
            "webhook" => Some(Self::Webhook),
            "platform" => Some(Self::Platform),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// An HTTP method a route may accept (Java `HttpMethod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl HttpMethod {
    /// The stored reader: the exact upper-case constant name (`parse`),
    /// which is also how a request's method is read.
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "GET" => Self::Get,
            "HEAD" => Self::Head,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "OPTIONS" => Self::Options,
            _ => return None,
        })
    }

    /// The wire reader: case-insensitive (`tryParseStrict`).
    pub fn parse_lenient(raw: &str) -> Option<Self> {
        Self::parse(&raw.to_uppercase())
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }
}

/// A route's CORS policy (Java `Manifest.Cors`); every part optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cors {
    pub origins: Vec<String>,
    pub methods: Vec<String>,
    pub headers: Vec<String>,
    pub allow_credentials: bool,
}

/// One entry of the function's HTTP surface (Java `Manifest.Endpoint`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub path: RoutePattern,
    pub auth: EndpointAuth,
    /// Empty means every method.
    pub methods: Vec<HttpMethod>,
    pub cors: Option<Cors>,
    pub max_body_bytes: i32,
    pub timeout_ms: i32,
}

impl Endpoint {
    /// The methods the endpoint accepts: `webhook` is always exactly `POST`
    /// whatever is stored (spec `function-host-listener.md` §2 step 4); empty
    /// means every method.
    pub fn effective_methods(&self) -> Vec<HttpMethod> {
        if self.auth == EndpointAuth::Webhook {
            vec![HttpMethod::Post]
        } else {
            self.methods.clone()
        }
    }
}

/// A version's resolved limits (Java `Manifest.Limits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_duration_ms: i32,
    pub max_concurrency: i32,
    /// `None` for a JVM function.
    pub wasm_memory_mb: Option<i32>,
}

/// Java `readLimits`: each limit a positive `int`, else its default.
pub fn read_limits(root: &Value, runtime: &str) -> Limits {
    let node = root.get("limits");
    Limits {
        max_duration_ms: read_positive_int(node, "maxDurationMs", DEFAULT_MAX_DURATION_MS),
        max_concurrency: read_positive_int(node, "maxConcurrency", DEFAULT_MAX_CONCURRENCY),
        wasm_memory_mb: (runtime == "wasm")
            .then(|| read_positive_int(node, "wasmMemoryMb", DEFAULT_WASM_MEMORY_MB)),
    }
}

/// Java `readEndpoints`: not an array reads as none; a malformed entry is
/// dropped.
pub fn read_endpoints(root: &Value) -> Vec<Endpoint> {
    match root.get("endpoints") {
        Some(Value::Array(entries)) => entries.iter().filter_map(read_endpoint).collect(),
        _ => Vec::new(),
    }
}

fn read_endpoint(node: &Value) -> Option<Endpoint> {
    if !node.is_object() {
        return None;
    }
    let path = RoutePattern::parse(node.get("path")?.as_str()?)?;
    let auth = EndpointAuth::parse(node.get("auth")?.as_str()?)?;
    let methods = match node.get("methods") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|m| m.as_str().and_then(HttpMethod::parse_lenient))
            .collect(),
        _ => Vec::new(),
    };
    let cors = match node.get("cors") {
        Some(cors @ Value::Object(_)) => Some(Cors {
            origins: read_string_list(cors, "origins"),
            methods: read_string_list(cors, "methods"),
            headers: read_string_list(cors, "headers"),
            allow_credentials: matches!(cors.get("allowCredentials"), Some(Value::Bool(true))),
        }),
        _ => None,
    };
    Some(Endpoint {
        path,
        auth,
        methods,
        cors,
        max_body_bytes: read_positive_int(Some(node), "maxBodyBytes", DEFAULT_MAX_BODY_BYTES),
        timeout_ms: read_positive_int(Some(node), "timeoutMs", DEFAULT_MAX_DURATION_MS),
    })
}

fn read_positive_int(node: Option<&Value>, key: &str, default: i32) -> i32 {
    match java::as_java_int(node.and_then(|n| n.get(key))) {
        Some(value) if value > 0 => value,
        _ => default,
    }
}

/// Non-blank strings only.
fn read_string_list(node: &Value, key: &str) -> Vec<String> {
    match node.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !java::is_blank(s))
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoints_are_read_tolerantly() {
        let manifest = json!({"endpoints": [
            {"path": "/events/*", "auth": "WEBHOOK", "methods": ["GET"]},
            {"path": "/api/{id}", "auth": "platform", "methods": ["get", "bogus", 3],
             "cors": {"origins": ["https://a.test", " "], "allowCredentials": true},
             "maxBodyBytes": 10, "timeoutMs": 200},
            {"path": "no-slash", "auth": "none"},
            {"path": "/x", "auth": "sometimes"},
            {"path": "/y"},
            "not an object",
            {"path": "/z", "auth": "none", "maxBodyBytes": 0, "timeoutMs": 1.5}
        ]});
        let endpoints = read_endpoints(&manifest);
        assert_eq!(endpoints.len(), 3);
        assert_eq!(endpoints[0].auth, EndpointAuth::Webhook);
        assert_eq!(endpoints[0].effective_methods(), [HttpMethod::Post]);
        assert_eq!(endpoints[1].methods, [HttpMethod::Get]);
        let cors = endpoints[1].cors.as_ref().unwrap();
        assert_eq!(cors.origins, ["https://a.test"]);
        assert!(cors.allow_credentials);
        assert_eq!(endpoints[1].max_body_bytes, 10);
        assert_eq!(endpoints[1].timeout_ms, 200);
        assert_eq!(endpoints[2].max_body_bytes, DEFAULT_MAX_BODY_BYTES);
        assert_eq!(endpoints[2].timeout_ms, DEFAULT_MAX_DURATION_MS);
    }

    #[test]
    fn limits_default_per_field() {
        let limits = read_limits(
            &json!({"limits": {"maxConcurrency": 3, "maxDurationMs": -1}}),
            "wasm",
        );
        assert_eq!(limits.max_concurrency, 3);
        assert_eq!(limits.max_duration_ms, DEFAULT_MAX_DURATION_MS);
        assert_eq!(limits.wasm_memory_mb, Some(DEFAULT_WASM_MEMORY_MB));
        assert_eq!(read_limits(&json!({}), "jvm").wasm_memory_mb, None);
    }
}

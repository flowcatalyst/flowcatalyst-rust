//! Java `function/Manifest.java`: a function version's manifest.
//!
//! A function is always invoked over HTTP. `endpoints` declares its HTTP
//! surface and how the host authenticates each path; `subscriptions`,
//! `schedules` and `public` declare what the platform wires to an endpoint
//! at promote. A subscription or schedule needs a `webhook` endpoint to
//! deliver to.
//!
//! Three readers, as in Java:
//!
//! - [`Manifest::check`] walks the whole document and returns every
//!   independent problem, each with a JSON pointer, or the manifest.
//! - [`Manifest::parse_strict`], the publish reader, returns the first of
//!   those problems (same code and message) as the error.
//! - [`Manifest::read_stored`] reads an `fn_versions.manifest` row: tolerant
//!   of unknown keys and missing optionals, dropping a malformed entry rather
//!   than failing, and failing only when `runtime` or `entrypoint` is
//!   unreadable.
//!
//! `Manifest`'s `Serialize` writes the normalised stored form (every default
//! filled in, lower-case `runtime`/`auth`, upper-case methods and modes,
//! absent optionals omitted), so `read_stored(parse_strict(x).to_json())`
//! gives back an equal manifest.
//!
//! Problems are recorded in the order Java's parser records them: unknown
//! keys of an object as soon as it is entered, then its fields in a fixed
//! order. A problem is reported once: a field that depends on one that
//! failed (an entrypoint's rule on an invalid runtime, an endpoint's default
//! timeout on invalid limits, a subscription path that resolves to an
//! endpoint that failed for its own reason) is skipped, not reported again.
//! Pointers are built the way Java builds them, key names unescaped.

use std::collections::HashSet;

use crate::dns_label::DnsLabel;
use crate::endpoint_auth::EndpointAuth;
use crate::function_limits::{ClientCeilings, FunctionLimits};
use crate::hostname::Hostname;
use crate::http_method::HttpMethod;
use crate::java::is_blank as java_is_blank;
use crate::json::JsonNode;
use crate::route_pattern::RoutePattern;
use crate::runtime::Runtime;
use crate::setting_key::SettingKey;
use crate::subscription_mode::SubscriptionMode;
use crate::ValidationError;
use crate::LIVE_ALIAS;

/// A version's manifest, with every applicable default resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// Must match the function's own runtime (`RUNTIME_MISMATCH`).
    pub runtime: Runtime,
    /// A class name (JVM) or export name (WASM).
    pub entrypoint: String,
    /// The execution pool; [`Manifest::DEFAULT_POOL`] when absent.
    pub pool: DnsLabel,
    /// Keep the function warm; `false` when absent.
    pub warm: bool,
    /// The resolved limits, always fully populated.
    pub limits: Limits,
    pub endpoints: Vec<Endpoint>,
    pub subscriptions: Vec<SubscriptionSpec>,
    pub schedules: Vec<ScheduleSpec>,
    /// Wire key `public`.
    pub public_routes: Vec<PublicRoute>,
    /// Required config keys.
    pub config: Vec<String>,
    /// Required secret keys.
    pub secrets: Vec<String>,
    pub db: Vec<DbRef>,
    /// Outbound hosts this version may call.
    pub http_allow: Vec<String>,
}

/// A version's resolved limits, frozen at publish. `wasm_memory_mb` is
/// `None` for a runtime it does not apply to and always set otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_duration_ms: i32,
    pub max_concurrency: i32,
    pub wasm_memory_mb: Option<i32>,
}

/// One entry of the function's HTTP surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub path: RoutePattern,
    /// Required: there is no default.
    pub auth: EndpointAuth,
    /// Empty means every method. A `webhook` endpoint's, if given, must be
    /// exactly `[POST]`.
    pub methods: Vec<HttpMethod>,
    pub cors: Option<Cors>,
    /// [`Endpoint::DEFAULT_MAX_BODY_BYTES`] when absent.
    pub max_body_bytes: i32,
    /// The function's `maxDurationMs` when absent.
    pub timeout_ms: i32,
}

impl Endpoint {
    /// 1 MiB.
    pub const DEFAULT_MAX_BODY_BYTES: i32 = 1_048_576;

    /// The methods the host lets through: a `webhook` endpoint is always
    /// exactly `POST`, whatever is stored (spec `function-host-listener.md`
    /// §2 step 4); otherwise the declared methods, where empty means every
    /// method.
    pub fn effective_methods(&self) -> Vec<HttpMethod> {
        if self.auth == EndpointAuth::Webhook {
            vec![HttpMethod::Post]
        } else {
            self.methods.clone()
        }
    }
}

/// An event-type subscription the platform creates at promote. `path` is a
/// literal path that resolves to a `webhook` endpoint; one entry per
/// `event_type`.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionSpec {
    pub event_type: String,
    pub path: RoutePattern,
    /// [`Manifest::DEFAULT_SUBSCRIPTION_MODE`] when absent.
    pub mode: SubscriptionMode,
    pub max_retries: i32,
    pub timeout_seconds: i32,
    /// `false` when absent: a function sees the whole envelope.
    pub data_only: bool,
}

/// A cron schedule the platform creates at promote, with the same path rule
/// as a subscription. One entry per `(cron, timezone)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleSpec {
    pub cron: String,
    pub timezone: Option<String>,
    pub path: RoutePattern,
    /// Opaque JSON delivered with each firing.
    pub payload: Option<JsonNode>,
}

/// A `(hostname, pathPrefix)` pair exposed on the public listener.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicRoute {
    pub hostname: Hostname,
    /// A literal path; `/` when absent.
    pub path_prefix: RoutePattern,
    /// Opt-in alias prefixes: DNS labels, never `live`, no duplicates. Empty
    /// means an exact hostname match only.
    pub alias_prefixes: Vec<String>,
}

/// An endpoint's CORS policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cors {
    pub origins: Vec<String>,
    pub methods: Vec<String>,
    pub headers: Vec<String>,
    pub allow_credentials: bool,
}

/// A database connection this version needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbRef {
    /// Unique within the manifest.
    pub name: DnsLabel,
    /// The secret holding the DSN.
    pub secret_ref: String,
    /// Resolved against the client's `dbPoolSize` ceiling.
    pub pool_size: i32,
}

/// One problem [`Manifest::check`] found. `pointer` is where it is (`""` is
/// the document root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestProblem {
    pub code: &'static str,
    pub message: String,
    pub pointer: String,
}

impl ManifestProblem {
    /// The check route's error entry: the problem as a validation error
    /// carrying its pointer (the platform's `details.pointer`).
    pub fn to_validation_error(&self) -> ValidationError {
        ValidationError::new(self.code, self.message.clone()).with_pointer(self.pointer.clone())
    }
}

/// Every problem [`Manifest::check`] found; never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRejected {
    problems: Vec<ManifestProblem>,
}

impl ManifestRejected {
    pub fn problems(&self) -> &[ManifestProblem] {
        &self.problems
    }

    /// What publish rejects with: the first problem's code and message, and
    /// no pointer.
    pub fn first_error(&self) -> ValidationError {
        let first = &self.problems[0];
        ValidationError::new(first.code, first.message.clone())
    }
}

/// A stored manifest [`Manifest::read_stored`] cannot read (Java's
/// `IllegalStateException`): there is no safe default for `runtime` or
/// `entrypoint`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct UnreadableManifest(&'static str);

/// `subscription.Subscription.DEFAULT_MAX_RETRIES` in Java.
const SUBSCRIPTION_DEFAULT_MAX_RETRIES: i32 = 3;
/// `subscription.Subscription.DEFAULT_TIMEOUT_SECONDS` in Java.
const SUBSCRIPTION_DEFAULT_TIMEOUT_SECONDS: i32 = 30;
const DISPATCH_MODE_INVALID_MESSAGE: &str =
    "mode must be IMMEDIATE, NEXT_ON_ERROR or BLOCK_ON_ERROR";

// The parser's key set per object, which `function-manifest.schema.json`'s
// `properties` must equal (the platform's schema drift test reads them).
#[doc(hidden)]
pub const TOP_KEYS: &[&str] = &[
    "runtime",
    "entrypoint",
    "pool",
    "warm",
    "limits",
    "endpoints",
    "subscriptions",
    "schedules",
    "public",
    "config",
    "secrets",
    "db",
    "httpAllow",
];
#[doc(hidden)]
pub const LIMITS_KEYS: &[&str] = &["maxDurationMs", "maxConcurrency", "wasmMemoryMb"];
#[doc(hidden)]
pub const ENDPOINT_KEYS: &[&str] = &[
    "path",
    "auth",
    "methods",
    "cors",
    "maxBodyBytes",
    "timeoutMs",
];
#[doc(hidden)]
pub const SUBSCRIPTION_KEYS: &[&str] = &[
    "eventType",
    "path",
    "mode",
    "maxRetries",
    "timeoutSeconds",
    "dataOnly",
];
#[doc(hidden)]
pub const SCHEDULE_KEYS: &[&str] = &["cron", "timezone", "path", "payload"];
#[doc(hidden)]
pub const PUBLIC_ROUTE_KEYS: &[&str] = &["hostname", "pathPrefix", "aliasPrefixes"];
#[doc(hidden)]
pub const CORS_KEYS: &[&str] = &["origins", "methods", "headers", "allowCredentials"];
#[doc(hidden)]
pub const DB_KEYS: &[&str] = &["name", "secretRef", "poolSize"];
/// `$schema` points an editor at the JSON Schema: accepted at the top level,
/// type-checked, never written back.
const SCHEMA_KEY: &str = "$schema";

/// Present and not JSON `null` (Jackson's `!isMissingNode() && !isNull()`).
fn present(node: Option<&JsonNode>) -> Option<&JsonNode> {
    node.filter(|n| !n.is_null())
}

/// A non-blank string, in Java's sense of blank.
fn non_blank_str(node: Option<&JsonNode>) -> Option<&str> {
    node.and_then(JsonNode::as_str)
        .filter(|s| !java_is_blank(s))
}

fn positive_int(node: Option<&JsonNode>) -> Option<i32> {
    node.and_then(JsonNode::fits_int).filter(|v| *v > 0)
}

/// The problems found so far.
#[derive(Default)]
struct Collector {
    problems: Vec<ManifestProblem>,
}

impl Collector {
    fn add(&mut self, code: &'static str, message: impl Into<String>, pointer: impl Into<String>) {
        self.problems.push(ManifestProblem {
            code,
            message: message.into(),
            pointer: pointer.into(),
        });
    }

    /// Every key of `node` outside `allowed`, named by its full dotted path.
    fn reject_unknown(&mut self, node: &JsonNode, allowed: &[&str], dotted: &str, pointer: &str) {
        let Some(map) = node.as_object() else {
            return;
        };
        for key in map.keys() {
            if !allowed.contains(&key.as_str()) {
                let full = if dotted.is_empty() {
                    key.clone()
                } else {
                    format!("{dotted}.{key}")
                };
                self.add(
                    "MANIFEST_UNKNOWN_FIELD",
                    format!("{full} is not a recognised manifest field"),
                    format!("{pointer}/{key}"),
                );
            }
        }
    }
}

/// An `endpoints[i]` entry's outcome: `path` whenever the entry's own path
/// parsed, `endpoint` only when the whole entry did.
struct EndpointAttempt {
    path: RoutePattern,
    endpoint: Option<Endpoint>,
}

struct EndpointsResult {
    /// The entries that parsed with no problem.
    endpoints: Vec<Endpoint>,
    /// Every entry whose path parsed, valid or not: what a subscription's or
    /// schedule's path is matched against.
    attempts: Vec<EndpointAttempt>,
}

impl Manifest {
    /// The pool a manifest gets when it names none.
    pub const DEFAULT_POOL: &'static str = "default";
    /// A subscription's `mode` when absent. Not the router's default
    /// (`NEXT_ON_ERROR`): a manifest author who wants ordering asks for it.
    pub const DEFAULT_SUBSCRIPTION_MODE: SubscriptionMode = SubscriptionMode::Immediate;
    /// A subscription's `dataOnly` when absent.
    pub const DEFAULT_SUBSCRIPTION_DATA_ONLY: bool = false;

    /// Parses JSON text and checks it. A document that is not JSON at all is
    /// the HTTP layer's `INVALID_JSON`, not a manifest problem.
    pub fn check_text(
        text: &str,
        function_runtime: Runtime,
        defaults: &FunctionLimits,
        ceilings: &ClientCeilings,
    ) -> Result<Result<Manifest, ManifestRejected>, crate::json::JsonParseError> {
        let root = JsonNode::parse(text)?;
        Ok(Self::check(
            Some(&root),
            function_runtime,
            defaults,
            ceilings,
        ))
    }

    /// Every independent problem in the document, each with a pointer, or
    /// the manifest when there are none. `None` is an absent manifest.
    pub fn check(
        root: Option<&JsonNode>,
        function_runtime: Runtime,
        defaults: &FunctionLimits,
        ceilings: &ClientCeilings,
    ) -> Result<Manifest, ManifestRejected> {
        let Some(root) = root.filter(|r| r.is_object()) else {
            // The only problem when it occurs.
            return Err(ManifestRejected {
                problems: vec![ManifestProblem {
                    code: "MANIFEST_REQUIRED",
                    message: "manifest is required and must be an object".into(),
                    pointer: String::new(),
                }],
            });
        };

        let mut c = Collector::default();
        let mut top_keys = TOP_KEYS.to_vec();
        top_keys.push(SCHEMA_KEY);
        c.reject_unknown(root, &top_keys, "", "");
        if let Some(schema) = present(root.get(SCHEMA_KEY)) {
            if schema.as_str().is_none() {
                c.add("MANIFEST_INVALID", "$schema must be a string", "/$schema");
            }
        }

        let runtime = parse_runtime(&mut c, root, function_runtime);
        let entrypoint = parse_entrypoint(&mut c, root, runtime);
        let pool = parse_pool(&mut c, root);
        let warm = parse_warm(&mut c, root);
        let limits = parse_limits(&mut c, root, runtime, defaults, ceilings);
        let endpoints = parse_endpoints(&mut c, root, limits.as_ref(), ceilings);
        let subscriptions = parse_subscriptions(&mut c, root, &endpoints);
        let schedules = parse_schedules(&mut c, root, &endpoints);
        let public_routes = parse_public(&mut c, root);
        let db = parse_db(&mut c, root, defaults, ceilings);
        let config = parse_setting_key_list(&mut c, root, "config");
        let secrets = parse_setting_key_list(&mut c, root, "secrets");
        let http_allow = parse_simple_string_list(&mut c, root, "httpAllow");

        if !c.problems.is_empty() {
            return Err(ManifestRejected {
                problems: c.problems,
            });
        }
        // No problem recorded means every field parsed.
        let missing = "a field failed without recording a problem";
        Ok(Manifest {
            runtime: runtime.expect(missing),
            entrypoint: entrypoint.expect(missing),
            pool: pool.expect(missing),
            warm: warm.expect(missing),
            limits: limits.expect(missing),
            endpoints: endpoints.endpoints,
            subscriptions: subscriptions.expect(missing),
            schedules: schedules.expect(missing),
            public_routes: public_routes.expect(missing),
            config: config.expect(missing),
            secrets: secrets.expect(missing),
            db: db.expect(missing),
            http_allow: http_allow.expect(missing),
        })
    }

    /// The publish reader: [`Manifest::check`], failing with the first
    /// problem's code and message.
    pub fn parse_strict(
        root: Option<&JsonNode>,
        function_runtime: Runtime,
        defaults: &FunctionLimits,
        ceilings: &ClientCeilings,
    ) -> Result<Manifest, ValidationError> {
        Self::check(root, function_runtime, defaults, ceilings).map_err(|r| r.first_error())
    }

    /// Reads a stored `fn_versions.manifest`. Ignores unknown keys, applies
    /// no ceilings, and drops a malformed endpoint, subscription, schedule,
    /// public route or `db` entry instead of failing.
    pub fn read_stored(root: &JsonNode) -> Result<Manifest, UnreadableManifest> {
        if !root.is_object() {
            return Err(UnreadableManifest("manifest is unreadable: not an object"));
        }
        let runtime_text = root
            .get("runtime")
            .map(JsonNode::scalar_text)
            .unwrap_or_default();
        let runtime = Runtime::try_parse_strict(&runtime_text)
            .ok_or(UnreadableManifest("manifest runtime is unreadable"))?;
        let entrypoint = root
            .get("entrypoint")
            .map(JsonNode::scalar_text)
            .unwrap_or_default();
        if java_is_blank(&entrypoint) {
            return Err(UnreadableManifest("manifest entrypoint is unreadable"));
        }
        let endpoints = read_endpoints(root);
        Ok(Manifest {
            runtime,
            entrypoint,
            pool: read_pool(root),
            warm: read_bool(root, "warm", false),
            limits: read_limits(root, runtime),
            subscriptions: read_subscriptions(root, &endpoints),
            schedules: read_schedules(root, &endpoints),
            endpoints,
            public_routes: read_public_routes(root),
            db: read_db(root),
            config: read_setting_key_list(root, "config"),
            secrets: read_setting_key_list(root, "secrets"),
            http_allow: read_string_list(root, "httpAllow"),
        })
    }

    /// The `pool` of a stored manifest [`Manifest::read_stored`] may refuse:
    /// looks at nothing else, so it works on an otherwise corrupt manifest.
    /// `None` only when the document is not an object.
    pub fn peek_stored_pool(root: &JsonNode) -> Option<DnsLabel> {
        root.is_object().then(|| read_pool(root))
    }

    /// Whether any endpoint authenticates with `webhook`: what decides that
    /// a version is handed its application's signing secret (Java
    /// `DesiredState.hasWebhookEndpoint`, `Reconciler.hasWebhookEndpoint`).
    pub fn has_webhook_endpoint(&self) -> bool {
        self.endpoints
            .iter()
            .any(|e| e.auth == EndpointAuth::Webhook)
    }

    /// The normalised stored form, as a tree (see the [`serde::Serialize`]
    /// impl for the JSON).
    pub fn to_json(&self) -> JsonNode {
        let text = serde_json::to_string(self).expect("a manifest always serialises");
        JsonNode::parse(&text).expect("serde_json writes valid JSON")
    }
}

// ── check: top-level fields ─────────────────────────────────────────────

fn parse_runtime(c: &mut Collector, root: &JsonNode, function_runtime: Runtime) -> Option<Runtime> {
    let text = root
        .get("runtime")
        .map(JsonNode::scalar_text)
        .unwrap_or_default();
    let Some(runtime) = Runtime::try_parse_strict(&text) else {
        c.add("RUNTIME_INVALID", Runtime::invalid_message(), "/runtime");
        return None;
    };
    if runtime != function_runtime {
        c.add(
            "RUNTIME_MISMATCH",
            format!(
                "manifest runtime '{}' does not match the function's runtime '{}'",
                runtime.wire_value(),
                function_runtime.wire_value()
            ),
            "/runtime",
        );
        return None;
    }
    Some(runtime)
}

/// The rule depends on the runtime and is skipped when the runtime is
/// invalid.
fn parse_entrypoint(
    c: &mut Collector,
    root: &JsonNode,
    runtime: Option<Runtime>,
) -> Option<String> {
    let raw = root
        .get("entrypoint")
        .map(JsonNode::scalar_text)
        .unwrap_or_default();
    if java_is_blank(&raw) {
        c.add(
            "ENTRYPOINT_REQUIRED",
            "entrypoint is required",
            "/entrypoint",
        );
        return None;
    }
    if let Some(runtime) = runtime {
        let rule = runtime.entrypoint_rule();
        if !rule.matches(&raw) {
            c.add("ENTRYPOINT_INVALID", rule.invalid_message(), "/entrypoint");
            return None;
        }
    }
    Some(raw)
}

fn parse_pool(c: &mut Collector, root: &JsonNode) -> Option<DnsLabel> {
    let Some(node) = present(root.get("pool")) else {
        return Some(DnsLabel::new_unchecked(Manifest::DEFAULT_POOL));
    };
    match node.as_str().filter(|s| DnsLabel::is_valid(s)) {
        Some(pool) => Some(DnsLabel::new_unchecked(pool)),
        None => {
            c.add("POOL_INVALID", "pool must be a DNS label", "/pool");
            None
        }
    }
}

fn parse_warm(c: &mut Collector, root: &JsonNode) -> Option<bool> {
    let Some(node) = present(root.get("warm")) else {
        return Some(false);
    };
    if node.as_bool().is_none() {
        c.add("MANIFEST_INVALID", "warm must be a boolean", "/warm");
    }
    node.as_bool()
}

/// `maxDurationMs` and `maxConcurrency` are always checked;
/// `wasmMemoryMb` depends on the runtime and is left unresolved when the
/// runtime is invalid (the manifest is rejected anyway).
fn parse_limits(
    c: &mut Collector,
    root: &JsonNode,
    runtime: Option<Runtime>,
    defaults: &FunctionLimits,
    ceilings: &ClientCeilings,
) -> Option<Limits> {
    let empty = JsonNode::object();
    let node = match present(root.get("limits")) {
        Some(node) if !node.is_object() => {
            c.add("LIMIT_INVALID", "limits must be an object", "/limits");
            return None;
        }
        Some(node) => {
            c.reject_unknown(node, LIMITS_KEYS, "limits", "/limits");
            node
        }
        None => &empty,
    };

    let max_duration_ms = resolve_limit(
        c,
        node,
        "maxDurationMs",
        defaults.max_duration_ms(),
        ceilings.max_duration_ms(),
    );
    let max_concurrency = resolve_limit(
        c,
        node,
        "maxConcurrency",
        defaults.max_concurrency(),
        ceilings.max_concurrency(),
    );

    let mut wasm_memory_mb = None;
    let mut wasm_ok = true;
    if let Some(runtime) = runtime {
        if runtime.takes_wasm_memory() {
            match resolve_limit(
                c,
                node,
                "wasmMemoryMb",
                defaults.wasm_memory_mb(),
                ceilings.wasm_memory_mb(),
            ) {
                Some(mb) => wasm_memory_mb = Some(mb),
                None => wasm_ok = false,
            }
        } else if present(node.get("wasmMemoryMb")).is_some() {
            c.add(
                "LIMIT_NOT_APPLICABLE",
                format!(
                    "wasmMemoryMb is not applicable to a {} function",
                    runtime.wire_value()
                ),
                "/limits/wasmMemoryMb",
            );
            wasm_ok = false;
        }
    }

    let limits = Limits {
        max_duration_ms: max_duration_ms?,
        max_concurrency: max_concurrency?,
        wasm_memory_mb,
    };
    wasm_ok.then_some(limits)
}

/// Absent: `min(default, ceiling)`. Present: a positive `int`
/// (`LIMIT_INVALID`) not above the ceiling (`LIMIT_OVER_CEILING`).
fn resolve_limit(
    c: &mut Collector,
    limits: &JsonNode,
    key: &str,
    default: i32,
    ceiling: i32,
) -> Option<i32> {
    let Some(node) = present(limits.get(key)) else {
        return Some(default.min(ceiling));
    };
    let Some(value) = positive_int(Some(node)) else {
        c.add(
            "LIMIT_INVALID",
            format!("{key} must be a positive integer"),
            format!("/limits/{key}"),
        );
        return None;
    };
    if value > ceiling {
        c.add(
            "LIMIT_OVER_CEILING",
            format!("{key} is {value}, which exceeds the ceiling of {ceiling}"),
            format!("/limits/{key}"),
        );
        return None;
    }
    Some(value)
}

// ── check: endpoints ────────────────────────────────────────────────────

fn parse_endpoints(
    c: &mut Collector,
    root: &JsonNode,
    limits: Option<&Limits>,
    ceilings: &ClientCeilings,
) -> EndpointsResult {
    let mut result = EndpointsResult {
        endpoints: Vec::new(),
        attempts: Vec::new(),
    };
    let Some(node) = present(root.get("endpoints")) else {
        return result;
    };
    let Some(items) = node.as_array() else {
        c.add(
            "ENDPOINT_INVALID",
            "endpoints must be an array",
            "/endpoints",
        );
        return result;
    };
    for (i, item) in items.iter().enumerate() {
        let dotted = format!("endpoints[{i}]");
        let pointer = format!("/endpoints/{i}");
        let (path, endpoint) = parse_endpoint(c, item, &dotted, &pointer, limits, ceilings);
        if let Some(endpoint) = &endpoint {
            result.endpoints.push(endpoint.clone());
        }
        if let Some(path) = path {
            result.attempts.push(EndpointAttempt { path, endpoint });
        }
    }
    // Every ambiguous pair that shares a method (no methods means all).
    for (i, a) in result.endpoints.iter().enumerate() {
        for b in &result.endpoints[i + 1..] {
            if share_method(a, b) && a.path.ambiguous_with(&b.path) {
                c.add(
                    "ROUTE_AMBIGUOUS",
                    format!(
                        "endpoint '{}' and endpoint '{}' are ambiguous",
                        a.path.value(),
                        b.path.value()
                    ),
                    "/endpoints",
                );
            }
        }
    }
    result
}

fn share_method(a: &Endpoint, b: &Endpoint) -> bool {
    a.methods.is_empty() || b.methods.is_empty() || a.methods.iter().any(|m| b.methods.contains(m))
}

fn parse_endpoint(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
    limits: Option<&Limits>,
    ceilings: &ClientCeilings,
) -> (Option<RoutePattern>, Option<Endpoint>) {
    if !node.is_object() {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted} must be an object"),
            pointer,
        );
        return (None, None);
    }
    c.reject_unknown(node, ENDPOINT_KEYS, dotted, pointer);

    let path = parse_route_path(c, node, dotted, pointer);
    let auth = parse_endpoint_auth(c, node, dotted, pointer);
    let methods = parse_methods(c, node, dotted, pointer);
    let mut methods_auth_ok = true;
    if let (Some(EndpointAuth::Webhook), Some(methods)) = (auth, &methods) {
        if !methods.is_empty() && methods.as_slice() != [HttpMethod::Post] {
            c.add(
                "ENDPOINT_INVALID",
                format!(
                    "{dotted}: a webhook endpoint's methods, if given, must be exactly [\"POST\"]"
                ),
                pointer,
            );
            methods_auth_ok = false;
        }
    }
    let cors = match present(node.get("cors")) {
        None => Some(None),
        Some(cors) => parse_cors(c, cors, dotted, pointer).map(Some),
    };
    let max_body_bytes = parse_positive_or_default(
        c,
        node,
        "maxBodyBytes",
        dotted,
        pointer,
        Endpoint::DEFAULT_MAX_BODY_BYTES,
        "ENDPOINT_INVALID",
    );
    let timeout_ms = parse_timeout_ms(c, node, dotted, pointer, limits, ceilings);

    let endpoint = match (auth, methods, cors, max_body_bytes, timeout_ms, &path) {
        (
            Some(auth),
            Some(methods),
            Some(cors),
            Some(max_body_bytes),
            Some(timeout_ms),
            Some(path),
        ) if methods_auth_ok => Some(Endpoint {
            path: path.clone(),
            auth,
            methods,
            cors,
            max_body_bytes,
            timeout_ms,
        }),
        _ => None,
    };
    (path, endpoint)
}

fn parse_route_path(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
) -> Option<RoutePattern> {
    let Some(raw) = node.get("path").and_then(JsonNode::as_str) else {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted}.path is required"),
            format!("{pointer}/path"),
        );
        return None;
    };
    let parsed = RoutePattern::try_parse(raw);
    if parsed.is_none() {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted}.path: {}", RoutePattern::INVALID_MESSAGE),
            format!("{pointer}/path"),
        );
    }
    parsed
}

/// Absent is `ENDPOINT_AUTH_REQUIRED`; unrecognised is `ENDPOINT_INVALID`.
fn parse_endpoint_auth(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
) -> Option<EndpointAuth> {
    let Some(auth) = present(node.get("auth")) else {
        c.add(
            "ENDPOINT_AUTH_REQUIRED",
            format!("{dotted}.auth is required"),
            format!("{pointer}/auth"),
        );
        return None;
    };
    let Some(raw) = auth.as_str() else {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted}.auth must be a string"),
            format!("{pointer}/auth"),
        );
        return None;
    };
    let parsed = EndpointAuth::try_parse_strict(raw);
    if parsed.is_none() {
        c.add(
            "ENDPOINT_INVALID",
            EndpointAuth::INVALID_MESSAGE,
            format!("{pointer}/auth"),
        );
    }
    parsed
}

/// Absent means every method; present, a non-empty array of distinct known
/// methods, each entry checked on its own.
fn parse_methods(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
) -> Option<Vec<HttpMethod>> {
    let Some(methods) = present(node.get("methods")) else {
        return Some(Vec::new());
    };
    let Some(entries) = methods.as_array().filter(|a| !a.is_empty()) else {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted}.methods, if given, must be a non-empty array"),
            format!("{pointer}/methods"),
        );
        return None;
    };
    let mut out = Vec::new();
    let mut ok = true;
    for (i, entry) in entries.iter().enumerate() {
        let entry_pointer = format!("{pointer}/methods/{i}");
        let Some(raw) = entry.as_str() else {
            c.add(
                "ENDPOINT_INVALID",
                format!("{dotted}.methods entries must be strings"),
                entry_pointer,
            );
            ok = false;
            continue;
        };
        let Some(method) = HttpMethod::try_parse_strict(raw) else {
            c.add(
                "ENDPOINT_INVALID",
                format!("{dotted}.methods has an unrecognised entry '{raw}'"),
                entry_pointer,
            );
            ok = false;
            continue;
        };
        if out.contains(&method) {
            c.add(
                "ENDPOINT_INVALID",
                format!(
                    "{dotted}.methods has a duplicate entry '{}'",
                    method.as_str()
                ),
                entry_pointer,
            );
            ok = false;
            continue;
        }
        out.push(method);
    }
    ok.then_some(out)
}

/// Java's `\s`: space, `\t`, `\n`, `\x0B`, `\f`, `\r`.
fn is_java_regex_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// A CORS origin: exactly `*`, or `scheme://host[:port]` with no path,
/// query, fragment or userinfo (Java's
/// `^[A-Za-z][A-Za-z0-9+.-]*://[^/@?#\s]+(:\d+)?$`).
fn is_origin(origin: &str) -> bool {
    if origin == "*" {
        return true;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    let mut scheme_chars = scheme.chars();
    scheme_chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme_chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        && !rest.is_empty()
        && rest
            .chars()
            .all(|c| !matches!(c, '/' | '@' | '?' | '#') && !is_java_regex_space(c))
}

fn parse_cors(c: &mut Collector, node: &JsonNode, dotted: &str, pointer: &str) -> Option<Cors> {
    if !node.is_object() {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted}.cors must be an object"),
            format!("{pointer}/cors"),
        );
        return None;
    }
    let dotted = format!("{dotted}.cors");
    let pointer = format!("{pointer}/cors");
    c.reject_unknown(node, CORS_KEYS, &dotted, &pointer);

    let origins = parse_string_list(c, node, "origins", &dotted, &pointer);
    let mut origins_ok = true;
    if let Some(origins) = &origins {
        for (i, origin) in origins.iter().enumerate() {
            if !is_origin(origin) {
                c.add(
                    "ENDPOINT_INVALID",
                    format!(
                        "{dotted}.origins entry '{origin}' must be '*' or 'scheme://host[:port]' with no path"
                    ),
                    format!("{pointer}/origins/{i}"),
                );
                origins_ok = false;
            }
        }
    }
    let methods = parse_string_list(c, node, "methods", &dotted, &pointer);
    let headers = parse_string_list(c, node, "headers", &dotted, &pointer);
    let allow_credentials = match present(node.get("allowCredentials")) {
        None => Some(false),
        Some(value) => {
            if value.as_bool().is_none() {
                c.add(
                    "ENDPOINT_INVALID",
                    format!("{dotted}.allowCredentials must be a boolean"),
                    format!("{pointer}/allowCredentials"),
                );
            }
            value.as_bool()
        }
    };
    // `*` with credentials: browsers refuse it, and reflecting any origin
    // with credentials is the classic hole. Only once both inputs resolved.
    let mut cross_ok = true;
    if let (Some(origins), Some(true)) = (&origins, allow_credentials) {
        if origins.iter().any(|o| o == "*") {
            c.add(
                "ENDPOINT_INVALID",
                format!("{dotted}: origins must not contain '*' when allowCredentials is true"),
                pointer.clone(),
            );
            cross_ok = false;
        }
    }
    if !(origins_ok && cross_ok) {
        return None;
    }
    Some(Cors {
        origins: origins?,
        methods: methods?,
        headers: headers?,
        allow_credentials: allow_credentials?,
    })
}

/// A list of non-blank strings, each entry checked on its own.
fn parse_string_list(
    c: &mut Collector,
    node: &JsonNode,
    key: &str,
    dotted: &str,
    pointer: &str,
) -> Option<Vec<String>> {
    let Some(list) = present(node.get(key)) else {
        return Some(Vec::new());
    };
    let Some(entries) = list.as_array() else {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted}.{key} must be an array"),
            format!("{pointer}/{key}"),
        );
        return None;
    };
    let mut out = Vec::new();
    let mut ok = true;
    for (i, entry) in entries.iter().enumerate() {
        match non_blank_str(Some(entry)) {
            Some(value) => out.push(value.to_string()),
            None => {
                c.add(
                    "ENDPOINT_INVALID",
                    format!("{dotted}.{key} entries must be non-blank strings"),
                    format!("{pointer}/{key}/{i}"),
                );
                ok = false;
            }
        }
    }
    ok.then_some(out)
}

fn parse_positive_or_default(
    c: &mut Collector,
    node: &JsonNode,
    key: &str,
    dotted: &str,
    pointer: &str,
    default: i32,
    code: &'static str,
) -> Option<i32> {
    let Some(value) = present(node.get(key)) else {
        return Some(default);
    };
    let parsed = positive_int(Some(value));
    if parsed.is_none() {
        c.add(
            code,
            format!("{dotted}.{key} must be a positive integer"),
            format!("{pointer}/{key}"),
        );
    }
    parsed
}

/// Absent: the resolved `maxDurationMs`; skipped with no new problem when
/// the limits themselves are invalid.
fn parse_timeout_ms(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
    limits: Option<&Limits>,
    ceilings: &ClientCeilings,
) -> Option<i32> {
    let Some(value) = present(node.get("timeoutMs")) else {
        return limits.map(|l| l.max_duration_ms);
    };
    let Some(timeout_ms) = positive_int(Some(value)) else {
        c.add(
            "ENDPOINT_INVALID",
            format!("{dotted}.timeoutMs must be a positive integer"),
            format!("{pointer}/timeoutMs"),
        );
        return None;
    };
    if timeout_ms > ceilings.max_duration_ms() {
        c.add(
            "LIMIT_OVER_CEILING",
            format!(
                "timeoutMs is {timeout_ms}, which exceeds the ceiling of {}",
                ceilings.max_duration_ms()
            ),
            format!("{pointer}/timeoutMs"),
        );
        return None;
    }
    Some(timeout_ms)
}

// ── check: subscriptions and schedules ──────────────────────────────────

/// A literal path: a route pattern with only literal segments.
fn parse_literal_path(
    c: &mut Collector,
    node: &JsonNode,
    key: &str,
    dotted: &str,
    pointer: &str,
    code: &'static str,
) -> Option<RoutePattern> {
    let Some(raw) = node.get(key).and_then(JsonNode::as_str) else {
        c.add(
            code,
            format!("{dotted}.{key} is required"),
            format!("{pointer}/{key}"),
        );
        return None;
    };
    let Some(parsed) = RoutePattern::try_parse(raw) else {
        c.add(
            code,
            format!("{dotted}.{key}: {}", RoutePattern::INVALID_MESSAGE),
            format!("{pointer}/{key}"),
        );
        return None;
    };
    if !parsed.is_literal() {
        c.add(
            code,
            format!("{dotted}.{key} must be a literal path, not a pattern"),
            format!("{pointer}/{key}"),
        );
        return None;
    }
    Some(parsed)
}

/// The path must resolve to a `webhook` endpoint; when several endpoints
/// match, the most specific decides. When that endpoint failed to parse for
/// its own reason, it is not reported again here.
fn require_webhook_match(
    c: &mut Collector,
    literal: &RoutePattern,
    endpoints: &EndpointsResult,
    dotted: &str,
    pointer: &str,
    code: &'static str,
) -> bool {
    let winner = endpoints
        .attempts
        .iter()
        .filter(|a| a.path.matches(literal.value()).is_some())
        .min_by(|a, b| a.path.cmp(&b.path));
    let ok = match winner {
        None => false,
        Some(EndpointAttempt { endpoint: None, .. }) => true,
        Some(EndpointAttempt {
            endpoint: Some(endpoint),
            ..
        }) => endpoint.auth == EndpointAuth::Webhook,
    };
    if !ok {
        c.add(
            code,
            format!(
                "{dotted}.path '{}' does not match a webhook endpoint",
                literal.value()
            ),
            format!("{pointer}/path"),
        );
    }
    ok
}

fn parse_subscriptions(
    c: &mut Collector,
    root: &JsonNode,
    endpoints: &EndpointsResult,
) -> Option<Vec<SubscriptionSpec>> {
    let Some(node) = present(root.get("subscriptions")) else {
        return Some(Vec::new());
    };
    let Some(items) = node.as_array() else {
        c.add(
            "SUBSCRIPTION_INVALID",
            "subscriptions must be an array",
            "/subscriptions",
        );
        return None;
    };
    let mut specs = Vec::new();
    let mut event_types = HashSet::new();
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let dotted = format!("subscriptions[{i}]");
        let pointer = format!("/subscriptions/{i}");
        let Some(spec) = parse_subscription(c, item, &dotted, &pointer, endpoints) else {
            ok = false;
            continue;
        };
        if !event_types.insert(spec.event_type.clone()) {
            c.add(
                "SUBSCRIPTION_DUPLICATE",
                format!("duplicate subscription for eventType '{}'", spec.event_type),
                pointer,
            );
            ok = false;
            continue;
        }
        specs.push(spec);
    }
    ok.then_some(specs)
}

fn parse_subscription(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
    endpoints: &EndpointsResult,
) -> Option<SubscriptionSpec> {
    if !node.is_object() {
        c.add(
            "SUBSCRIPTION_INVALID",
            format!("{dotted} must be an object"),
            pointer,
        );
        return None;
    }
    c.reject_unknown(node, SUBSCRIPTION_KEYS, dotted, pointer);

    let event_type = non_blank_str(node.get("eventType")).map(str::to_string);
    if event_type.is_none() {
        c.add(
            "SUBSCRIPTION_INVALID",
            format!("{dotted}.eventType is required"),
            format!("{pointer}/eventType"),
        );
    }
    let path = parse_literal_path(c, node, "path", dotted, pointer, "SUBSCRIPTION_INVALID");
    let webhook_ok = match &path {
        None => true,
        Some(path) => require_webhook_match(
            c,
            path,
            endpoints,
            dotted,
            pointer,
            "SUBSCRIPTION_PATH_NOT_WEBHOOK",
        ),
    };
    let mode = parse_subscription_mode(c, node, dotted, pointer);
    let max_retries = parse_positive_or_default(
        c,
        node,
        "maxRetries",
        dotted,
        pointer,
        SUBSCRIPTION_DEFAULT_MAX_RETRIES,
        "SUBSCRIPTION_INVALID",
    );
    let timeout_seconds = parse_positive_or_default(
        c,
        node,
        "timeoutSeconds",
        dotted,
        pointer,
        SUBSCRIPTION_DEFAULT_TIMEOUT_SECONDS,
        "SUBSCRIPTION_INVALID",
    );
    let data_only = parse_bool_or_default(
        c,
        node,
        "dataOnly",
        dotted,
        pointer,
        Manifest::DEFAULT_SUBSCRIPTION_DATA_ONLY,
        "SUBSCRIPTION_INVALID",
    );
    if !webhook_ok {
        return None;
    }
    Some(SubscriptionSpec {
        event_type: event_type?,
        path: path?,
        mode: mode?,
        max_retries: max_retries?,
        timeout_seconds: timeout_seconds?,
        data_only: data_only?,
    })
}

/// Java `DispatchMode.tryParseStrict`: the exact constant name, nothing
/// else (unlike the router's lenient reader).
fn dispatch_mode_strict(raw: &str) -> Option<SubscriptionMode> {
    raw.parse().ok()
}

fn parse_subscription_mode(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
) -> Option<SubscriptionMode> {
    let Some(mode) = present(node.get("mode")) else {
        return Some(Manifest::DEFAULT_SUBSCRIPTION_MODE);
    };
    let Some(raw) = mode.as_str() else {
        c.add(
            "SUBSCRIPTION_INVALID",
            format!("{dotted}.mode must be a string"),
            format!("{pointer}/mode"),
        );
        return None;
    };
    let parsed = dispatch_mode_strict(raw);
    if parsed.is_none() {
        c.add(
            "SUBSCRIPTION_INVALID",
            format!("{dotted}.mode: {DISPATCH_MODE_INVALID_MESSAGE}"),
            format!("{pointer}/mode"),
        );
    }
    parsed
}

fn parse_bool_or_default(
    c: &mut Collector,
    node: &JsonNode,
    key: &str,
    dotted: &str,
    pointer: &str,
    default: bool,
    code: &'static str,
) -> Option<bool> {
    let Some(value) = present(node.get(key)) else {
        return Some(default);
    };
    if value.as_bool().is_none() {
        c.add(
            code,
            format!("{dotted}.{key} must be a boolean"),
            format!("{pointer}/{key}"),
        );
    }
    value.as_bool()
}

fn parse_schedules(
    c: &mut Collector,
    root: &JsonNode,
    endpoints: &EndpointsResult,
) -> Option<Vec<ScheduleSpec>> {
    let Some(node) = present(root.get("schedules")) else {
        return Some(Vec::new());
    };
    let Some(items) = node.as_array() else {
        c.add(
            "SCHEDULE_INVALID",
            "schedules must be an array",
            "/schedules",
        );
        return None;
    };
    let mut specs = Vec::new();
    let mut seen = HashSet::new();
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let dotted = format!("schedules[{i}]");
        let pointer = format!("/schedules/{i}");
        let Some(spec) = parse_schedule(c, item, &dotted, &pointer, endpoints) else {
            ok = false;
            continue;
        };
        // NUL-joined: a cron holds spaces, so a space-joined key collides.
        let key = format!("{}\0{}", spec.cron, spec.timezone.as_deref().unwrap_or(""));
        if !seen.insert(key) {
            c.add(
                "SCHEDULE_DUPLICATE",
                format!(
                    "duplicate schedule for cron '{}' timezone '{}'",
                    spec.cron,
                    // Java concatenates a null timezone as "null".
                    spec.timezone.as_deref().unwrap_or("null")
                ),
                pointer,
            );
            ok = false;
            continue;
        }
        specs.push(spec);
    }
    ok.then_some(specs)
}

fn parse_schedule(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
    endpoints: &EndpointsResult,
) -> Option<ScheduleSpec> {
    if !node.is_object() {
        c.add(
            "SCHEDULE_INVALID",
            format!("{dotted} must be an object"),
            pointer,
        );
        return None;
    }
    c.reject_unknown(node, SCHEDULE_KEYS, dotted, pointer);

    let cron = non_blank_str(node.get("cron")).map(str::to_string);
    if cron.is_none() {
        c.add(
            "SCHEDULE_INVALID",
            format!("{dotted}.cron is required"),
            format!("{pointer}/cron"),
        );
    }
    let timezone = match present(node.get("timezone")) {
        None => Some(None),
        Some(value) => match value.as_str() {
            Some(tz) => Some(Some(tz.to_string())),
            None => {
                c.add(
                    "SCHEDULE_INVALID",
                    format!("{dotted}.timezone must be a string"),
                    format!("{pointer}/timezone"),
                );
                None
            }
        },
    };
    let path = parse_literal_path(c, node, "path", dotted, pointer, "SCHEDULE_INVALID");
    let webhook_ok = match &path {
        None => true,
        Some(path) => require_webhook_match(
            c,
            path,
            endpoints,
            dotted,
            pointer,
            "SCHEDULE_PATH_NOT_WEBHOOK",
        ),
    };
    let payload = present(node.get("payload")).cloned();
    if !webhook_ok {
        return None;
    }
    Some(ScheduleSpec {
        cron: cron?,
        timezone: timezone?,
        path: path?,
        payload,
    })
}

// ── check: public routes ────────────────────────────────────────────────

fn parse_public(c: &mut Collector, root: &JsonNode) -> Option<Vec<PublicRoute>> {
    let Some(node) = present(root.get("public")) else {
        return Some(Vec::new());
    };
    let Some(items) = node.as_array() else {
        c.add("PUBLIC_ROUTE_INVALID", "public must be an array", "/public");
        return None;
    };
    let mut routes = Vec::new();
    let mut seen = HashSet::new();
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let dotted = format!("public[{i}]");
        let pointer = format!("/public/{i}");
        let Some(route) = parse_public_route(c, item, &dotted, &pointer) else {
            ok = false;
            continue;
        };
        let key = format!("{}\0{}", route.hostname.value(), route.path_prefix.value());
        if !seen.insert(key) {
            c.add(
                "PUBLIC_ROUTE_DUPLICATE",
                format!(
                    "duplicate public route for '{}{}'",
                    route.hostname.value(),
                    route.path_prefix.value()
                ),
                pointer,
            );
            ok = false;
            continue;
        }
        routes.push(route);
    }
    ok.then_some(routes)
}

fn parse_public_route(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
) -> Option<PublicRoute> {
    if !node.is_object() {
        c.add(
            "PUBLIC_ROUTE_INVALID",
            format!("{dotted} must be an object"),
            pointer,
        );
        return None;
    }
    c.reject_unknown(node, PUBLIC_ROUTE_KEYS, dotted, pointer);

    let hostname = match node.get("hostname").and_then(JsonNode::as_str) {
        None => {
            c.add(
                "PUBLIC_ROUTE_INVALID",
                format!("{dotted}.hostname is required"),
                format!("{pointer}/hostname"),
            );
            None
        }
        Some(raw) => {
            let parsed = Hostname::try_parse(raw);
            if parsed.is_none() {
                c.add(
                    "PUBLIC_ROUTE_INVALID",
                    format!("{dotted}.hostname: {}", Hostname::INVALID_MESSAGE),
                    format!("{pointer}/hostname"),
                );
            }
            parsed
        }
    };
    let path_prefix = match present(node.get("pathPrefix")) {
        None => Some(default_path_prefix()),
        Some(_) => parse_literal_path(
            c,
            node,
            "pathPrefix",
            dotted,
            pointer,
            "PUBLIC_ROUTE_INVALID",
        ),
    };
    let alias_prefixes = parse_alias_prefixes(c, node, dotted, pointer);
    Some(PublicRoute {
        hostname: hostname?,
        path_prefix: path_prefix?,
        alias_prefixes: alias_prefixes?,
    })
}

fn default_path_prefix() -> RoutePattern {
    RoutePattern::try_parse("/").expect("/ is a route pattern")
}

/// Each entry a DNS label, never `live` (reserved for the exact hostname),
/// no duplicates; each checked on its own.
fn parse_alias_prefixes(
    c: &mut Collector,
    node: &JsonNode,
    dotted: &str,
    pointer: &str,
) -> Option<Vec<String>> {
    let Some(aliases) = present(node.get("aliasPrefixes")) else {
        return Some(Vec::new());
    };
    let Some(entries) = aliases.as_array() else {
        c.add(
            "PUBLIC_ROUTE_INVALID",
            format!("{dotted}.aliasPrefixes must be an array"),
            format!("{pointer}/aliasPrefixes"),
        );
        return None;
    };
    let mut out: Vec<String> = Vec::new();
    let mut ok = true;
    for (i, entry) in entries.iter().enumerate() {
        let entry_dotted = format!("{dotted}.aliasPrefixes[{i}]");
        let entry_pointer = format!("{pointer}/aliasPrefixes/{i}");
        let Some(value) = entry.as_str().filter(|v| DnsLabel::is_valid(v)) else {
            c.add(
                "PUBLIC_ROUTE_INVALID",
                format!("{entry_dotted} must be a DNS label"),
                entry_pointer,
            );
            ok = false;
            continue;
        };
        if value == LIVE_ALIAS {
            c.add(
                "PUBLIC_ROUTE_INVALID",
                format!("{entry_dotted} must not be 'live'"),
                entry_pointer,
            );
            ok = false;
            continue;
        }
        if out.iter().any(|v| v == value) {
            c.add(
                "PUBLIC_ROUTE_INVALID",
                format!("{entry_dotted} is a duplicate"),
                entry_pointer,
            );
            ok = false;
            continue;
        }
        out.push(value.to_string());
    }
    ok.then_some(out)
}

// ── check: db, config, secrets, httpAllow ───────────────────────────────

fn parse_db(
    c: &mut Collector,
    root: &JsonNode,
    defaults: &FunctionLimits,
    ceilings: &ClientCeilings,
) -> Option<Vec<DbRef>> {
    let Some(node) = present(root.get("db")) else {
        return Some(Vec::new());
    };
    let Some(items) = node.as_array() else {
        c.add("DB_INVALID", "db must be an array", "/db");
        return None;
    };
    let mut refs = Vec::new();
    let mut names = HashSet::new();
    let mut ok = true;
    for (i, entry) in items.iter().enumerate() {
        let dotted = format!("db[{i}]");
        let pointer = format!("/db/{i}");
        if !entry.is_object() {
            c.add("DB_INVALID", format!("{dotted} must be an object"), pointer);
            ok = false;
            continue;
        }
        c.reject_unknown(entry, DB_KEYS, &dotted, &pointer);

        let name = match entry.get("name").and_then(JsonNode::as_str) {
            Some(n) if DnsLabel::is_valid(n) => {
                if names.insert(n.to_string()) {
                    Some(n)
                } else {
                    c.add(
                        "DB_INVALID",
                        format!("{dotted}.name '{n}' is duplicated"),
                        format!("{pointer}/name"),
                    );
                    None
                }
            }
            _ => {
                c.add(
                    "DB_INVALID",
                    format!("{dotted}.name must be a DNS label"),
                    format!("{pointer}/name"),
                );
                None
            }
        };
        let secret_ref = match non_blank_str(entry.get("secretRef")) {
            None => {
                c.add(
                    "DB_INVALID",
                    format!("{dotted}.secretRef is required"),
                    format!("{pointer}/secretRef"),
                );
                None
            }
            Some(s) if !SettingKey::is_valid(s) => {
                c.add(
                    "DB_INVALID",
                    format!("{dotted}.secretRef: {}", SettingKey::invalid_message(s)),
                    format!("{pointer}/secretRef"),
                );
                None
            }
            Some(s) => Some(s),
        };
        let pool_size = resolve_pool_size(
            c,
            entry,
            &dotted,
            &pointer,
            defaults.db_pool_size(),
            ceilings.db_pool_size(),
        );
        match (name, secret_ref, pool_size) {
            (Some(name), Some(secret_ref), Some(pool_size)) => refs.push(DbRef {
                name: DnsLabel::new_unchecked(name),
                secret_ref: secret_ref.to_string(),
                pool_size,
            }),
            _ => ok = false,
        }
    }
    ok.then_some(refs)
}

fn resolve_pool_size(
    c: &mut Collector,
    entry: &JsonNode,
    dotted: &str,
    pointer: &str,
    default: i32,
    ceiling: i32,
) -> Option<i32> {
    let Some(node) = present(entry.get("poolSize")) else {
        return Some(default.min(ceiling));
    };
    let Some(value) = positive_int(Some(node)) else {
        c.add(
            "DB_INVALID",
            format!("{dotted}.poolSize must be a positive integer"),
            format!("{pointer}/poolSize"),
        );
        return None;
    };
    if value > ceiling {
        c.add(
            "LIMIT_OVER_CEILING",
            format!("{dotted}.poolSize is {value}, which exceeds the ceiling of {ceiling}"),
            format!("{pointer}/poolSize"),
        );
        return None;
    }
    Some(value)
}

/// `httpAllow`: distinct non-blank strings.
fn parse_simple_string_list(c: &mut Collector, root: &JsonNode, key: &str) -> Option<Vec<String>> {
    parse_config_list(c, root, key, false)
}

/// `config` / `secrets`: distinct setting keys, reported as `CONFIG_INVALID`.
fn parse_setting_key_list(c: &mut Collector, root: &JsonNode, key: &str) -> Option<Vec<String>> {
    parse_config_list(c, root, key, true)
}

fn parse_config_list(
    c: &mut Collector,
    root: &JsonNode,
    key: &str,
    setting_keys: bool,
) -> Option<Vec<String>> {
    let Some(node) = present(root.get(key)) else {
        return Some(Vec::new());
    };
    let Some(entries) = node.as_array() else {
        c.add(
            "CONFIG_INVALID",
            format!("{key} must be an array"),
            format!("/{key}"),
        );
        return None;
    };
    let mut out: Vec<String> = Vec::new();
    let mut ok = true;
    for (i, entry) in entries.iter().enumerate() {
        let pointer = format!("/{key}/{i}");
        let Some(value) = non_blank_str(Some(entry)) else {
            c.add(
                "CONFIG_INVALID",
                format!("{key} entries must be non-blank strings"),
                pointer,
            );
            ok = false;
            continue;
        };
        if setting_keys && !SettingKey::is_valid(value) {
            c.add(
                "CONFIG_INVALID",
                format!("{key}: {}", SettingKey::invalid_message(value)),
                pointer,
            );
            ok = false;
            continue;
        }
        if out.iter().any(|v| v == value) {
            c.add(
                "CONFIG_INVALID",
                format!("{key} has a duplicate entry '{value}'"),
                pointer,
            );
            ok = false;
            continue;
        }
        out.push(value.to_string());
    }
    ok.then_some(out)
}

// ── read_stored ─────────────────────────────────────────────────────────

fn read_pool(root: &JsonNode) -> DnsLabel {
    match root.get("pool").and_then(JsonNode::as_str) {
        Some(pool) if DnsLabel::is_valid(pool) => DnsLabel::new_unchecked(pool),
        _ => DnsLabel::new_unchecked(Manifest::DEFAULT_POOL),
    }
}

fn read_bool(node: &JsonNode, key: &str, default: bool) -> bool {
    node.get(key).and_then(JsonNode::as_bool).unwrap_or(default)
}

fn read_positive_int(node: Option<&JsonNode>, key: &str, default: i32) -> i32 {
    positive_int(node.and_then(|n| n.get(key))).unwrap_or(default)
}

fn read_limits(root: &JsonNode, runtime: Runtime) -> Limits {
    let node = root.get("limits");
    Limits {
        max_duration_ms: read_positive_int(
            node,
            "maxDurationMs",
            FunctionLimits::DEFAULT_MAX_DURATION_MS,
        ),
        max_concurrency: read_positive_int(
            node,
            "maxConcurrency",
            FunctionLimits::DEFAULT_MAX_CONCURRENCY,
        ),
        wasm_memory_mb: runtime.takes_wasm_memory().then(|| {
            read_positive_int(node, "wasmMemoryMb", FunctionLimits::DEFAULT_WASM_MEMORY_MB)
        }),
    }
}

fn read_array<'a>(node: &'a JsonNode, key: &str) -> &'a [JsonNode] {
    node.get(key).and_then(JsonNode::as_array).unwrap_or(&[])
}

fn read_endpoints(root: &JsonNode) -> Vec<Endpoint> {
    read_array(root, "endpoints")
        .iter()
        .filter_map(read_endpoint)
        .collect()
}

fn read_endpoint(node: &JsonNode) -> Option<Endpoint> {
    if !node.is_object() {
        return None;
    }
    let path = RoutePattern::try_parse(node.get("path")?.as_str()?)?;
    let auth = EndpointAuth::try_parse_strict(node.get("auth")?.as_str()?)?;
    let methods = read_array(node, "methods")
        .iter()
        .filter_map(|m| m.as_str().and_then(HttpMethod::try_parse_strict))
        .collect();
    let cors = node.get("cors").filter(|c| c.is_object()).map(|cors| Cors {
        origins: read_string_list(cors, "origins"),
        methods: read_string_list(cors, "methods"),
        headers: read_string_list(cors, "headers"),
        allow_credentials: read_bool(cors, "allowCredentials", false),
    });
    Some(Endpoint {
        path,
        auth,
        methods,
        cors,
        max_body_bytes: read_positive_int(
            Some(node),
            "maxBodyBytes",
            Endpoint::DEFAULT_MAX_BODY_BYTES,
        ),
        timeout_ms: read_positive_int(
            Some(node),
            "timeoutMs",
            FunctionLimits::DEFAULT_MAX_DURATION_MS,
        ),
    })
}

fn read_literal_path(node: &JsonNode, key: &str) -> Option<RoutePattern> {
    RoutePattern::try_parse(node.get(key)?.as_str()?).filter(RoutePattern::is_literal)
}

fn read_webhook_match(literal: &RoutePattern, endpoints: &[Endpoint]) -> bool {
    endpoints
        .iter()
        .filter(|e| e.path.matches(literal.value()).is_some())
        .min_by(|a, b| a.path.cmp(&b.path))
        .is_some_and(|e| e.auth == EndpointAuth::Webhook)
}

fn read_subscriptions(root: &JsonNode, endpoints: &[Endpoint]) -> Vec<SubscriptionSpec> {
    read_array(root, "subscriptions")
        .iter()
        .filter_map(|node| {
            if !node.is_object() {
                return None;
            }
            let event_type = non_blank_str(node.get("eventType"))?;
            let path = read_literal_path(node, "path")?;
            if !read_webhook_match(&path, endpoints) {
                return None;
            }
            Some(SubscriptionSpec {
                event_type: event_type.to_string(),
                path,
                mode: node
                    .get("mode")
                    .and_then(JsonNode::as_str)
                    .and_then(dispatch_mode_strict)
                    .unwrap_or(Manifest::DEFAULT_SUBSCRIPTION_MODE),
                max_retries: read_positive_int(
                    Some(node),
                    "maxRetries",
                    SUBSCRIPTION_DEFAULT_MAX_RETRIES,
                ),
                timeout_seconds: read_positive_int(
                    Some(node),
                    "timeoutSeconds",
                    SUBSCRIPTION_DEFAULT_TIMEOUT_SECONDS,
                ),
                data_only: read_bool(node, "dataOnly", Manifest::DEFAULT_SUBSCRIPTION_DATA_ONLY),
            })
        })
        .collect()
}

fn read_schedules(root: &JsonNode, endpoints: &[Endpoint]) -> Vec<ScheduleSpec> {
    read_array(root, "schedules")
        .iter()
        .filter_map(|node| {
            if !node.is_object() {
                return None;
            }
            let cron = non_blank_str(node.get("cron"))?;
            let path = read_literal_path(node, "path")?;
            if !read_webhook_match(&path, endpoints) {
                return None;
            }
            Some(ScheduleSpec {
                cron: cron.to_string(),
                timezone: node
                    .get("timezone")
                    .and_then(JsonNode::as_str)
                    .map(str::to_string),
                path,
                payload: present(node.get("payload")).cloned(),
            })
        })
        .collect()
}

fn read_public_routes(root: &JsonNode) -> Vec<PublicRoute> {
    read_array(root, "public")
        .iter()
        .filter_map(|node| {
            if !node.is_object() {
                return None;
            }
            let hostname = Hostname::try_parse(node.get("hostname")?.as_str()?)?;
            let path_prefix = match present(node.get("pathPrefix")) {
                None => default_path_prefix(),
                Some(_) => read_literal_path(node, "pathPrefix")?,
            };
            // A malformed alias entry is dropped, not the route.
            let mut alias_prefixes: Vec<String> = Vec::new();
            for alias in read_array(node, "aliasPrefixes")
                .iter()
                .filter_map(JsonNode::as_str)
            {
                if DnsLabel::is_valid(alias)
                    && alias != LIVE_ALIAS
                    && !alias_prefixes.iter().any(|a| a == alias)
                {
                    alias_prefixes.push(alias.to_string());
                }
            }
            Some(PublicRoute {
                hostname,
                path_prefix,
                alias_prefixes,
            })
        })
        .collect()
}

fn read_db(root: &JsonNode) -> Vec<DbRef> {
    read_array(root, "db")
        .iter()
        .filter_map(|node| {
            if !node.is_object() {
                return None;
            }
            let name = node
                .get("name")
                .and_then(JsonNode::as_str)
                .filter(|n| DnsLabel::is_valid(n))?;
            let secret_ref = node
                .get("secretRef")
                .and_then(JsonNode::as_str)
                .filter(|s| SettingKey::is_valid(s))?;
            Some(DbRef {
                name: DnsLabel::new_unchecked(name),
                secret_ref: secret_ref.to_string(),
                pool_size: read_positive_int(
                    Some(node),
                    "poolSize",
                    FunctionLimits::DEFAULT_DB_POOL_SIZE,
                ),
            })
        })
        .collect()
}

fn read_string_list(node: &JsonNode, key: &str) -> Vec<String> {
    read_array(node, key)
        .iter()
        .filter_map(|e| non_blank_str(Some(e)))
        .map(str::to_string)
        .collect()
}

fn read_setting_key_list(root: &JsonNode, key: &str) -> Vec<String> {
    read_array(root, key)
        .iter()
        .filter_map(JsonNode::as_str)
        .filter(|s| SettingKey::is_valid(s))
        .map(str::to_string)
        .collect()
}

// ── the stored form ─────────────────────────────────────────────────────

/// The normalised stored form: every default filled in, lower-case
/// `runtime`/`auth`, upper-case methods and modes, absent optionals omitted.
/// Keys are written in field order, so one manifest always gives the same
/// bytes.
impl serde::Serialize for Manifest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let m = self;
        ManifestWire {
            runtime: m.runtime.wire_value(),
            entrypoint: &m.entrypoint,
            pool: m.pool.value(),
            warm: m.warm,
            limits: LimitsWire {
                max_duration_ms: m.limits.max_duration_ms,
                max_concurrency: m.limits.max_concurrency,
                wasm_memory_mb: m.limits.wasm_memory_mb,
            },
            endpoints: m
                .endpoints
                .iter()
                .map(|e| EndpointWire {
                    path: e.path.value(),
                    auth: e.auth.wire_value(),
                    methods: e.methods.iter().map(|m| m.as_str()).collect(),
                    cors: e.cors.as_ref().map(|c| CorsWire {
                        origins: &c.origins,
                        methods: &c.methods,
                        headers: &c.headers,
                        allow_credentials: c.allow_credentials,
                    }),
                    max_body_bytes: e.max_body_bytes,
                    timeout_ms: e.timeout_ms,
                })
                .collect(),
            subscriptions: m
                .subscriptions
                .iter()
                .map(|s| SubscriptionWire {
                    event_type: &s.event_type,
                    path: s.path.value(),
                    mode: s.mode.as_str(),
                    max_retries: s.max_retries,
                    timeout_seconds: s.timeout_seconds,
                    data_only: s.data_only,
                })
                .collect(),
            schedules: m
                .schedules
                .iter()
                .map(|s| ScheduleWire {
                    cron: &s.cron,
                    timezone: s.timezone.as_deref(),
                    path: s.path.value(),
                    payload: s.payload.as_ref(),
                })
                .collect(),
            public_routes: m
                .public_routes
                .iter()
                .map(|r| PublicRouteWire {
                    hostname: r.hostname.value(),
                    path_prefix: r.path_prefix.value(),
                    alias_prefixes: &r.alias_prefixes,
                })
                .collect(),
            config: &m.config,
            secrets: &m.secrets,
            db: m
                .db
                .iter()
                .map(|d| DbRefWire {
                    name: d.name.value(),
                    secret_ref: &d.secret_ref,
                    pool_size: d.pool_size,
                })
                .collect(),
            http_allow: &m.http_allow,
        }
        .serialize(serializer)
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestWire<'a> {
    runtime: &'a str,
    entrypoint: &'a str,
    pool: &'a str,
    warm: bool,
    limits: LimitsWire,
    endpoints: Vec<EndpointWire<'a>>,
    subscriptions: Vec<SubscriptionWire<'a>>,
    schedules: Vec<ScheduleWire<'a>>,
    #[serde(rename = "public")]
    public_routes: Vec<PublicRouteWire<'a>>,
    config: &'a [String],
    secrets: &'a [String],
    db: Vec<DbRefWire<'a>>,
    http_allow: &'a [String],
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LimitsWire {
    max_duration_ms: i32,
    max_concurrency: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    wasm_memory_mb: Option<i32>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EndpointWire<'a> {
    path: &'a str,
    auth: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cors: Option<CorsWire<'a>>,
    max_body_bytes: i32,
    timeout_ms: i32,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CorsWire<'a> {
    origins: &'a [String],
    methods: &'a [String],
    headers: &'a [String],
    allow_credentials: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionWire<'a> {
    event_type: &'a str,
    path: &'a str,
    mode: &'a str,
    max_retries: i32,
    timeout_seconds: i32,
    data_only: bool,
}

#[derive(serde::Serialize)]
struct ScheduleWire<'a> {
    cron: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    timezone: Option<&'a str>,
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<&'a JsonNode>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicRouteWire<'a> {
    hostname: &'a str,
    path_prefix: &'a str,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    alias_prefixes: &'a [String],
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DbRefWire<'a> {
    name: &'a str,
    secret_ref: &'a str,
    pool_size: i32,
}

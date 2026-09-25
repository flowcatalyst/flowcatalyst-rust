//! Java `ManifestTest` and the `$schema` part of `FunctionManifestSchemaTest`,
//! ported. Every single-mistake case of Java's table, and many more, also
//! runs against Java's own answers in `manifest_golden.rs`.

use fc_function_model::{
    ClientCeilings, Cors, DbRef, DnsLabel, EndpointAuth, FunctionLimits, Hostname, HttpMethod,
    JsonNode, Manifest, RoutePattern, Runtime, SubscriptionMode,
};

fn defaults() -> FunctionLimits {
    FunctionLimits::defaults()
}

fn unrestricted() -> ClientCeilings {
    ClientCeilings::of(&defaults())
}

fn tight() -> ClientCeilings {
    ClientCeilings::new(30_000, 10, 64, 4).unwrap()
}

fn tree(json: &str) -> JsonNode {
    JsonNode::parse(json).unwrap()
}

fn parse(
    json: &str,
    runtime: Runtime,
    ceilings: &ClientCeilings,
) -> Result<Manifest, (String, String)> {
    Manifest::parse_strict(Some(&tree(json)), runtime, &defaults(), ceilings)
        .map_err(|e| (e.code().to_string(), e.message().to_string()))
}

fn parse_jvm(json: &str) -> Manifest {
    parse(json, Runtime::Jvm, &unrestricted()).unwrap()
}

fn reject_jvm(json: &str) -> (String, String) {
    parse(json, Runtime::Jvm, &unrestricted()).unwrap_err()
}

const MINIMAL_JVM: &str = r#"{
  "runtime": "jvm",
  "entrypoint": "com.acme.billing.CreateInvoice",
  "endpoints": [ { "path": "/events/invoice-created", "auth": "webhook" } ],
  "subscriptions": [ { "eventType": "billing:invoices:invoice:created", "path": "/events/invoice-created" } ]
}"#;

const FULL_JVM: &str = r#"{
  "runtime": "jvm",
  "entrypoint": "com.acme.billing.CreateInvoice",
  "pool": "default",
  "warm": false,
  "limits": { "maxDurationMs": 30000, "maxConcurrency": 32 },
  "endpoints": [
    { "path": "/events/*",  "auth": "webhook" },
    { "path": "/jobs/*",    "auth": "webhook" },
    { "path": "/api/*",     "auth": "platform", "methods": ["GET","POST"],
      "cors": { "origins": ["https://app.acme.com"] },
      "maxBodyBytes": 1048576, "timeoutMs": 10000 },
    { "path": "/hooks/stripe", "auth": "none" }
  ],
  "subscriptions": [
    { "eventType": "billing:invoices:invoice:created", "path": "/events/invoice-created",
      "mode": "BLOCK_ON_ERROR", "maxRetries": 3, "timeoutSeconds": 30, "dataOnly": false }
  ],
  "schedules": [ { "cron": "0 * * * *", "timezone": "UTC", "path": "/jobs/hourly", "payload": { "x": 1 } } ],
  "public":    [ { "hostname": "api.acme.com", "pathPrefix": "/" } ],
  "config": ["INVOICE_PREFIX"],
  "secrets": ["billing/stripe-key"],
  "db": [ { "name": "main", "secretRef": "billing/dsn", "poolSize": 4 } ],
  "httpAllow": ["api.stripe.com"]
}"#;

fn with_webhook_and_subscription(body: &str) -> String {
    format!(
        r#"{{"runtime":"jvm","entrypoint":"x","endpoints":[{{"path":"/events/*","auth":"webhook"}}],"subscriptions":[{{{body}}}]}}"#
    )
}

fn with_public(body: &str) -> String {
    format!(r#"{{"runtime":"jvm","entrypoint":"x","public":[{{{body}}}]}}"#)
}

// ── the full happy path ─────────────────────────────────────────────────

#[test]
fn full_manifest_every_component() {
    let m = parse_jvm(FULL_JVM);
    assert_eq!(m.runtime, Runtime::Jvm);
    assert_eq!(m.entrypoint, "com.acme.billing.CreateInvoice");
    assert_eq!(m.pool.value(), "default");
    assert!(!m.warm);
    assert_eq!(m.limits.max_duration_ms, 30_000);
    assert_eq!(m.limits.max_concurrency, 32);
    assert_eq!(m.limits.wasm_memory_mb, None);

    assert_eq!(m.endpoints.len(), 4);
    let api = &m.endpoints[2];
    assert_eq!(api.path, RoutePattern::parse("/api/*").unwrap());
    assert_eq!(api.auth, EndpointAuth::Platform);
    assert_eq!(api.methods, [HttpMethod::Get, HttpMethod::Post]);
    assert_eq!(
        api.cors,
        Some(Cors {
            origins: vec!["https://app.acme.com".into()],
            methods: vec![],
            headers: vec![],
            allow_credentials: false,
        })
    );
    assert_eq!(api.max_body_bytes, 1_048_576);
    assert_eq!(api.timeout_ms, 10_000);
    assert_eq!(m.endpoints[0].auth, EndpointAuth::Webhook);
    assert_eq!(m.endpoints[3].auth, EndpointAuth::None);

    let sub = &m.subscriptions[0];
    assert_eq!(sub.event_type, "billing:invoices:invoice:created");
    assert_eq!(sub.path.value(), "/events/invoice-created");
    assert_eq!(sub.mode, SubscriptionMode::BlockOnError);
    assert_eq!(
        (sub.max_retries, sub.timeout_seconds, sub.data_only),
        (3, 30, false)
    );

    let sched = &m.schedules[0];
    assert_eq!(sched.cron, "0 * * * *");
    assert_eq!(sched.timezone.as_deref(), Some("UTC"));
    assert_eq!(sched.path.value(), "/jobs/hourly");
    assert_eq!(
        sched
            .payload
            .as_ref()
            .and_then(|p| p.get("x"))
            .and_then(JsonNode::fits_int),
        Some(1)
    );

    let public = &m.public_routes[0];
    assert_eq!(public.hostname, Hostname::parse("api.acme.com").unwrap());
    assert_eq!(public.path_prefix.value(), "/");

    assert_eq!(m.config, ["INVOICE_PREFIX"]);
    assert_eq!(m.secrets, ["billing/stripe-key"]);
    assert_eq!(
        m.db,
        [DbRef {
            name: DnsLabel::parse("name", "main").unwrap(),
            secret_ref: "billing/dsn".into(),
            pool_size: 4,
        }]
    );
    assert_eq!(m.http_allow, ["api.stripe.com"]);
}

#[test]
fn to_json_spellings() {
    let json = parse_jvm(FULL_JVM).to_json();
    assert_eq!(json.get("runtime").unwrap().as_str(), Some("jvm"));
    let endpoint = &json.get("endpoints").unwrap().as_array().unwrap()[2];
    assert_eq!(endpoint.get("auth").unwrap().as_str(), Some("platform"));
    let methods = endpoint.get("methods").unwrap().as_array().unwrap();
    assert_eq!(
        (methods[0].as_str(), methods[1].as_str()),
        (Some("GET"), Some("POST"))
    );
    let sub = &json.get("subscriptions").unwrap().as_array().unwrap()[0];
    assert_eq!(sub.get("mode").unwrap().as_str(), Some("BLOCK_ON_ERROR"));
}

#[test]
fn read_stored_of_parse_strict_to_json_round_trips() {
    let original = parse_jvm(FULL_JVM);
    assert_eq!(
        Manifest::read_stored(&original.to_json()).unwrap(),
        original
    );
}

#[test]
fn wasm_manifest_resolves_wasm_memory_mb() {
    let json = r#"{"runtime":"wasm","entrypoint":"handle","limits":{"wasmMemoryMb":32},
        "endpoints":[{"path":"/events/*","auth":"webhook"}],
        "subscriptions":[{"eventType":"billing:invoices:invoice:created","path":"/events/invoice-created"}]}"#;
    let m = parse(json, Runtime::Wasm, &unrestricted()).unwrap();
    assert_eq!(m.limits.wasm_memory_mb, Some(32));
    assert_eq!(Manifest::read_stored(&m.to_json()).unwrap(), m);
}

// ── structural rules ────────────────────────────────────────────────────

#[test]
fn manifest_required_when_absent_or_not_an_object() {
    let err = Manifest::parse_strict(None, Runtime::Jvm, &defaults(), &unrestricted()).unwrap_err();
    assert_eq!(err.code(), "MANIFEST_REQUIRED");
    for json in ["[]", "null", "\"x\""] {
        assert_eq!(reject_jvm(json).0, "MANIFEST_REQUIRED");
    }
}

#[test]
fn unknown_fields_are_named_by_their_full_path() {
    for (json, name) in [
        (
            r#"{"runtime":"jvm","entrypoint":"x","bogus":1,"endpoints":[{"path":"/a","auth":"none"}]}"#
                .to_string(),
            "bogus",
        ),
        (
            r#"{"runtime":"jvm","entrypoint":"x","limits":{"maxConcurency":1},"endpoints":[{"path":"/a","auth":"none"}]}"#
                .to_string(),
            "limits.maxConcurency",
        ),
        (
            r#"{"runtime":"jvm","entrypoint":"x","endpoints":[{"pth":"/a","auth":"none"}]}"#.to_string(),
            "endpoints[0].pth",
        ),
        (
            with_webhook_and_subscription(r#""eventType":"a:b:c","path":"/events/a","bogus":1"#),
            "subscriptions[0].bogus",
        ),
        // There is no `filter`: unknown, not accepted-and-dropped.
        (
            with_webhook_and_subscription(r#""eventType":"a:b:c","path":"/events/a","filter":"x""#),
            "subscriptions[0].filter",
        ),
    ] {
        let (code, message) = reject_jvm(&json);
        assert_eq!(code, "MANIFEST_UNKNOWN_FIELD");
        assert!(message.contains(name), "{message}");
    }
}

#[test]
fn pool_absent_defaults_to_default() {
    assert_eq!(parse_jvm(MINIMAL_JVM).pool.value(), Manifest::DEFAULT_POOL);
}

#[test]
fn limit_over_ceiling_names_limit_value_and_ceiling() {
    // The ceiling (10) is below the default (32) and the value (20) between
    // them, so comparing against the default would wrongly accept it.
    let json = r#"{"runtime":"jvm","entrypoint":"x","limits":{"maxConcurrency":20},"endpoints":[{"path":"/a","auth":"none"}]}"#;
    let (code, message) = parse(json, Runtime::Jvm, &tight()).unwrap_err();
    assert_eq!(code, "LIMIT_OVER_CEILING");
    for part in ["maxConcurrency", "20", "10"] {
        assert!(message.contains(part), "{message}");
    }
}

#[test]
fn absent_limit_frozen_to_min_of_default_and_ceiling() {
    let m = parse(MINIMAL_JVM, Runtime::Jvm, &tight()).unwrap();
    assert_eq!(m.limits.max_concurrency, 10);
    // The clamped value round-trips through the stored row.
    assert_eq!(
        Manifest::read_stored(&m.to_json())
            .unwrap()
            .limits
            .max_concurrency,
        10
    );
}

#[test]
fn route_ambiguous_names_both_patterns() {
    let (code, message) = reject_jvm(
        r#"{"runtime":"jvm","entrypoint":"x","endpoints":[{"path":"/a/{x}","auth":"none"},{"path":"/a/{y}","auth":"none"}]}"#,
    );
    assert_eq!(code, "ROUTE_AMBIGUOUS");
    assert!(message.contains("/a/{x}") && message.contains("/a/{y}"));
}

#[test]
fn subscription_defaults_when_absent() {
    let m = parse_jvm(&with_webhook_and_subscription(
        r#""eventType":"a:b:c","path":"/events/a""#,
    ));
    let sub = &m.subscriptions[0];
    // IMMEDIATE, not the router's NEXT_ON_ERROR default.
    assert_eq!(sub.mode, SubscriptionMode::Immediate);
    assert_eq!((sub.max_retries, sub.timeout_seconds), (3, 30));
    // false, unlike the subscription aggregate's own true default.
    assert!(!sub.data_only);
}

#[test]
fn most_specific_endpoint_decides_which_auth_governs() {
    let (code, _) = reject_jvm(
        r#"{"runtime":"jvm","entrypoint":"x",
            "endpoints":[{"path":"/events/*","auth":"webhook"},{"path":"/events/special","auth":"platform"}],
            "subscriptions":[{"eventType":"a:b:c","path":"/events/special"}]}"#,
    );
    assert_eq!(code, "SUBSCRIPTION_PATH_NOT_WEBHOOK");
}

// ── alias prefixes ──────────────────────────────────────────────────────

#[test]
fn alias_prefixes() {
    let m = parse_jvm(&with_public(r#""hostname":"api.acme.com""#));
    assert!(m.public_routes[0].alias_prefixes.is_empty());
    let json = m.to_json();
    let route = &json.get("public").unwrap().as_array().unwrap()[0];
    assert!(route.get("aliasPrefixes").is_none(), "omitted when empty");

    let m = parse_jvm(&with_public(
        r#""hostname":"api.acme.com","aliasPrefixes":["qa","staging"]"#,
    ));
    assert_eq!(m.public_routes[0].alias_prefixes, ["qa", "staging"]);
    assert_eq!(Manifest::read_stored(&m.to_json()).unwrap(), m);

    let (code, message) = reject_jvm(&with_public(
        r#""hostname":"api.acme.com","aliasPrefixes":["qa","QA"]"#,
    ));
    assert_eq!(code, "PUBLIC_ROUTE_INVALID");
    assert!(message.contains("public[0].aliasPrefixes[1]"), "{message}");
}

// ── read_stored ─────────────────────────────────────────────────────────

/// Java 892c711b: what the tolerant reader drops (or reads with a fallback)
/// is reported, so the caller can log it; the rest still reads, and a
/// well-formed manifest reports nothing.
#[test]
fn read_stored_reports_every_part_it_drops() {
    let stored = tree(
        r#"{"runtime":"wasm","entrypoint":"handle","pool":"p",
            "endpoints":[{"path":"/ok","auth":"webhook"},{"path":"no-leading-slash","auth":"none"}],
            "subscriptions":[
              {"eventType":"a:b:c:d","path":"/ok","mode":"SOMETIMES"},
              {"eventType":"a:b:c:e","path":"/elsewhere"}],
            "schedules":[{"cron":"0 * * * * *"}],
            "public":[{"hostname":"not a host"}],
            "db":[{"name":"main","secretRef":"DB_URL"},{"name":"Bad Name","secretRef":"DB_URL"}]}"#,
    );
    let (m, dropped) = Manifest::read_stored_reporting(&stored).unwrap();
    assert_eq!(m.endpoints.len(), 1);
    assert_eq!(m.subscriptions.len(), 1);
    assert_eq!(m.subscriptions[0].mode, SubscriptionMode::Immediate);
    assert_eq!(m.db.len(), 1);
    let parts: Vec<(&str, &str)> = dropped.iter().map(|d| (d.part, d.entry.as_str())).collect();
    assert_eq!(
        parts,
        [
            ("endpoint", r#"{"path":"no-leading-slash","auth":"none"}"#),
            (
                "subscription mode (fell back to IMMEDIATE)",
                r#""SOMETIMES""#
            ),
            (
                "subscription",
                r#"{"eventType":"a:b:c:e","path":"/elsewhere"}"#
            ),
            ("schedule", r#"{"cron":"0 * * * * *"}"#),
            ("public route", r#"{"hostname":"not a host"}"#),
            ("db", r#"{"name":"Bad Name","secretRef":"DB_URL"}"#),
        ]
    );
    assert_eq!(Manifest::read_stored(&stored).unwrap(), m);

    let (_, none) = Manifest::read_stored_reporting(&m.to_json()).unwrap();
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn a_dropped_parts_entry_is_capped() {
    let long = "x".repeat(400);
    let stored = tree(&format!(
        r#"{{"runtime":"wasm","entrypoint":"handle","endpoints":[{{"path":"{long}"}}]}}"#
    ));
    let (_, dropped) = Manifest::read_stored_reporting(&stored).unwrap();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].entry.chars().count(), 301);
    assert!(dropped[0].entry.ends_with('…'));
}

#[test]
fn read_stored_fails_only_when_runtime_or_entrypoint_is_unreadable() {
    for json in [
        "{}",
        r#"{"runtime":"cobol","entrypoint":"x"}"#,
        r#"{"runtime":"jvm"}"#,
        r#"{"runtime":"jvm","entrypoint":"  "}"#,
        "[]",
    ] {
        assert!(Manifest::read_stored(&tree(json)).is_err(), "{json}");
    }
}

#[test]
fn read_stored_tolerates_unknown_keys_and_missing_optionals() {
    let m = Manifest::read_stored(&tree(
        r#"{"runtime":"jvm","entrypoint":"x","bogus":1,
            "limits":{"maxDurationMs":30000,"maxConcurrency":32,"extra":true},
            "endpoints":[{"path":"/a","auth":"none","weird":true}]}"#,
    ))
    .unwrap();
    assert_eq!(m.runtime, Runtime::Jvm);
    assert_eq!(m.endpoints.len(), 1);

    let m = Manifest::read_stored(&tree(r#"{"runtime":"jvm","entrypoint":"x"}"#)).unwrap();
    assert_eq!(m.pool.value(), "default");
    assert!(!m.warm);
    assert_eq!(
        m.limits.max_duration_ms,
        FunctionLimits::DEFAULT_MAX_DURATION_MS
    );
    assert_eq!(m.limits.wasm_memory_mb, None);
    assert!(m.endpoints.is_empty() && m.subscriptions.is_empty() && m.schedules.is_empty());
    assert!(m.public_routes.is_empty() && m.config.is_empty() && m.secrets.is_empty());
    assert!(m.db.is_empty() && m.http_allow.is_empty());
}

#[test]
fn read_stored_falls_back_and_drops_rather_than_failing() {
    let m = Manifest::read_stored(&tree(
        r#"{"runtime":"jvm","entrypoint":"x","limits":{"maxDurationMs":5000000000}}"#,
    ))
    .unwrap();
    assert_eq!(
        m.limits.max_duration_ms,
        FunctionLimits::DEFAULT_MAX_DURATION_MS
    );

    let m = Manifest::read_stored(&tree(
        r#"{"runtime":"jvm","entrypoint":"x","endpoints":[{"path":"not-a-path","auth":"none"},{"path":"/a","auth":"none"}]}"#,
    ))
    .unwrap();
    assert_eq!(m.endpoints.len(), 1);
    assert_eq!(m.endpoints[0].path.value(), "/a");

    let m = Manifest::read_stored(&tree(
        r#"{"runtime":"jvm","entrypoint":"x","endpoints":[{"path":"/events/*","auth":"platform"}],
            "subscriptions":[{"eventType":"a:b:c","path":"/events/a"}]}"#,
    ))
    .unwrap();
    assert!(m.subscriptions.is_empty());
}

#[test]
fn peek_stored_pool_reads_pool_alone() {
    let corrupt = tree(r#"{"runtime":"cobol","pool":"orders"}"#);
    assert!(Manifest::read_stored(&corrupt).is_err());
    assert_eq!(
        Manifest::peek_stored_pool(&corrupt).unwrap().value(),
        "orders"
    );
    assert_eq!(
        Manifest::peek_stored_pool(&tree(r#"{"pool":"Bad"}"#))
            .unwrap()
            .value(),
        "default"
    );
    assert!(Manifest::peek_stored_pool(&tree("[]")).is_none());
}

// ── check: every independent problem ───────────────────────────────────

const SIX_MISTAKES: &str = r#"{"x":1,"runtime":"jvm","entrypoint":"com.acme.Fn","pool":"Bad Pool",
 "endpoints":[
    {"path":"/events/*","auth":"webhook"},
    {"path":"/foo"},
    {"path":"/other","auth":"none","cors":{"origins":["https://app.acme.com/callback"]}}
 ],
 "subscriptions":[
    {"eventType":"billing:invoice:created","path":"/events/created"},
    {"eventType":"billing:invoice:created","path":"/events/created2"}
 ],
 "db":[{"name":"Bad Name","secretRef":"billing/dsn"}]}"#;

#[test]
fn six_independent_mistakes_are_all_reported_in_order_with_pointers() {
    let rejected = Manifest::check(
        Some(&tree(SIX_MISTAKES)),
        Runtime::Jvm,
        &defaults(),
        &unrestricted(),
    )
    .unwrap_err();
    let got: Vec<(&str, &str)> = rejected
        .problems()
        .iter()
        .map(|p| (p.code, p.pointer.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("MANIFEST_UNKNOWN_FIELD", "/x"),
            ("POOL_INVALID", "/pool"),
            ("ENDPOINT_AUTH_REQUIRED", "/endpoints/1/auth"),
            ("ENDPOINT_INVALID", "/endpoints/2/cors/origins/0"),
            ("SUBSCRIPTION_DUPLICATE", "/subscriptions/1"),
            ("DB_INVALID", "/db/0/name"),
        ]
    );
    // Publish rejects with only the first.
    assert_eq!(reject_jvm(SIX_MISTAKES).0, "MANIFEST_UNKNOWN_FIELD");
    // The check route's entry carries the pointer (the platform's details).
    let entry = rejected.problems()[3].to_validation_error();
    assert_eq!(entry.code(), "ENDPOINT_INVALID");
    assert_eq!(entry.pointer(), Some("/endpoints/2/cors/origins/0"));
}

/// A subscription whose path matches only an endpoint that failed for its
/// own reason is not reported again: one problem, not two.
#[test]
fn a_cascade_is_not_reported() {
    let rejected = Manifest::check(
        Some(&tree(
            r#"{"runtime":"jvm","entrypoint":"x",
                "endpoints":[{"path":"/events/created","auth":"bearer"}],
                "subscriptions":[{"eventType":"a:b:c","path":"/events/created"}]}"#,
        )),
        Runtime::Jvm,
        &defaults(),
        &unrestricted(),
    )
    .unwrap_err();
    assert_eq!(rejected.problems().len(), 1);
    assert_eq!(rejected.problems()[0].code, "ENDPOINT_INVALID");
    assert_eq!(rejected.problems()[0].pointer, "/endpoints/0/auth");
}

#[test]
fn check_text_parses_then_checks() {
    let result = Manifest::check_text(MINIMAL_JVM, Runtime::Jvm, &defaults(), &unrestricted());
    assert!(result.unwrap().is_ok());
    assert!(Manifest::check_text("{", Runtime::Jvm, &defaults(), &unrestricted()).is_err());
}

// ── $schema (FunctionManifestSchemaTest) ────────────────────────────────

const MINIMAL_WITH_SCHEMA: &str = r#"{
  "$schema": "https://example.test/function-manifest.schema.json",
  "runtime": "jvm",
  "entrypoint": "com.acme.billing.CreateInvoice",
  "endpoints": [ { "path": "/events/invoice-created", "auth": "webhook" } ],
  "subscriptions": [ { "eventType": "billing:invoices:invoice:created", "path": "/events/invoice-created" } ]
}"#;

#[test]
fn schema_field_is_accepted_type_checked_and_never_written_back() {
    let m = parse_jvm(MINIMAL_WITH_SCHEMA);
    assert_eq!(m.entrypoint, "com.acme.billing.CreateInvoice");
    let written = m.to_json();
    assert!(written.get("$schema").is_none());
    assert_eq!(Manifest::read_stored(&written).unwrap(), m);

    let (code, _) = reject_jvm(
        r#"{"$schema": 42, "runtime": "jvm", "entrypoint": "com.acme.billing.CreateInvoice"}"#,
    );
    assert_eq!(code, "MANIFEST_INVALID");
}

// ── the function host's reading (formerly fc-fnhost-core's own reader) ──

fn stored(json: &str) -> Manifest {
    Manifest::read_stored(&tree(json)).unwrap()
}

#[test]
fn read_stored_endpoints_as_the_host_reads_them() {
    let m = stored(
        r#"{"runtime":"wasm","entrypoint":"handle","endpoints":[
            {"path":"/events/*","auth":"WEBHOOK","methods":["GET"]},
            {"path":"/api/{id}","auth":"platform","methods":["get","bogus",3],
             "cors":{"origins":["https://a.test"," "],"allowCredentials":true},
             "maxBodyBytes":10,"timeoutMs":200},
            {"path":"no-slash","auth":"none"},
            {"path":"/x","auth":"sometimes"},
            {"path":"/y"},
            "not an object",
            {"path":"/z","auth":"none","maxBodyBytes":0,"timeoutMs":1.5}
        ]}"#,
    );
    let endpoints = &m.endpoints;
    assert_eq!(endpoints.len(), 3);
    assert_eq!(endpoints[0].auth, EndpointAuth::Webhook);
    // A webhook endpoint is POST whatever is stored.
    assert_eq!(endpoints[0].methods, [HttpMethod::Get]);
    assert_eq!(endpoints[0].effective_methods(), [HttpMethod::Post]);
    assert_eq!(endpoints[1].methods, [HttpMethod::Get]);
    assert_eq!(endpoints[1].effective_methods(), [HttpMethod::Get]);
    let cors = endpoints[1].cors.as_ref().unwrap();
    assert_eq!(cors.origins, ["https://a.test"]);
    assert!(cors.allow_credentials);
    assert_eq!(endpoints[1].max_body_bytes, 10);
    assert_eq!(endpoints[1].timeout_ms, 200);
    assert_eq!(endpoints[2].path.value(), "/z");
    assert!(endpoints[2].effective_methods().is_empty());
    assert_eq!(
        endpoints[2].max_body_bytes,
        fc_function_model::Endpoint::DEFAULT_MAX_BODY_BYTES
    );
    assert_eq!(
        endpoints[2].timeout_ms,
        FunctionLimits::DEFAULT_MAX_DURATION_MS
    );
    assert!(m.has_webhook_endpoint());
    assert!(!stored(r#"{"runtime":"wasm","entrypoint":"handle"}"#).has_webhook_endpoint());
}

#[test]
fn read_stored_limits_default_per_field() {
    let m = stored(
        r#"{"runtime":"wasm","entrypoint":"handle",
            "limits":{"maxConcurrency":3,"maxDurationMs":-1}}"#,
    );
    assert_eq!(m.limits.max_concurrency, 3);
    assert_eq!(
        m.limits.max_duration_ms,
        FunctionLimits::DEFAULT_MAX_DURATION_MS
    );
    assert_eq!(
        m.limits.wasm_memory_mb,
        Some(FunctionLimits::DEFAULT_WASM_MEMORY_MB)
    );
    let jvm = stored(r#"{"runtime":"jvm","entrypoint":"a.B"}"#);
    assert_eq!(jvm.limits.wasm_memory_mb, None);
}

// ── runtime: component (owner decision 5, beyond Java) ──────────────────

#[test]
fn a_component_manifest_defaults_its_entrypoint() {
    let m = parse(
        r#"{"runtime": "component"}"#,
        Runtime::Component,
        &unrestricted(),
    )
    .unwrap();
    assert_eq!(m.runtime, Runtime::Component);
    assert_eq!(m.entrypoint, "wasi:http/incoming-handler");
    assert_eq!(
        m.limits.wasm_memory_mb,
        Some(FunctionLimits::DEFAULT_WASM_MEMORY_MB)
    );
    // Stored, it reads back the same, and so does one stored without it.
    assert_eq!(Manifest::read_stored(&m.to_json()).unwrap(), m);
    assert_eq!(
        Manifest::read_stored(&tree(r#"{"runtime": "component"}"#))
            .unwrap()
            .entrypoint,
        "wasi:http/incoming-handler"
    );
    for entrypoint in [
        "wasi:http/incoming-handler",
        "wasi:http/incoming-handler@0.2.3",
        "wasi_http_incoming_handler",
    ] {
        let json = format!(r#"{{"runtime": "component", "entrypoint": "{entrypoint}"}}"#);
        let m = parse(&json, Runtime::Component, &unrestricted()).unwrap();
        assert_eq!(m.entrypoint, entrypoint);
    }
    let (code, _) = parse(
        r#"{"runtime": "component", "entrypoint": "handle"}"#,
        Runtime::Component,
        &unrestricted(),
    )
    .unwrap_err();
    assert_eq!(code, "ENTRYPOINT_INVALID");
}

#[test]
fn wasm_and_component_functions_take_each_others_manifests_not_jvm() {
    let component = r#"{"runtime": "component"}"#;
    let wasm = r#"{"runtime": "wasm", "entrypoint": "wasi_http_incoming_handler"}"#;
    assert_eq!(
        parse(component, Runtime::Wasm, &unrestricted())
            .unwrap()
            .runtime,
        Runtime::Component
    );
    assert_eq!(
        parse(wasm, Runtime::Component, &unrestricted())
            .unwrap()
            .runtime,
        Runtime::Wasm
    );
    let (code, message) = parse(component, Runtime::Jvm, &unrestricted()).unwrap_err();
    assert_eq!(code, "RUNTIME_MISMATCH");
    assert_eq!(
        message,
        "manifest runtime 'component' does not match the function's runtime 'jvm'"
    );
    // A wasm manifest still needs its entrypoint: only a component has a default.
    let (code, _) = parse(r#"{"runtime": "wasm"}"#, Runtime::Wasm, &unrestricted()).unwrap_err();
    assert_eq!(code, "ENTRYPOINT_REQUIRED");
}

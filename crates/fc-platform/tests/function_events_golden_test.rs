//! Agreement with Java's own events. `tests/data/function/events-golden.json`
//! was written by running Java's `FunctionEvents.*.of(...)` at `0118cdca`
//! (`tests/java/io/flowcatalyst/platform/function/operations/FunctionEventsGoldenGen.java`)
//! over fixed aggregates. The Rust events built from the same aggregates
//! must carry the same type, source, spec version, subject and message
//! group, and serialise to byte-identical `data`.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

use fc_platform::function::entity::{
    ClientPolicy, Function, FunctionDomain, FunctionStatus, FunctionVersion, SecretValue,
    SignerIdentity, SignerRule,
};
use fc_platform::function::operations::events::{
    ConfigUpdated, DomainClaimed, DomainReleased, FunctionCreated, FunctionDeleted,
    FunctionUpdated, PolicyUpdated, SecretDeleted, SecretSet, VersionPublished, VersionRetired,
};
use fc_platform::function::{
    ClientCeilings, Digest, FunctionAddress, FunctionLimits, FunctionOwner, Hostname, JsonNode,
    Manifest, Runtime,
};
use fc_platform::usecase::{DomainEvent, EventMetadata, ExecutionContext};

fn golden() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/function/events-golden.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn at() -> DateTime<Utc> {
    "2026-09-24T10:00:00.123456Z".parse().unwrap()
}

fn function(
    id: &str,
    owner: FunctionOwner,
    runtime: Runtime,
    description: Option<&str>,
    status: FunctionStatus,
) -> Function {
    Function {
        id: id.into(),
        application_id: "app_1".into(),
        address: FunctionAddress::parse("billing.invoices.create").unwrap(),
        owner,
        runtime,
        description: description.map(str::to_string),
        status,
        aliases: vec![],
        created_at: at(),
        updated_at: at(),
    }
}

fn policy(
    owner: FunctionOwner,
    signers: Vec<SignerRule>,
    max_duration_ms: Option<i32>,
) -> ClientPolicy {
    ClientPolicy {
        owner,
        signers,
        max_duration_ms,
        max_concurrency: None,
        max_wasm_memory_mb: None,
        max_db_pool_size: None,
        created_at: at(),
        updated_at: at(),
    }
}

/// A version as Java's generator builds it (`version-*` cases).
fn version(
    id: &str,
    function_id: &str,
    number: i32,
    signer: Option<SignerIdentity>,
) -> FunctionVersion {
    let defaults = FunctionLimits::defaults();
    let manifest = Manifest::parse_strict(
        Some(
            &JsonNode::parse(r#"{"runtime":"wasm","entrypoint":"handle","pool":"edge"}"#).unwrap(),
        ),
        Runtime::Wasm,
        &defaults,
        &ClientCeilings::of(&defaults),
    )
    .unwrap();
    let mut v = FunctionVersion::publish(
        function_id,
        number,
        "oci://r/a",
        Digest::parse(&format!("sha256:{}", "b".repeat(64))).unwrap(),
        None,
        signer,
        manifest,
        "prn_1",
        at(),
    );
    v.id = id.into();
    v
}

fn domain(id: &str, owner: FunctionOwner, hostname: &str) -> FunctionDomain {
    FunctionDomain {
        id: id.into(),
        owner,
        hostname: Hostname::parse(hostname).unwrap(),
        created_at: at(),
    }
}

/// An event's envelope and its `data` as serialised.
type Built = (EventMetadata, String);
/// Builds one golden case's Rust event.
type Builder = Box<dyn Fn() -> Built>;

/// `(case, builder)` for every event the golden lists.
fn rust_events() -> Vec<(&'static str, Builder)> {
    fn pack<E: DomainEvent>(e: E) -> Built {
        (e.metadata().clone(), serde_json::to_string(&e).unwrap())
    }
    let ctx = || ExecutionContext::create("prn_1");
    let platform_fn = || {
        function(
            "fnc_1",
            FunctionOwner::Platform,
            Runtime::Wasm,
            None,
            FunctionStatus::Active,
        )
    };
    let client_fn = || {
        function(
            "fnc_2",
            FunctionOwner::Client("clt_1".into()),
            Runtime::Jvm,
            Some("Creates invoices"),
            FunctionStatus::Disabled,
        )
    };
    let platform_policy = || {
        policy(
            FunctionOwner::Platform,
            vec![
                SignerRule::new("https://issuer", "a", [Runtime::Jvm]),
                SignerRule::new("https://issuer", "b", [Runtime::Wasm]),
            ],
            Some(9000),
        )
    };
    let client_policy = || policy(FunctionOwner::Client("clt_1".into()), vec![], None);
    let platform_domain = || domain("fnd_1", FunctionOwner::Platform, "acme.com");
    let client_domain = || {
        domain(
            "fnd_2",
            FunctionOwner::Client("clt_1".into()),
            "api.example.org",
        )
    };
    vec![
        (
            "created/platform",
            Box::new(move || pack(FunctionCreated::new(&ctx(), &platform_fn()))),
        ),
        (
            "created/client",
            Box::new(move || pack(FunctionCreated::new(&ctx(), &client_fn()))),
        ),
        (
            "updated/platform",
            Box::new(move || pack(FunctionUpdated::new(&ctx(), &platform_fn()))),
        ),
        (
            "updated/client",
            Box::new(move || pack(FunctionUpdated::new(&ctx(), &client_fn()))),
        ),
        (
            "deleted/client",
            Box::new(move || pack(FunctionDeleted::new(&ctx(), &client_fn()))),
        ),
        (
            "config/client",
            Box::new(move || {
                pack(ConfigUpdated::new(
                    &ctx(),
                    &client_fn(),
                    vec!["A".into(), "B".into()],
                ))
            }),
        ),
        (
            "config/empty",
            Box::new(move || pack(ConfigUpdated::new(&ctx(), &client_fn(), vec![]))),
        ),
        (
            "secret-set/client",
            Box::new(move || pack(SecretSet::new(&ctx(), &client_fn(), "API_KEY"))),
        ),
        (
            "secret-deleted/client",
            Box::new(move || pack(SecretDeleted::new(&ctx(), &client_fn(), "API_KEY"))),
        ),
        (
            "policy/platform",
            Box::new(move || pack(PolicyUpdated::new(&ctx(), &platform_policy()))),
        ),
        (
            "policy/client",
            Box::new(move || pack(PolicyUpdated::new(&ctx(), &client_policy()))),
        ),
        (
            "domain-claimed/platform",
            Box::new(move || pack(DomainClaimed::new(&ctx(), &platform_domain()))),
        ),
        (
            "domain-claimed/client",
            Box::new(move || pack(DomainClaimed::new(&ctx(), &client_domain()))),
        ),
        (
            "domain-released/client",
            Box::new(move || pack(DomainReleased::new(&ctx(), &client_domain()))),
        ),
        (
            "version-published/signed",
            Box::new(move || {
                let signer = SignerIdentity {
                    issuer: "https://issuer".into(),
                    subject: "repo:acme/fn".into(),
                };
                let v = version("fnv_3", "fnc_1", 3, Some(signer));
                pack(VersionPublished::new(&ctx(), &platform_fn(), &v))
            }),
        ),
        (
            "version-published/unsigned",
            Box::new(move || {
                let v = version("fnv_4", "fnc_2", 4, None);
                pack(VersionPublished::new(&ctx(), &client_fn(), &v))
            }),
        ),
        (
            "version-retired/client",
            Box::new(move || {
                let v = version("fnv_4", "fnc_2", 4, None);
                pack(VersionRetired::new(&ctx(), &client_fn(), &v))
            }),
        ),
    ]
}

#[test]
fn every_event_matches_javas_envelope_and_data_bytes() {
    let golden = golden();
    let expected = golden["events"].as_array().unwrap();
    let actual = rust_events();
    assert_eq!(
        actual.len(),
        expected.len(),
        "every golden case has a Rust event"
    );
    for (want, (case, build)) in expected.iter().zip(actual) {
        assert_eq!(want["case"], case);
        let (meta, data) = build();
        assert_eq!(
            meta.event_type,
            want["type"].as_str().unwrap(),
            "{case}: type"
        );
        assert_eq!(
            meta.source,
            want["source"].as_str().unwrap(),
            "{case}: source"
        );
        assert_eq!(
            meta.spec_version,
            want["specVersion"].as_str().unwrap(),
            "{case}: spec version"
        );
        assert_eq!(
            meta.subject,
            want["subject"].as_str().unwrap(),
            "{case}: subject"
        );
        assert_eq!(
            meta.message_group,
            want["messageGroup"].as_str().unwrap(),
            "{case}: message group"
        );
        assert_eq!(data, want["data"].as_str().unwrap(), "{case}: data bytes");
    }
}

#[test]
fn a_secret_value_serialises_as_java_masks_it() {
    assert_eq!(
        serde_json::to_string(&SecretValue::new("sk_live_MARKER")).unwrap(),
        golden()["secretValue"].as_str().unwrap()
    );
}

//! Audit rows never store passwords or secrets (owner spec
//! `docs/spec/audit-redaction.md` in the Java repo).
//!
//! `tests/fixtures/audit-redaction-vectors.json` is a byte-identical copy of
//! the canonical vectors (fc-common's parity test checks it); every case
//! runs through each place fc-sdk builds an audit payload.

use fc_sdk::outbox::{AuditLogPayload, CreateAuditLogDto};
use fc_sdk::usecase::audit::redact;
use fc_sdk::usecase::{AuditMasked, Audited, EventMetadata};
use serde::Serialize;
use serde_json::{json, Value};

struct Vector {
    name: String,
    input: Value,
    masked: Vec<String>,
    expected: Value,
}

fn vectors() -> Vec<Vector> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/audit-redaction-vectors.json"
    );
    let cases: Vec<Value> =
        serde_json::from_slice(&std::fs::read(path).expect("read vectors")).expect("parse");
    assert!(!cases.is_empty());
    cases
        .into_iter()
        .map(|c| Vector {
            name: c["name"].as_str().unwrap().to_string(),
            masked: c["masked"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m.as_str().unwrap().to_string())
                .collect(),
            input: c["input"].clone(),
            expected: c["expected"].clone(),
        })
        .collect()
}

fn masked(v: &Vector) -> Vec<&str> {
    v.masked.iter().map(String::as_str).collect()
}

#[test]
fn every_vector_through_the_rule() {
    for v in vectors() {
        assert_eq!(redact(&v.input, &masked(&v)), v.expected, "{}", v.name);
    }
}

/// `CreateAuditLogDto` (the simple outbox pattern): the payload's
/// `operationData` string holds the redacted document.
#[test]
fn every_vector_through_create_audit_log_dto() {
    for v in vectors() {
        let payload = CreateAuditLogDto::new("Thing", "thg_1", "UPDATE")
            .operation_data_masked(v.input.clone(), &masked(&v))
            .to_payload();
        let data: Value =
            serde_json::from_str(payload["operationData"].as_str().expect("string")).unwrap();
        assert_eq!(data, v.expected, "{}", v.name);
    }
}

/// `operation_data` (no masks) still gets the name rule.
#[test]
fn create_audit_log_dto_applies_the_name_rule_without_masks() {
    let payload = CreateAuditLogDto::new("Principal", "p_1", "CREATE")
        .operation_data(json!({
            "email": "a@b.c",
            "password": "hunter2",
            "webhookCredentials": {"token": "tok", "signingSecret": "s3"},
        }))
        .to_payload()
        .to_string();
    for secret in ["hunter2", "tok\\\"", "s3\\\""] {
        assert!(!payload.contains(secret), "{secret} in {payload}");
    }
    assert!(payload.contains("a@b.c"));
}

/// `AuditLogPayload` built by hand: the outbox payload is redacted even
/// though the struct was not built through `from_event`.
#[test]
fn every_unmasked_vector_through_a_hand_built_audit_log_payload() {
    for v in vectors().into_iter().filter(|v| v.masked.is_empty()) {
        let audit = AuditLogPayload {
            entity_type: "Thing".into(),
            entity_id: "thg_1".into(),
            operation: "Update".into(),
            operation_json: Some(v.input.clone()),
            principal_id: "prn_1".into(),
            application_id: None,
            client_id: None,
            performed_at: "2026-09-24T00:00:00Z".into(),
            message_group: None,
        };
        let payload = audit.to_outbox_payload().unwrap();
        assert_eq!(payload["operation_json"], v.expected, "{}", v.name);
    }
}

#[derive(Serialize)]
struct TestEvent {
    metadata: EventMetadata,
}

fc_sdk::impl_domain_event!(TestEvent);

fn event() -> TestEvent {
    TestEvent {
        metadata: EventMetadata {
            event_id: "evt_1".into(),
            event_type: "shop:config:property:set".into(),
            spec_version: "1.0".into(),
            source: "shop:config".into(),
            subject: "config.property.cfg_1".into(),
            time: chrono::Utc::now(),
            execution_id: "exec-1".into(),
            correlation_id: "corr-1".into(),
            causation_id: None,
            principal_id: "prn_user".into(),
            message_group: "config:property:cfg_1".into(),
        },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetPropertyCommand {
    property: String,
    value: String,
    value_type: String,
    api_key: String,
}

impl AuditMasked for SetPropertyCommand {
    fn audit_masked_fields(&self) -> &'static [&'static str] {
        if self.value_type == "PLAIN" {
            &[]
        } else {
            &["value"]
        }
    }
}

fn set_property(value_type: &str) -> SetPropertyCommand {
    SetPropertyCommand {
        property: "stripe".into(),
        value: "sk_live_123".into(),
        value_type: value_type.into(),
        api_key: "k".into(),
    }
}

/// `AuditLogPayload::from_event`: the name rule always; the command's
/// declared masks when it is passed as `Audited(&cmd)`, with the operation
/// still the command's own name.
#[test]
fn audit_log_payload_from_event_redacts_and_honours_declared_masks() {
    let bare = AuditLogPayload::from_event(&event(), &set_property("SECRET"));
    assert_eq!(bare.operation, "SetPropertyCommand");
    assert_eq!(
        bare.operation_json.unwrap(),
        json!({"property": "stripe", "value": "sk_live_123", "valueType": "SECRET", "apiKey": "***"})
    );

    let audited = AuditLogPayload::from_event(&event(), &Audited(&set_property("SECRET")));
    assert_eq!(audited.operation, "SetPropertyCommand");
    assert_eq!(
        audited.operation_json.unwrap(),
        json!({"property": "stripe", "value": "***", "valueType": "SECRET", "apiKey": "***"})
    );

    let plain = AuditLogPayload::from_event(&event(), &Audited(&set_property("PLAIN")));
    assert_eq!(plain.operation_json.unwrap()["value"], "sk_live_123");
}

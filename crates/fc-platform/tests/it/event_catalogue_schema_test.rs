//! The seeded 1.0 schemas describe what the platform actually emits: real
//! events, serialized as the unit of work persists them (`msg_events.data`
//! is the event's `Serialize` output), validate against the catalogue
//! schema of their type. Covers the 33 codes catalogued on 2026-10-11
//! (docs/go-mirror/2026-10-11-event-catalogue.md), each with its optional
//! fields both present and absent where it has any; the function events
//! come from Java's golden data, which the Rust events match
//! (`function_events_golden_test`).
//!
//! Also covers every code whose older seeded schema migration 070 replaces
//! (Go's F9 follow-up, `4c23adf`): the corrected schema must admit what the
//! event really sends, and the test fails on a code 070 lists without a
//! sample here.
//!
//! The validator knows the subset of draft-07 the catalogue's builders
//! write: `type` (one or a list), `properties`, `required`,
//! `additionalProperties: false`, `items`, `minimum`, `enum`, and
//! `format: date-time` (RFC 3339).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};

use fc_platform::domain::UserScope;
use fc_platform::seed::platform_event_schemas::schema_for;
use fc_platform::seed::platform_event_types;
use fc_platform::shared::id::{
    ApplicationId, ClientId, ConnectionId, DispatchPoolId, EmailDomainMappingId, EventTypeId,
    FunctionId, FunctionVersionId, IdentityProviderId, OAuthClientId, PermissionId, PortalAppId,
    PortalIdentityId, PrincipalId, SubscriptionId,
};
use fc_platform::usecase::{DomainEvent, EventMetadata, ExecutionContext};
use fc_platform_auth::portal::operations::events::{
    IdentityAppGranted, IdentityAppRevoked, IdentityDeleted, IdentityEnsured, IdentityStatusSet,
    PortalAppChanged, APP_CREATED, APP_DELETED, APP_UPDATED,
};
use fc_platform_functions::function::operations::events::{
    AliasChanged, AliasRemoved, VersionReady,
};
use fc_platform_iam::app_docs::operations::sync::AppDocsSynced;
use fc_platform_iam::application::operations::{
    ApplicationActivated, ApplicationCreated, ApplicationDeactivated, ApplicationDeleted,
    ApplicationUpdated, ClientApplicationsUpdated,
};
use fc_platform_iam::audit::operations::events::AuditLogsRedacted;
use fc_platform_iam::auth::operations::OAuthClientSecretRotated;
use fc_platform_iam::developer_credential::events::{
    DeveloperCredentialRevoked, DeveloperCredentialSet,
};
use fc_platform_iam::email_domain_mapping::operations::move_provider::EmailDomainMappingProviderChanged;
use fc_platform_iam::portal::entity::IdentityStatus;
use fc_platform_iam::portal::PortalApp;
use fc_platform_iam::principal::operations::{
    FederatedClaims, FlowcatalystClaims, PasswordResetCompleted, UserCreated, UserDeactivated,
    UserDeleted, UserLoggedIn, UserUpdated,
};
use fc_platform_iam::role::operations::RolesSynced;
use fc_platform_iam::role::operations::{PermissionDefined, PermissionDeleted};
use fc_platform_iam::service_account::entity::ServiceAccount;
use fc_platform_iam::service_account::operations::mint_token::ServiceAccountTokenMinted;
use fc_platform_iam::service_account::operations::{ServiceAccountCreated, ServiceAccountUpdated};
use fc_platform_messaging::connection::operations::{
    ConnectionCreated, ConnectionDeleted, ConnectionUpdated,
};
use fc_platform_messaging::dispatch_job_actions::operations::{
    DispatchJobPayloadEdited, DispatchJobSettled, DispatchJobsResent, CANCELLED_EVENT,
    COMPLETED_EVENT, PAYLOAD_EDITED_EVENT,
};
use fc_platform_messaging::dispatch_pool::operations::{
    DispatchPoolArchived, DispatchPoolCreated, DispatchPoolDeleted, DispatchPoolUpdated,
};
use fc_platform_messaging::event_type::operations::{
    EventTypeUpdated, SchemaAdded, SchemaDeprecated, SchemaFinalised,
};
use fc_platform_messaging::subscription::operations::{
    SubscriptionCreated, SubscriptionPaused, SubscriptionResumed, SubscriptionUpdated,
    SubscriptionsSynced,
};

/// Errors of `value` against `schema`, at `path`.
fn validate(schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => vec![],
        };
        let ok = types.iter().any(|t| match *t {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "null" => value.is_null(),
            _ => false,
        });
        if !ok {
            errors.push(format!("{path}: {value} is not {types:?}"));
            return;
        }
    }
    if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
        if value.as_f64().is_some_and(|n| n < min) {
            errors.push(format!("{path}: {value} < {min}"));
        }
    }
    if schema.get("format").and_then(Value::as_str) == Some("date-time") {
        if let Some(s) = value.as_str() {
            if DateTime::parse_from_rfc3339(s).is_err() {
                errors.push(format!("{path}: {value} is not an RFC 3339 date-time"));
            }
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            errors.push(format!("{path}: {value} not in {allowed:?}"));
        }
    }
    if let (Some(items), Some(arr)) = (schema.get("items"), value.as_array()) {
        for (i, v) in arr.iter().enumerate() {
            validate(items, v, &format!("{path}[{i}]"), errors);
        }
    }
    if let Some(obj) = value.as_object() {
        let props = schema.get("properties").and_then(Value::as_object);
        for req in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !obj.contains_key(req) {
                errors.push(format!("{path}: missing required {req}"));
            }
        }
        for (k, v) in obj {
            match props.and_then(|p| p.get(k)) {
                Some(s) => validate(s, v, &format!("{path}.{k}"), errors),
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    errors.push(format!("{path}: unexpected property {k}"));
                }
                None => {}
            }
        }
    }
}

fn errors_for(code: &str, data: &Value) -> Vec<String> {
    let Some(schema) = schema_for(code) else {
        return vec![format!("{code}: no catalogue schema")];
    };
    let mut errors = Vec::new();
    validate(&schema, data, code, &mut errors);
    errors
}

/// `(type, data)` as the unit of work would persist the event.
fn emitted<E: DomainEvent>(e: &E) -> (String, Value) {
    (
        e.metadata().event_type.clone(),
        serde_json::to_value(e).unwrap(),
    )
}

/// The struct's metadata with only the type set (for events built by
/// literal, as their use cases build them).
fn meta(event_type: &str) -> EventMetadata {
    EventMetadata {
        event_type: event_type.to_string(),
        ..EventMetadata::default()
    }
}

fn data_of<E: Serialize>(e: &E) -> Value {
    serde_json::to_value(e).unwrap()
}

fn samples() -> Vec<(String, Value)> {
    let ctx = ExecutionContext::system("prn_1");
    let user = PrincipalId::from_wire("prn_2");
    let identity = PortalIdentityId::from_wire("ptu_1");
    let client = ClientId::from_wire("clt_1");
    let app_id = PortalAppId::from_wire("pta_1");
    let app = PortalApp::new(&client, "Suppliers", "Suppliers Portal");
    let mut ensured_with_app =
        IdentityEnsured::new(&ctx, &identity, &client, "a@b.c", false, "SDK");
    ensured_with_app.portal_app_id = Some(app_id.clone());
    ensured_with_app.portal_app_code = Some("suppliers".into());

    let mut out = vec![
        emitted(&DeveloperCredentialSet::new(&ctx, &user)),
        emitted(&DeveloperCredentialRevoked::new(&ctx, &user)),
        emitted(&EmailDomainMappingProviderChanged {
            metadata: meta(EmailDomainMappingProviderChanged::EVENT_TYPE),
            mapping_id: EmailDomainMappingId::from_wire("edm_1"),
            email_domain: "acme.com".into(),
            from_identity_provider_id: IdentityProviderId::from_wire("idp_1"),
            to_identity_provider_id: IdentityProviderId::from_wire("idp_2"),
            users_reset: 3,
        }),
        emitted(&DispatchJobSettled {
            metadata: meta(CANCELLED_EVENT),
            dispatch_job_id: "dj_1".into(),
        }),
        emitted(&DispatchJobSettled {
            metadata: meta(COMPLETED_EVENT),
            dispatch_job_id: "dj_1".into(),
        }),
        emitted(&DispatchJobPayloadEdited {
            metadata: meta(PAYLOAD_EDITED_EVENT),
            dispatch_job_id: "dj_1".into(),
            status: "FAILED".into(),
            previous_size: 12,
            previous_sha256: "a".repeat(64),
            size: 20,
            sha256: "b".repeat(64),
            previous_content_type: "application/json".into(),
            content_type: "application/xml".into(),
        }),
        emitted(&DispatchJobsResent {
            metadata: meta(DispatchJobsResent::EVENT_TYPE),
            ids: vec!["dj_1".into(), "dj_2".into()],
            redrive: true,
            redriven_count: 1,
            redriven_ids: vec!["dj_2".into()],
            skipped_in_flight: 2,
            skipped_completed: 0,
            skipped: Vec::new(),
        }),
        // A run that redrove nothing leaves `redrivenIds` out.
        emitted(&DispatchJobsResent {
            metadata: meta(DispatchJobsResent::EVENT_TYPE),
            ids: vec!["dj_1".into()],
            redrive: false,
            redriven_count: 0,
            redriven_ids: Vec::new(),
            skipped_in_flight: 0,
            skipped_completed: 3,
            skipped: Vec::new(),
        }),
        emitted(&PortalAppChanged::new(&ctx, APP_CREATED, &app)),
        emitted(&PortalAppChanged::new(&ctx, APP_UPDATED, &app)),
        emitted(&PortalAppChanged::new(&ctx, APP_DELETED, &app)),
        emitted(&IdentityEnsured::new(
            &ctx, &identity, &client, "a@b.c", true, "INVITE",
        )),
        emitted(&ensured_with_app),
        emitted(&IdentityStatusSet::new(
            &ctx,
            &identity,
            &client,
            IdentityStatus::Disabled,
        )),
        emitted(&IdentityDeleted::new(&ctx, &identity, &client, "a@b.c")),
        emitted(&IdentityAppGranted::new(
            &ctx,
            &identity,
            &client,
            &app_id,
            "suppliers",
            "ADMIN",
        )),
        emitted(&IdentityAppRevoked::new(
            &ctx,
            &identity,
            &client,
            &app_id,
            "suppliers",
        )),
        emitted(&AppDocsSynced {
            metadata: meta(AppDocsSynced::EVENT_TYPE),
            application_code: "billing".into(),
            created: 1,
            updated: 2,
            deleted: 0,
            synced_codes: vec!["intro".into()],
        }),
        emitted(&AuditLogsRedacted::new(&ctx, 120, 7)),
        emitted(&PermissionDefined::new(
            &ctx,
            &PermissionId::from_wire("perm_1"),
            "billing:invoice:view",
            Some("View invoices"),
        )),
        emitted(&PermissionDefined::new(
            &ctx,
            &PermissionId::from_wire("perm_1"),
            "billing:invoice:view",
            None,
        )),
        emitted(&PermissionDeleted::new(
            &ctx,
            &PermissionId::from_wire("perm_1"),
            "billing:invoice:view",
        )),
        emitted(&ServiceAccountTokenMinted::new(&ctx, "sac_1", "billing-sa")),
    ];

    // The three function events Java's golden does not list.
    let version_id = FunctionVersionId::from_wire("fnv_3");
    let function_id = FunctionId::from_wire("fnc_1");
    for previous in [None, Some(FunctionVersionId::from_wire("fnv_2"))] {
        out.push((
            "platform:function:alias:changed".into(),
            data_of(&AliasChanged {
                metadata: EventMetadata::default(),
                function_id: function_id.clone(),
                address: "billing.invoices.create".into(),
                alias: "live".into(),
                version_id: version_id.clone(),
                version: 3,
                previous_version_id: previous,
            }),
        ));
    }
    out.push((
        "platform:function:alias:removed".into(),
        data_of(&AliasRemoved {
            metadata: EventMetadata::default(),
            function_id: function_id.clone(),
            address: "billing.invoices.create".into(),
            alias: "live".into(),
            version_id: version_id.clone(),
            version: 3,
        }),
    ));
    out.push((
        "platform:function:version:ready".into(),
        data_of(&VersionReady {
            metadata: EventMetadata::default(),
            function_id,
            address: "billing.invoices.create".into(),
            version_id,
            version: 3,
            host_id: "fnh_1".into(),
        }),
    ));

    // Every function event Java's golden lists (the Rust events equal it).
    let golden: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/function/events-golden.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for e in golden["events"].as_array().unwrap() {
        out.push((
            e["type"].as_str().unwrap().to_string(),
            serde_json::from_str(e["data"].as_str().unwrap()).unwrap(),
        ));
    }
    out
}

#[test]
fn emitted_data_validates_against_the_catalogue_schema() {
    let mut errors = Vec::new();
    for (code, data) in samples() {
        errors.extend(errors_for(&code, &data));
    }
    assert!(errors.is_empty(), "{errors:#?}");
}

/// Real data of every code migration 070 corrects, each optional field
/// present and absent where it has any.
fn corrected_samples() -> Vec<(String, Value)> {
    let ctx = ExecutionContext::system("prn_1");
    let user = PrincipalId::from_wire("prn_2");
    let app = ApplicationId::from_wire("app_1");
    let et = EventTypeId::from_wire("etp_1");
    let conn = ConnectionId::from_wire("con_1");
    let pool = DispatchPoolId::from_wire("dpl_1");
    let sub = SubscriptionId::from_wire("sub_1");
    let account = ServiceAccount::new("billing-sa", "Billing", UserScope::Anchor);
    let claims = FlowcatalystClaims {
        email: "a@b.c".into(),
        principal_type: "USER".into(),
        roles: vec!["platform:viewer".into()],
        clients: vec!["*".into()],
        applications: vec![],
    };
    let mut rotated = OAuthClientSecretRotated::new(&ctx, &OAuthClientId::from_wire("oac_1"));
    let rotated_now = emitted(&rotated);
    rotated.previous_secret_expires_at = Some(Utc::now());
    let roles_synced = |code: &str, synced: Vec<String>| RolesSynced {
        metadata: RolesSynced::metadata_for(&ctx, code),
        created: 1,
        updated: 0,
        removed: 0,
        total: 1,
        application_code: code.into(),
        synced_codes: synced,
    };
    let subs_synced = |client: Option<ClientId>| SubscriptionsSynced {
        metadata: SubscriptionsSynced::metadata_for(&ctx, "billing"),
        application_code: "billing".into(),
        client_id: client,
        created: 1,
        updated: 0,
        deleted: 0,
        synced_codes: vec!["s1".into()],
    };
    vec![
        emitted(&UserCreated::new(&ctx, &user, "a@b.c")),
        emitted(&UserUpdated::new(&ctx, &user, "A")),
        emitted(&UserDeactivated::new(&ctx, &user)),
        emitted(&UserDeleted::new(&ctx, &user, "a@b.c")),
        emitted(&UserDeleted::new(&ctx, &user, "")),
        emitted(&PasswordResetCompleted::from_ctx(&ctx, &user)),
        emitted(&UserLoggedIn::new(
            &ctx,
            &user,
            "a@b.c",
            "OIDC",
            None,
            claims.clone(),
            None,
        )),
        emitted(&UserLoggedIn::new(
            &ctx,
            &user,
            "a@b.c",
            "OIDC",
            Some("entra"),
            claims,
            Some(FederatedClaims {
                access_token: json!({"sub": "x"}),
                id_token: json!({"sub": "x"}),
            }),
        )),
        emitted(&ServiceAccountCreated::new(&ctx, &account)),
        emitted(&ServiceAccountUpdated::new(&ctx, &account)),
        emitted(&ApplicationCreated::new(&ctx, &app, "billing", "Billing")),
        emitted(&ApplicationUpdated::new(&ctx, &app, "Billing")),
        emitted(&ApplicationActivated::new(&ctx, &app)),
        emitted(&ApplicationDeactivated::new(&ctx, &app)),
        emitted(&ApplicationDeleted::new(&ctx, &app, "billing")),
        emitted(&ClientApplicationsUpdated::new(
            &ctx,
            &ClientId::from_wire("clt_1"),
            vec![],
            vec![],
            vec![],
        )),
        emitted(&ClientApplicationsUpdated::new(
            &ctx,
            &ClientId::from_wire("clt_1"),
            vec![app.clone()],
            vec![app.clone()],
            vec![],
        )),
        emitted(&EventTypeUpdated::new(&ctx, &et, "Thing Done", None)),
        emitted(&EventTypeUpdated::new(&ctx, &et, "Thing Done", Some("d"))),
        emitted(&SchemaAdded::new(&ctx, &et, "1.1")),
        emitted(&SchemaFinalised::new(&ctx, &et, "1.1", None)),
        emitted(&SchemaFinalised::new(&ctx, &et, "1.1", Some("1.0"))),
        emitted(&SchemaDeprecated::new(&ctx, &et, "1.0")),
        emitted(&ConnectionCreated::new(&ctx, &conn, "hook", "Hook")),
        emitted(&ConnectionUpdated::new(&ctx, &conn, "Hook")),
        emitted(&ConnectionDeleted::new(&ctx, &conn, "hook")),
        emitted(&DispatchPoolCreated::new(&ctx, &pool, "p", "P")),
        emitted(&DispatchPoolUpdated::new(&ctx, &pool, "P")),
        emitted(&DispatchPoolArchived::new(&ctx, &pool, "p")),
        emitted(&DispatchPoolDeleted::new(&ctx, &pool, "p")),
        emitted(&SubscriptionCreated::new(&ctx, &sub, "s", "S")),
        emitted(&SubscriptionUpdated::new(&ctx, &sub, "S")),
        emitted(&SubscriptionPaused::new(&ctx, &sub)),
        emitted(&SubscriptionResumed::new(&ctx, &sub)),
        emitted(&subs_synced(None)),
        emitted(&subs_synced(Some(ClientId::from_wire("clt_1")))),
        emitted(&roles_synced("", vec![])),
        emitted(&roles_synced("billing", vec!["billing:clerk".into()])),
        rotated_now,
        emitted(&rotated),
    ]
}

/// The codes migration 070 replaces a seeded schema of.
fn migration_070_codes() -> BTreeSet<String> {
    let sql = include_str!("../../../../migrations/070_platform_event_schema_fixes.sql");
    sql.lines()
        .filter_map(|l| l.trim().strip_prefix("('platform:"))
        .filter_map(|rest| rest.split_once('\'').map(|(c, _)| format!("platform:{c}")))
        .collect()
}

#[test]
fn emitted_data_of_the_corrected_codes_validates() {
    let mut errors = Vec::new();
    for (code, data) in corrected_samples() {
        errors.extend(errors_for(&code, &data));
    }
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn every_code_migration_070_corrects_is_sampled() {
    let codes = migration_070_codes();
    assert_eq!(codes.len(), 32, "{codes:#?}");
    let sampled: BTreeSet<String> = corrected_samples().into_iter().map(|(c, _)| c).collect();
    let missing: Vec<&String> = codes.iter().filter(|c| !sampled.contains(*c)).collect();
    assert!(missing.is_empty(), "no sample: {missing:#?}");
}

/// The corrected schemas reject the older shapes they replace: the
/// dispatch-pool id name, the event-type schema version name, a null list
/// where the field is left out, a timestamp that is not one.
#[test]
fn the_corrected_schemas_reject_the_old_shapes() {
    assert!(!errors_for(
        "platform:admin:dispatch-pool:created",
        &json!({"dispatchPoolId": "dpl_1", "code": "p", "name": "P"})
    )
    .is_empty());
    assert!(!errors_for(
        "platform:admin:eventtype:schema-added",
        &json!({"eventTypeId": "etp_1", "version": "1.0"})
    )
    .is_empty());
    assert!(!errors_for(
        "platform:admin:roles:synced",
        &json!({"created": 0, "updated": 0, "removed": 0, "total": 0, "syncedCodes": null})
    )
    .is_empty());
    assert!(!errors_for(
        "platform:admin:oauth-client:secret-rotated",
        &json!({"oauthClientId": "oac_1", "previousSecretExpiresAt": "tomorrow"})
    )
    .is_empty());
}

/// Every code catalogued on 2026-10-11 has at least one real sample above.
#[test]
fn every_newly_catalogued_code_is_sampled() {
    let sampled: BTreeSet<String> = samples().into_iter().map(|(c, _)| c).collect();
    let catalogued: BTreeSet<String> = platform_event_types::definitions()
        .into_iter()
        .map(|d| d.code)
        .collect();
    for code in [
        "platform:admin:app-docs:synced",
        "platform:admin:audit-log:redacted",
        "platform:admin:email-domain-mapping:provider-changed",
        "platform:admin:permission:defined",
        "platform:admin:permission:deleted",
        "platform:function:alias:changed",
        "platform:function:alias:removed",
        "platform:function:config:updated",
        "platform:function:domain:claimed",
        "platform:function:domain:released",
        "platform:function:function:created",
        "platform:function:function:deleted",
        "platform:function:function:updated",
        "platform:function:policy:updated",
        "platform:function:secret:deleted",
        "platform:function:secret:set",
        "platform:function:version:published",
        "platform:function:version:ready",
        "platform:function:version:retired",
        "platform:iam:serviceaccount:token-minted",
        "platform:iam:user:developer-credential-revoked",
        "platform:iam:user:developer-credential-set",
        "platform:messaging:dispatch-job:cancelled",
        "platform:messaging:dispatch-job:completed",
        "platform:messaging:dispatch-job:payload-edited",
        "platform:messaging:dispatch-jobs:resent",
        "platform:portal:app:created",
        "platform:portal:app:deleted",
        "platform:portal:app:updated",
        "platform:portal:identity:app-granted",
        "platform:portal:identity:app-revoked",
        "platform:portal:identity:deleted",
        "platform:portal:identity:ensured",
        "platform:portal:identity:status-set",
    ] {
        assert!(catalogued.contains(code), "{code} is not catalogued");
        assert!(sampled.contains(code), "{code} has no sample");
    }
}

/// The validator rejects what it should, so a pass means something.
#[test]
fn the_validator_rejects_envelope_fields_and_wrong_types() {
    let code = "platform:iam:user:developer-credential-set";
    assert!(errors_for(code, &json!({"userId": "prn_1"})).is_empty());
    assert!(!errors_for(code, &json!({"userId": "prn_1", "event_type": "x"})).is_empty());
    assert!(!errors_for(code, &json!({})).is_empty());
    assert!(!errors_for(code, &json!({"userId": 1})).is_empty());
    assert!(!errors_for(
        "platform:admin:audit-log:redacted",
        &json!({"scanned": -1, "redacted": 0})
    )
    .is_empty());
}

//! Snapshot of exactly what a UoW commit persists for a domain event.
//!
//! For a representative event from every domain module, this pins the
//! `msg_events` row (envelope columns + `data` payload + `context_data`) and
//! the `aud_logs` row derived from the event and command. Event ids and
//! timestamps are fixed so the JSON is byte-for-byte stable; any change to
//! event construction, serialization or the UoW row mapping shows up here.

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;

use super::unit_of_work::{AuditRow, EventRow};
use super::{DomainEvent, ExecutionContext};

use crate::application::operations::events::{ApplicationClientConfigUpdated, ApplicationCreated};
use crate::application_openapi_spec::operations::events::ApplicationOpenApiSpecSynced;
use crate::auth::operations::events::{AuthConfigCreated, IdpRoleMappingCreated};
use crate::client::operations::events::ClientCreated;
use crate::connection::operations::events::ConnectionCreated;
use crate::cors::operations::events::CorsOriginAdded;
use crate::dispatch_pool::operations::events::DispatchPoolsSynced;
use crate::email_domain_mapping::operations::events::EmailDomainMappingCreated;
use crate::event_type::operations::{EventTypeCreated, EventTypesSynced};
use crate::identity_provider::operations::events::IdentityProviderCreated;
use crate::platform_config::operations::events::{
    PlatformConfigAccessGranted, PlatformConfigPropertySet,
};
use crate::principal::entity::UserScope;
use crate::principal::operations::events::{
    FederatedClaims, FlowcatalystClaims, RolesAssigned, UserCreated, UserLoggedIn,
};
use crate::process::operations::{ProcessCreated, ProcessUpdated};
use crate::role::operations::events::RoleCreated;
use crate::scheduled_job::operations::events::{ScheduledJobCreated, ScheduledJobsSynced};
use crate::service_account::operations::events::{
    ServiceAccountCreated, ServiceAccountSecretRegenerated, ServiceAccountTokenRegenerated,
};
use crate::service_account::operations::{
    CreateServiceAccountResult, RegenerateAuthTokenResult, RegenerateSigningSecretResult,
};
use crate::subscription::operations::events::SubscriptionCreated;
use crate::webauthn::operations::events::PasskeyRegistered;

const EVENT_ID: &str = "evt_0SNAPSHOT0001";

fn fixed_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
}

/// A context carrying a causation id (event raised in reaction to another).
fn ctx() -> ExecutionContext {
    ExecutionContext {
        execution_id: "exec-snap".to_string(),
        correlation_id: "corr-snap".to_string(),
        causation_id: Some("evt_parent".to_string()),
        principal_id: "prn_actor".to_string(),
        initiated_at: fixed_time(),
    }
}

/// A fresh-request context (no causation id).
fn fresh_ctx() -> ExecutionContext {
    ExecutionContext {
        causation_id: None,
        ..ctx()
    }
}

/// Pin the two non-deterministic metadata fields of a freshly built event.
macro_rules! fixed {
    ($event:expr) => {{
        let mut event = $event;
        event.metadata.event_id = EVENT_ID.to_string();
        event.metadata.time = fixed_time();
        event
    }};
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotCommand {
    target_id: &'static str,
}

const CMD: SnapshotCommand = SnapshotCommand {
    target_id: "cmd-target",
};

/// Everything a commit writes for `event`, as one compact JSON string.
fn persisted<E: DomainEvent, C: Serialize>(event: &E, command: &C) -> String {
    let event_row = EventRow::from_event(event).expect("event row");
    let audit_row = AuditRow::from_event(event, command);
    serde_json::to_string(&serde_json::json!({
        "msg_events": event_row,
        "aud_logs": audit_row,
    }))
    .expect("serialize rows")
}

#[track_caller]
fn check<E: DomainEvent>(event: &E, expected: &str) {
    let actual = persisted(event, &CMD);
    assert_eq!(actual, expected, "\nactual:\n{actual}\n");
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

// ── application ─────────────────────────────────────────────────────────────

#[test]
fn application_created() {
    let e = fixed!(ApplicationCreated::new(
        &ctx(),
        "app_1",
        "orders",
        "Orders",
        "APPLICATION"
    ));
    check(&e, EXPECTED_APPLICATION_CREATED);
}

#[test]
fn application_client_config_updated() {
    let e = fixed!(ApplicationClientConfigUpdated::new(
        &ctx(),
        "app_1",
        "clt_1",
        "acc_1",
        Some(true),
        None,
        false
    ));
    check(&e, EXPECTED_APPLICATION_CLIENT_CONFIG_UPDATED);
}

#[test]
fn application_openapi_spec_synced() {
    let e = fixed!(ApplicationOpenApiSpecSynced::new(
        &ctx(),
        "app_1",
        "orders",
        "spec_1",
        "1.2.0",
        "sha256:abc",
        Some("1.1.0".to_string()),
        true,
        false
    ));
    check(&e, EXPECTED_APPLICATION_OPENAPI_SPEC_SYNCED);
}

// ── auth ────────────────────────────────────────────────────────────────────

#[test]
fn auth_config_created() {
    let e = fixed!(AuthConfigCreated::new(
        &fresh_ctx(),
        "cac_1",
        "example.com",
        "INTERNAL"
    ));
    check(&e, EXPECTED_AUTH_CONFIG_CREATED);
}

#[test]
fn idp_role_mapping_created() {
    let e = fixed!(IdpRoleMappingCreated::new(
        &ctx(),
        "irm_1",
        "okta-admins",
        "platform:admin"
    ));
    check(&e, EXPECTED_IDP_ROLE_MAPPING_CREATED);
}

// ── client / connection / cors ──────────────────────────────────────────────

#[test]
fn client_created() {
    let e = fixed!(ClientCreated::new(
        &ctx(),
        "clt_1",
        "Acme",
        "acme",
        Some("Acme Corp")
    ));
    check(&e, EXPECTED_CLIENT_CREATED);
}

#[test]
fn connection_created() {
    let e = fixed!(ConnectionCreated::new(
        &ctx(),
        "con_1",
        "orders-hook",
        "Orders hook",
        "sac_1",
        None
    ));
    check(&e, EXPECTED_CONNECTION_CREATED);
}

#[test]
fn cors_origin_added() {
    let e = fixed!(CorsOriginAdded::new(
        &ctx(),
        "cor_1",
        "https://app.example.com"
    ));
    check(&e, EXPECTED_CORS_ORIGIN_ADDED);
}

// ── dispatch_pool / email_domain_mapping / identity_provider ────────────────

#[test]
fn dispatch_pools_synced() {
    let e = fixed!(DispatchPoolsSynced::new(
        &ctx(),
        "orders",
        2,
        1,
        0,
        s(&["default", "bulk"])
    ));
    check(&e, EXPECTED_DISPATCH_POOLS_SYNCED);
}

#[test]
fn email_domain_mapping_created() {
    let e = fixed!(EmailDomainMappingCreated::new(
        &ctx(),
        "edm_1",
        "example.com",
        "idp_1",
        "CLIENT"
    ));
    check(&e, EXPECTED_EMAIL_DOMAIN_MAPPING_CREATED);
}

#[test]
fn identity_provider_created() {
    let e = fixed!(IdentityProviderCreated::new(
        &ctx(),
        "idp_1",
        "okta",
        "Okta",
        "OIDC"
    ));
    check(&e, EXPECTED_IDENTITY_PROVIDER_CREATED);
}

// ── event_type ──────────────────────────────────────────────────────────────

/// Mirrors `CreateEventTypeUseCase::execute`.
#[test]
fn event_type_created() {
    let ctx = ctx();
    let event = EventTypeCreated::builder()
        .with_context(&ctx)
        .event_type_id("evt_type_1")
        .code("orders:fulfillment:shipment:shipped")
        .name("Shipment shipped")
        .application("orders")
        .subdomain("fulfillment")
        .aggregate("shipment")
        .event_name("shipped")
        .build();
    let event = EventTypeCreated {
        description: Some("A shipment left the warehouse".to_string()),
        client_id: Some("clt_1".to_string()),
        ..event
    };
    let e = fixed!(event);
    check(&e, EXPECTED_EVENT_TYPE_CREATED);
}

#[test]
fn event_types_synced() {
    let e = fixed!(EventTypesSynced::new(
        &ctx(),
        "orders",
        3,
        2,
        1,
        s(&["orders:a:b:c"]),
        4,
        5,
        6
    ));
    check(&e, EXPECTED_EVENT_TYPES_SYNCED);
}

// ── platform_config ─────────────────────────────────────────────────────────

#[test]
fn platform_config_property_set() {
    let e = fixed!(PlatformConfigPropertySet::new(
        &ctx(),
        "pcf_1",
        "orders",
        "limits",
        "max_batch",
        "CLIENT",
        Some("clt_1"),
        "NUMBER",
        true
    ));
    check(&e, EXPECTED_PLATFORM_CONFIG_PROPERTY_SET);
}

#[test]
fn platform_config_access_granted() {
    let e = fixed!(PlatformConfigAccessGranted::new(
        &ctx(),
        "pca_1",
        "orders",
        "orders:viewer",
        true,
        false,
        true
    ));
    check(&e, EXPECTED_PLATFORM_CONFIG_ACCESS_GRANTED);
}

// ── principal ───────────────────────────────────────────────────────────────

/// Mirrors `CreateUserUseCase::execute`.
#[test]
fn user_created() {
    let ctx = fresh_ctx();
    let e = fixed!(UserCreated::builder()
        .from(&ctx)
        .principal_id("prn_1")
        .email("Jane@Example.COM")
        .name("Jane")
        .scope(UserScope::Anchor)
        .client_id(None::<&str>)
        .is_anchor_user(true)
        .build());
    check(&e, EXPECTED_USER_CREATED);
}

#[test]
fn user_logged_in() {
    let claims = FlowcatalystClaims {
        email: "jane@example.com".to_string(),
        principal_type: "USER".to_string(),
        roles: s(&["platform:admin"]),
        clients: s(&["*"]),
        applications: s(&["platform"]),
    };
    let federated = FederatedClaims {
        access_token: serde_json::json!({"aud": "api://default"}),
        id_token: serde_json::json!({"sub": "ext-1"}),
    };
    let e = fixed!(UserLoggedIn::new(
        &ctx(),
        "prn_1",
        "jane@example.com",
        "OIDC",
        Some("okta"),
        claims,
        Some(federated)
    ));
    check(&e, EXPECTED_USER_LOGGED_IN);
}

#[test]
fn roles_assigned() {
    let e = fixed!(RolesAssigned::new(
        &ctx(),
        "prn_1",
        s(&["a", "b"]),
        s(&["b"]),
        s(&["c"])
    ));
    check(&e, EXPECTED_ROLES_ASSIGNED);
}

// ── process ─────────────────────────────────────────────────────────────────

#[test]
fn process_created() {
    let e = fixed!(ProcessCreated::new(
        &ctx(),
        "prc_1",
        "orders:fulfillment:ship",
        "Ship",
        Some("Ship an order"),
        "orders",
        "fulfillment",
        "ship"
    ));
    check(&e, EXPECTED_PROCESS_CREATED);
}

#[test]
fn process_updated() {
    let tags = s(&["x", "y"]);
    let e = fixed!(ProcessUpdated::new(
        &ctx(),
        "prc_1",
        Some("Ship v2"),
        None,
        Some(true),
        Some(tags.as_slice())
    ));
    check(&e, EXPECTED_PROCESS_UPDATED);
}

// ── role ────────────────────────────────────────────────────────────────────

#[test]
fn role_created() {
    let e = fixed!(RoleCreated::new(
        &ctx(),
        "rol_1",
        "orders:viewer",
        "Orders viewer",
        "orders",
        s(&["orders:order:read"])
    ));
    check(&e, EXPECTED_ROLE_CREATED);
}

// ── scheduled_job ───────────────────────────────────────────────────────────

#[test]
fn scheduled_job_created() {
    let e = fixed!(ScheduledJobCreated::new(
        &ctx(),
        "sjb_1",
        Some("clt_1"),
        "nightly",
        "Nightly",
        &s(&["0 0 * * *"]),
        "UTC",
        false,
        true
    ));
    check(&e, EXPECTED_SCHEDULED_JOB_CREATED);
}

#[test]
fn scheduled_jobs_synced() {
    let e = fixed!(ScheduledJobsSynced::new(
        &ctx(),
        "orders",
        None,
        s(&["a"]),
        s(&["b"]),
        s(&[])
    ));
    check(&e, EXPECTED_SCHEDULED_JOBS_SYNCED);
}

// ── service_account ─────────────────────────────────────────────────────────
//
// The use cases return wrapper results carrying one-time secrets. The secrets
// must never reach the persisted event.

#[test]
fn service_account_created() {
    let e = fixed!(ServiceAccountCreated::new(
        &ctx(),
        "sac_1",
        "orders-bot",
        "Orders bot",
        Some("app_1"),
        s(&["clt_1"])
    ));
    check(&e, EXPECTED_SERVICE_ACCOUNT_CREATED);
    let result = CreateServiceAccountResult {
        event: e,
        auth_token: "fc_secret_token".to_string(),
        signing_secret: "secret_signing".to_string(),
    };
    check(&result, EXPECTED_SERVICE_ACCOUNT_CREATED);
}

#[test]
fn service_account_token_regenerated() {
    let e = fixed!(ServiceAccountTokenRegenerated::new(
        &ctx(),
        "sac_1",
        "orders-bot"
    ));
    check(&e, EXPECTED_SERVICE_ACCOUNT_TOKEN_REGENERATED);
    let result = RegenerateAuthTokenResult {
        event: e,
        auth_token: "fc_secret_token".to_string(),
    };
    check(&result, EXPECTED_SERVICE_ACCOUNT_TOKEN_REGENERATED);
}

#[test]
fn service_account_secret_regenerated() {
    let e = fixed!(ServiceAccountSecretRegenerated::new(
        &ctx(),
        "sac_1",
        "orders-bot"
    ));
    check(&e, EXPECTED_SERVICE_ACCOUNT_SECRET_REGENERATED);
    let result = RegenerateSigningSecretResult {
        event: e,
        signing_secret: "secret_signing".to_string(),
    };
    check(&result, EXPECTED_SERVICE_ACCOUNT_SECRET_REGENERATED);
}

// ── subscription / webauthn ─────────────────────────────────────────────────

#[test]
fn subscription_created() {
    let e = fixed!(SubscriptionCreated::new(
        &ctx(),
        "sub_1",
        "orders-shipped",
        "Orders shipped",
        "https://hooks.example.com/shipped",
        s(&["orders:fulfillment:shipment:shipped"]),
        Some("clt_1")
    ));
    check(&e, EXPECTED_SUBSCRIPTION_CREATED);
}

#[test]
fn passkey_registered() {
    let e = fixed!(PasskeyRegistered::new(
        &ctx(),
        "pkc_1",
        "prn_1",
        Some("YubiKey".to_string())
    ));
    check(&e, EXPECTED_PASSKEY_REGISTERED);
}

// ── expected rows ───────────────────────────────────────────────────────────

const EXPECTED_APPLICATION_CREATED: &str = r#"{"aud_logs":{"entity_id":"app_1","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationId":"app_1","applicationType":"APPLICATION","causation_id":"evt_parent","code":"orders","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:application:created","execution_id":"exec-snap","message_group":"platform:application:app_1","name":"Orders","principal_id":"prn_actor","source":"platform:application","spec_version":"1.0","subject":"platform.application.app_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:application:created-evt_0SNAPSHOT0001","event_type":"platform:iam:application:created","id":"evt_0SNAPSHOT0001","message_group":"platform:application:app_1","source":"platform:application","spec_version":"1.0","subject":"platform.application.app_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_APPLICATION_CLIENT_CONFIG_UPDATED: &str = r#"{"aud_logs":{"entity_id":"app_1","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationId":"app_1","causation_id":"evt_parent","clientId":"clt_1","configChanged":false,"configId":"acc_1","correlation_id":"corr-snap","enabled":true,"event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:application:client-config-updated","execution_id":"exec-snap","message_group":"platform:application:app_1","principal_id":"prn_actor","source":"platform:application","spec_version":"1.0","subject":"platform.application.app_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:application:client-config-updated-evt_0SNAPSHOT0001","event_type":"platform:iam:application:client-config-updated","id":"evt_0SNAPSHOT0001","message_group":"platform:application:app_1","source":"platform:application","spec_version":"1.0","subject":"platform.application.app_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_APPLICATION_OPENAPI_SPEC_SYNCED: &str = r#"{"aud_logs":{"entity_id":"spec_1","entity_type":"Application-openapi","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application-openapi"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","applicationId":"app_1","archivedPriorVersion":"1.1.0","causation_id":"evt_parent","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:developer:application-openapi:synced","execution_id":"exec-snap","hasBreaking":true,"message_group":"platform:application-openapi:app_1","principal_id":"prn_actor","source":"platform:developer","specHash":"sha256:abc","specId":"spec_1","spec_version":"1.0","subject":"platform.application-openapi.spec_1","time":"2026-01-02T03:04:05Z","unchanged":false,"version":"1.2.0"},"deduplication_id":"platform:developer:application-openapi:synced-evt_0SNAPSHOT0001","event_type":"platform:developer:application-openapi:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:application-openapi:app_1","source":"platform:developer","spec_version":"1.0","subject":"platform.application-openapi.spec_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_AUTH_CONFIG_CREATED: &str = r#"{"aud_logs":{"entity_id":"cac_1","entity_type":"Authconfig","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":null,"context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Authconfig"}],"correlation_id":"corr-snap","data":{"authConfigId":"cac_1","configType":"INTERNAL","correlation_id":"corr-snap","emailDomain":"example.com","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:auth-config:created","execution_id":"exec-snap","message_group":"platform:authconfig:cac_1","principal_id":"prn_actor","source":"platform:iam","spec_version":"1.0","subject":"platform.authconfig.cac_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:auth-config:created-evt_0SNAPSHOT0001","event_type":"platform:iam:auth-config:created","id":"evt_0SNAPSHOT0001","message_group":"platform:authconfig:cac_1","source":"platform:iam","spec_version":"1.0","subject":"platform.authconfig.cac_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_IDP_ROLE_MAPPING_CREATED: &str = r#"{"aud_logs":{"entity_id":"irm_1","entity_type":"Idprolemapping","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Idprolemapping"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:idp-role-mapping:created","execution_id":"exec-snap","idpRole":"okta-admins","idpRoleMappingId":"irm_1","mappedRole":"platform:admin","message_group":"platform:idprolemapping:irm_1","principal_id":"prn_actor","source":"platform:iam","spec_version":"1.0","subject":"platform.idprolemapping.irm_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:idp-role-mapping:created-evt_0SNAPSHOT0001","event_type":"platform:iam:idp-role-mapping:created","id":"evt_0SNAPSHOT0001","message_group":"platform:idprolemapping:irm_1","source":"platform:iam","spec_version":"1.0","subject":"platform.idprolemapping.irm_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_CLIENT_CREATED: &str = r#"{"aud_logs":{"entity_id":"clt_1","entity_type":"Client","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Client"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","clientId":"clt_1","correlation_id":"corr-snap","description":"Acme Corp","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:client:created","execution_id":"exec-snap","identifier":"acme","message_group":"platform:client:clt_1","name":"Acme","principal_id":"prn_actor","source":"platform:iam","spec_version":"1.0","subject":"platform.client.clt_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:client:created-evt_0SNAPSHOT0001","event_type":"platform:iam:client:created","id":"evt_0SNAPSHOT0001","message_group":"platform:client:clt_1","source":"platform:iam","spec_version":"1.0","subject":"platform.client.clt_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_CONNECTION_CREATED: &str = r#"{"aud_logs":{"entity_id":"con_1","entity_type":"Connection","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Connection"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","code":"orders-hook","connectionId":"con_1","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:connection:created","execution_id":"exec-snap","message_group":"platform:connection:con_1","name":"Orders hook","principal_id":"prn_actor","serviceAccountId":"sac_1","source":"platform:admin","spec_version":"1.0","subject":"platform.connection.con_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:connection:created-evt_0SNAPSHOT0001","event_type":"platform:admin:connection:created","id":"evt_0SNAPSHOT0001","message_group":"platform:connection:con_1","source":"platform:admin","spec_version":"1.0","subject":"platform.connection.con_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_CORS_ORIGIN_ADDED: &str = r#"{"aud_logs":{"entity_id":"cor_1","entity_type":"Cors","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Cors"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:cors:origin-added","execution_id":"exec-snap","message_group":"platform:cors:cor_1","origin":"https://app.example.com","originId":"cor_1","principal_id":"prn_actor","source":"platform:admin","spec_version":"1.0","subject":"platform.cors.cor_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:cors:origin-added-evt_0SNAPSHOT0001","event_type":"platform:admin:cors:origin-added","id":"evt_0SNAPSHOT0001","message_group":"platform:cors:cor_1","source":"platform:admin","spec_version":"1.0","subject":"platform.cors.cor_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_DISPATCH_POOLS_SYNCED: &str = r#"{"aud_logs":{"entity_id":"orders","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","correlation_id":"corr-snap","created":2,"deleted":0,"event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:dispatch-pools:synced","execution_id":"exec-snap","message_group":"platform:application:orders","principal_id":"prn_actor","source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","syncedCodes":["default","bulk"],"time":"2026-01-02T03:04:05Z","updated":1},"deduplication_id":"platform:admin:dispatch-pools:synced-evt_0SNAPSHOT0001","event_type":"platform:admin:dispatch-pools:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:application:orders","source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_EMAIL_DOMAIN_MAPPING_CREATED: &str = r#"{"aud_logs":{"entity_id":"edm_1","entity_type":"Edm","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Edm"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","correlation_id":"corr-snap","emailDomain":"example.com","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:edm:created","execution_id":"exec-snap","identityProviderId":"idp_1","mappingId":"edm_1","message_group":"platform:edm:edm_1","principal_id":"prn_actor","scopeType":"CLIENT","source":"platform:admin","spec_version":"1.0","subject":"platform.edm.edm_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:edm:created-evt_0SNAPSHOT0001","event_type":"platform:admin:edm:created","id":"evt_0SNAPSHOT0001","message_group":"platform:edm:edm_1","source":"platform:admin","spec_version":"1.0","subject":"platform.edm.edm_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_IDENTITY_PROVIDER_CREATED: &str = r#"{"aud_logs":{"entity_id":"idp_1","entity_type":"Idp","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Idp"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","code":"okta","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:idp:created","execution_id":"exec-snap","idpId":"idp_1","idpType":"OIDC","message_group":"platform:idp:idp_1","name":"Okta","principal_id":"prn_actor","source":"platform:admin","spec_version":"1.0","subject":"platform.idp.idp_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:idp:created-evt_0SNAPSHOT0001","event_type":"platform:admin:idp:created","id":"evt_0SNAPSHOT0001","message_group":"platform:idp:idp_1","source":"platform:admin","spec_version":"1.0","subject":"platform.idp.idp_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_EVENT_TYPE_CREATED: &str = r#"{"aud_logs":{"entity_id":"evt_type_1","entity_type":"Eventtype","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Eventtype"}],"correlation_id":"corr-snap","data":{"aggregate":"shipment","application":"orders","causation_id":"evt_parent","clientId":"clt_1","code":"orders:fulfillment:shipment:shipped","correlation_id":"corr-snap","description":"A shipment left the warehouse","eventName":"shipped","eventTypeId":"evt_type_1","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:eventtype:created","execution_id":"exec-snap","message_group":"platform:eventtype:evt_type_1","name":"Shipment shipped","principal_id":"prn_actor","source":"platform:admin","spec_version":"1.0","subdomain":"fulfillment","subject":"platform.eventtype.evt_type_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:eventtype:created-evt_0SNAPSHOT0001","event_type":"platform:admin:eventtype:created","id":"evt_0SNAPSHOT0001","message_group":"platform:eventtype:evt_type_1","source":"platform:admin","spec_version":"1.0","subject":"platform.eventtype.evt_type_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_EVENT_TYPES_SYNCED: &str = r#"{"aud_logs":{"entity_id":"orders","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","correlation_id":"corr-snap","created":3,"deleted":1,"event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:eventtypes:synced","execution_id":"exec-snap","message_group":"platform:application:orders","principal_id":"prn_actor","schemasCreated":4,"schemasUnchanged":6,"schemasUpdated":5,"source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","syncedCodes":["orders:a:b:c"],"time":"2026-01-02T03:04:05Z","updated":2},"deduplication_id":"platform:admin:eventtypes:synced-evt_0SNAPSHOT0001","event_type":"platform:admin:eventtypes:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:application:orders","source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_PLATFORM_CONFIG_PROPERTY_SET: &str = r#"{"aud_logs":{"entity_id":"pcf_1","entity_type":"Platformconfig","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Platformconfig"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","clientId":"clt_1","configId":"pcf_1","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:config:property-set","execution_id":"exec-snap","message_group":"platform:platformconfig:pcf_1","principal_id":"prn_actor","property":"max_batch","scope":"CLIENT","section":"limits","source":"platform:admin","spec_version":"1.0","subject":"platform.platformconfig.pcf_1","time":"2026-01-02T03:04:05Z","valueType":"NUMBER","wasCreated":true},"deduplication_id":"platform:admin:config:property-set-evt_0SNAPSHOT0001","event_type":"platform:admin:config:property-set","id":"evt_0SNAPSHOT0001","message_group":"platform:platformconfig:pcf_1","source":"platform:admin","spec_version":"1.0","subject":"platform.platformconfig.pcf_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_PLATFORM_CONFIG_ACCESS_GRANTED: &str = r#"{"aud_logs":{"entity_id":"pca_1","entity_type":"Platformconfigaccess","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Platformconfigaccess"}],"correlation_id":"corr-snap","data":{"accessId":"pca_1","applicationCode":"orders","canRead":true,"canWrite":false,"causation_id":"evt_parent","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:config-access:granted","execution_id":"exec-snap","message_group":"platform:platformconfigaccess:pca_1","principal_id":"prn_actor","roleCode":"orders:viewer","source":"platform:admin","spec_version":"1.0","subject":"platform.platformconfigaccess.pca_1","time":"2026-01-02T03:04:05Z","wasCreated":true},"deduplication_id":"platform:admin:config-access:granted-evt_0SNAPSHOT0001","event_type":"platform:admin:config-access:granted","id":"evt_0SNAPSHOT0001","message_group":"platform:platformconfigaccess:pca_1","source":"platform:admin","spec_version":"1.0","subject":"platform.platformconfigaccess.pca_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_USER_CREATED: &str = r#"{"aud_logs":{"entity_id":"prn_1","entity_type":"User","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":null,"context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"User"}],"correlation_id":"corr-snap","data":{"correlation_id":"corr-snap","email":"Jane@Example.COM","emailDomain":"example.com","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:user:created","execution_id":"exec-snap","isAnchorUser":true,"message_group":"platform:user:prn_1","name":"Jane","principalId":"prn_1","principal_id":"prn_actor","scope":"ANCHOR","source":"platform:iam","spec_version":"1.0","subject":"platform.user.prn_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:user:created-evt_0SNAPSHOT0001","event_type":"platform:iam:user:created","id":"evt_0SNAPSHOT0001","message_group":"platform:user:prn_1","source":"platform:iam","spec_version":"1.0","subject":"platform.user.prn_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_USER_LOGGED_IN: &str = r#"{"aud_logs":{"entity_id":"prn_1","entity_type":"User","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"User"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","correlation_id":"corr-snap","email":"jane@example.com","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:user:logged-in","execution_id":"exec-snap","federatedClaims":{"accessToken":{"aud":"api://default"},"idToken":{"sub":"ext-1"}},"flowcatalystClaims":{"applications":["platform"],"clients":["*"],"email":"jane@example.com","roles":["platform:admin"],"type":"USER"},"identityProviderCode":"okta","loginMethod":"OIDC","message_group":"platform:user:prn_1","principal_id":"prn_actor","source":"platform:iam","spec_version":"1.0","subject":"platform.user.prn_1","time":"2026-01-02T03:04:05Z","userId":"prn_1"},"deduplication_id":"platform:iam:user:logged-in-evt_0SNAPSHOT0001","event_type":"platform:iam:user:logged-in","id":"evt_0SNAPSHOT0001","message_group":"platform:user:prn_1","source":"platform:iam","spec_version":"1.0","subject":"platform.user.prn_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_ROLES_ASSIGNED: &str = r#"{"aud_logs":{"entity_id":"prn_1","entity_type":"User","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"User"}],"correlation_id":"corr-snap","data":{"added":["b"],"causation_id":"evt_parent","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:user:roles-assigned","execution_id":"exec-snap","message_group":"platform:user:prn_1","principalId":"prn_1","principal_id":"prn_actor","removed":["c"],"roles":["a","b"],"source":"platform:iam","spec_version":"1.0","subject":"platform.user.prn_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:user:roles-assigned-evt_0SNAPSHOT0001","event_type":"platform:iam:user:roles-assigned","id":"evt_0SNAPSHOT0001","message_group":"platform:user:prn_1","source":"platform:iam","spec_version":"1.0","subject":"platform.user.prn_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_PROCESS_CREATED: &str = r#"{"aud_logs":{"entity_id":"prc_1","entity_type":"Process","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Process"}],"correlation_id":"corr-snap","data":{"application":"orders","causation_id":"evt_parent","code":"orders:fulfillment:ship","correlation_id":"corr-snap","description":"Ship an order","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:process:created","execution_id":"exec-snap","message_group":"platform:process:prc_1","name":"Ship","principal_id":"prn_actor","processId":"prc_1","processName":"ship","source":"platform:admin","spec_version":"1.0","subdomain":"fulfillment","subject":"platform.process.prc_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:process:created-evt_0SNAPSHOT0001","event_type":"platform:admin:process:created","id":"evt_0SNAPSHOT0001","message_group":"platform:process:prc_1","source":"platform:admin","spec_version":"1.0","subject":"platform.process.prc_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_PROCESS_UPDATED: &str = r#"{"aud_logs":{"entity_id":"prc_1","entity_type":"Process","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Process"}],"correlation_id":"corr-snap","data":{"bodyChanged":true,"causation_id":"evt_parent","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:process:updated","execution_id":"exec-snap","message_group":"platform:process:prc_1","name":"Ship v2","principal_id":"prn_actor","processId":"prc_1","source":"platform:admin","spec_version":"1.0","subject":"platform.process.prc_1","tags":["x","y"],"time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:process:updated-evt_0SNAPSHOT0001","event_type":"platform:admin:process:updated","id":"evt_0SNAPSHOT0001","message_group":"platform:process:prc_1","source":"platform:admin","spec_version":"1.0","subject":"platform.process.prc_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_ROLE_CREATED: &str = r#"{"aud_logs":{"entity_id":"rol_1","entity_type":"Role","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Role"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","code":"orders:viewer","correlation_id":"corr-snap","displayName":"Orders viewer","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:role:created","execution_id":"exec-snap","message_group":"platform:role:rol_1","permissions":["orders:order:read"],"principal_id":"prn_actor","roleId":"rol_1","source":"platform:iam","spec_version":"1.0","subject":"platform.role.rol_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:role:created-evt_0SNAPSHOT0001","event_type":"platform:iam:role:created","id":"evt_0SNAPSHOT0001","message_group":"platform:role:rol_1","source":"platform:iam","spec_version":"1.0","subject":"platform.role.rol_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_SCHEDULED_JOB_CREATED: &str = r#"{"aud_logs":{"entity_id":"sjb_1","entity_type":"Scheduledjob","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Scheduledjob"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","clientId":"clt_1","code":"nightly","concurrent":false,"correlation_id":"corr-snap","crons":["0 0 * * *"],"event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:scheduledjob:created","execution_id":"exec-snap","message_group":"platform:scheduledjob:sjb_1","name":"Nightly","principal_id":"prn_actor","scheduledJobId":"sjb_1","source":"platform:admin","spec_version":"1.0","subject":"platform.scheduledjob.sjb_1","time":"2026-01-02T03:04:05Z","timezone":"UTC","tracksCompletion":true},"deduplication_id":"platform:admin:scheduledjob:created-evt_0SNAPSHOT0001","event_type":"platform:admin:scheduledjob:created","id":"evt_0SNAPSHOT0001","message_group":"platform:scheduledjob:sjb_1","source":"platform:admin","spec_version":"1.0","subject":"platform.scheduledjob.sjb_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_SCHEDULED_JOBS_SYNCED: &str = r#"{"aud_logs":{"entity_id":"synced","entity_type":"Scheduledjobs","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Scheduledjobs"}],"correlation_id":"corr-snap","data":{"archived":[],"causation_id":"evt_parent","correlation_id":"corr-snap","created":["a"],"event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:scheduledjobs:synced","execution_id":"exec-snap","message_group":"platform:scheduledjobs:synced:sync:orders:platform","principal_id":"prn_actor","scope":"orders","source":"platform:admin","spec_version":"1.0","subject":"platform.scheduledjobs.synced.sync:orders:platform","time":"2026-01-02T03:04:05Z","updated":["b"]},"deduplication_id":"platform:admin:scheduledjobs:synced-evt_0SNAPSHOT0001","event_type":"platform:admin:scheduledjobs:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:scheduledjobs:synced:sync:orders:platform","source":"platform:admin","spec_version":"1.0","subject":"platform.scheduledjobs.synced.sync:orders:platform","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_SERVICE_ACCOUNT_CREATED: &str = r#"{"aud_logs":{"entity_id":"sac_1","entity_type":"Serviceaccount","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Serviceaccount"}],"correlation_id":"corr-snap","data":{"applicationId":"app_1","causation_id":"evt_parent","clientIds":["clt_1"],"code":"orders-bot","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:serviceaccount:created","execution_id":"exec-snap","message_group":"platform:serviceaccount:sac_1","name":"Orders bot","principal_id":"prn_actor","serviceAccountId":"sac_1","source":"platform:serviceaccount","spec_version":"1.0","subject":"platform.serviceaccount.sac_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:serviceaccount:created-evt_0SNAPSHOT0001","event_type":"platform:iam:serviceaccount:created","id":"evt_0SNAPSHOT0001","message_group":"platform:serviceaccount:sac_1","source":"platform:serviceaccount","spec_version":"1.0","subject":"platform.serviceaccount.sac_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_SERVICE_ACCOUNT_TOKEN_REGENERATED: &str = r#"{"aud_logs":{"entity_id":"sac_1","entity_type":"Serviceaccount","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Serviceaccount"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","code":"orders-bot","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:serviceaccount:token-regenerated","execution_id":"exec-snap","message_group":"platform:serviceaccount:sac_1","principal_id":"prn_actor","serviceAccountId":"sac_1","source":"platform:serviceaccount","spec_version":"1.0","subject":"platform.serviceaccount.sac_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:serviceaccount:token-regenerated-evt_0SNAPSHOT0001","event_type":"platform:iam:serviceaccount:token-regenerated","id":"evt_0SNAPSHOT0001","message_group":"platform:serviceaccount:sac_1","source":"platform:serviceaccount","spec_version":"1.0","subject":"platform.serviceaccount.sac_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_SERVICE_ACCOUNT_SECRET_REGENERATED: &str = r#"{"aud_logs":{"entity_id":"sac_1","entity_type":"Serviceaccount","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Serviceaccount"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","code":"orders-bot","correlation_id":"corr-snap","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:serviceaccount:secret-regenerated","execution_id":"exec-snap","message_group":"platform:serviceaccount:sac_1","principal_id":"prn_actor","serviceAccountId":"sac_1","source":"platform:serviceaccount","spec_version":"1.0","subject":"platform.serviceaccount.sac_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:serviceaccount:secret-regenerated-evt_0SNAPSHOT0001","event_type":"platform:iam:serviceaccount:secret-regenerated","id":"evt_0SNAPSHOT0001","message_group":"platform:serviceaccount:sac_1","source":"platform:serviceaccount","spec_version":"1.0","subject":"platform.serviceaccount.sac_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_SUBSCRIPTION_CREATED: &str = r#"{"aud_logs":{"entity_id":"sub_1","entity_type":"Subscription","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Subscription"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","clientId":"clt_1","code":"orders-shipped","correlation_id":"corr-snap","endpoint":"https://hooks.example.com/shipped","eventTypes":["orders:fulfillment:shipment:shipped"],"event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:subscription:created","execution_id":"exec-snap","message_group":"platform:admin:subscription:sub_1","name":"Orders shipped","principal_id":"prn_actor","source":"platform:admin","spec_version":"1.0","subject":"platform.subscription.sub_1","subscriptionId":"sub_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:admin:subscription:created-evt_0SNAPSHOT0001","event_type":"platform:admin:subscription:created","id":"evt_0SNAPSHOT0001","message_group":"platform:admin:subscription:sub_1","source":"platform:admin","spec_version":"1.0","subject":"platform.subscription.sub_1","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_PASSKEY_REGISTERED: &str = r#"{"aud_logs":{"entity_id":"pkc_1","entity_type":"Webauthncredential","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Webauthncredential"}],"correlation_id":"corr-snap","data":{"causation_id":"evt_parent","correlation_id":"corr-snap","credentialId":"pkc_1","event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:passkey:registered","execution_id":"exec-snap","message_group":"platform:webauthncredential:pkc_1","name":"YubiKey","principalId":"prn_1","principal_id":"prn_actor","source":"platform:iam","spec_version":"1.0","subject":"platform.webauthncredential.pkc_1","time":"2026-01-02T03:04:05Z"},"deduplication_id":"platform:iam:passkey:registered-evt_0SNAPSHOT0001","event_type":"platform:iam:passkey:registered","id":"evt_0SNAPSHOT0001","message_group":"platform:webauthncredential:pkc_1","source":"platform:iam","spec_version":"1.0","subject":"platform.webauthncredential.pkc_1","time":"2026-01-02T03:04:05Z"}}"#;

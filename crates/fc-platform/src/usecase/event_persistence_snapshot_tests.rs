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
use super::{AuditMasked, DomainEvent, ExecutionContext};

use crate::application::operations::events::{ApplicationClientConfigUpdated, ApplicationCreated};
use crate::application_openapi_spec::operations::events::ApplicationOpenApiSpecSynced;
use crate::auth::operations::events::{AuthConfigCreated, IdpRoleMappingCreated};
use crate::client::operations::events::ClientCreated;
use crate::connection::operations::events::ConnectionCreated;
use crate::cors::operations::events::CorsOriginAdded;
use crate::dispatch_pool::operations::events::DispatchPoolsSynced;
use crate::email_domain_mapping::operations::events::EmailDomainMappingCreated;
use crate::event_type::operations::{EventTypeCreated, EventTypesSynced};
use crate::identity_provider::api::seal_client_secret;
use crate::identity_provider::entity::IdentityProviderType;
use crate::identity_provider::operations::events::{
    IdentityProviderCreated, IdentityProviderUpdated,
};
use crate::identity_provider::operations::{
    CreateIdentityProviderCommand, UpdateIdentityProviderCommand,
};
use crate::platform_config::entity::{ConfigScope, ConfigValueType};
use crate::platform_config::operations::events::{
    PlatformConfigAccessGranted, PlatformConfigPropertySet,
};
use crate::platform_config::operations::SetPlatformConfigPropertyCommand;
use crate::principal::entity::UserScope;
use crate::principal::operations::events::{
    FederatedClaims, FlowcatalystClaims, PrincipalsSynced, RolesAssigned, UserCreated, UserLoggedIn,
};
use crate::process::operations::{ProcessCreated, ProcessUpdated, ProcessesSynced};
use crate::role::operations::events::{RoleCreated, RolesSynced};
use crate::scheduled_job::operations::events::{ScheduledJobCreated, ScheduledJobsSynced};
use crate::service_account::operations::events::{
    ServiceAccountCreated, ServiceAccountSecretRegenerated, ServiceAccountTokenRegenerated,
};
use crate::service_account::operations::{
    CreateServiceAccountCommand, CreateServiceAccountResult, RegenerateAuthTokenCommand,
    RegenerateAuthTokenResult, RegenerateSigningSecretCommand, RegenerateSigningSecretResult,
};
use crate::shared::encryption_service::EncryptionService;
use crate::subscription::operations::events::{SubscriptionCreated, SubscriptionsSynced};
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

impl AuditMasked for SnapshotCommand {}

const CMD: SnapshotCommand = SnapshotCommand {
    target_id: "cmd-target",
};

/// Everything a commit writes for `event`, as one compact JSON string.
fn persisted<E: DomainEvent, C: Serialize + AuditMasked>(event: &E, command: &C) -> String {
    let event_row = EventRow::from_event(event).expect("event row");
    let audit_row = AuditRow::from_event(event, command);
    serde_json::to_string(&serde_json::json!({
        "msg_events": event_row,
        "aud_logs": audit_row,
    }))
    .expect("serialize rows")
}

#[track_caller]
/// Compares as parsed JSON, not strings: the columns are JSONB, so key order
/// is not part of what gets persisted. `Value` equality ignores map order but
/// still checks every key, value and array position.
fn check<E: DomainEvent>(event: &E, expected: &str) {
    let actual = persisted(event, &CMD);
    let actual_json: serde_json::Value =
        serde_json::from_str(&actual).expect("persisted() produced invalid JSON");
    let expected_json: serde_json::Value =
        serde_json::from_str(expected).expect("snapshot constant is invalid JSON");
    assert_eq!(actual_json, expected_json, "\nactual:\n{actual}\n");
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
    let e = fixed!(ApplicationClientConfigUpdated {
        metadata: ApplicationClientConfigUpdated::metadata_for(&ctx(), "app_1"),
        application_id: "app_1".to_string(),
        client_id: "clt_1".to_string(),
        config_id: "acc_1".to_string(),
        enabled: Some(true),
        base_url_override: None,
        config_changed: false,
    });
    check(&e, EXPECTED_APPLICATION_CLIENT_CONFIG_UPDATED);
}

#[test]
fn application_openapi_spec_synced() {
    let e = fixed!(ApplicationOpenApiSpecSynced {
        metadata: ApplicationOpenApiSpecSynced::metadata_for(&ctx(), "app_1", "spec_1"),
        application_id: "app_1".to_string(),
        application_code: "orders".to_string(),
        spec_id: "spec_1".to_string(),
        version: "1.2.0".to_string(),
        spec_hash: "sha256:abc".to_string(),
        archived_prior_version: Some("1.1.0".to_string()),
        has_breaking: true,
        unchanged: false,
    });
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
    let e = fixed!(DispatchPoolsSynced {
        metadata: DispatchPoolsSynced::metadata_for(&ctx(), "orders"),
        application_code: "orders".to_string(),
        created: 2,
        updated: 1,
        deleted: 0,
        synced_codes: s(&["default", "bulk"]),
    });
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
    let e = fixed!(EventTypeCreated {
        metadata: EventTypeCreated::metadata_for(&ctx(), "evt_type_1"),
        event_type_id: "evt_type_1".to_string(),
        code: "orders:fulfillment:shipment:shipped".to_string(),
        name: "Shipment shipped".to_string(),
        description: Some("A shipment left the warehouse".to_string()),
        application: "orders".to_string(),
        subdomain: "fulfillment".to_string(),
        aggregate: "shipment".to_string(),
        event_name: "shipped".to_string(),
        client_id: Some("clt_1".to_string()),
    });
    check(&e, EXPECTED_EVENT_TYPE_CREATED);
}

#[test]
fn event_types_synced() {
    let e = fixed!(EventTypesSynced {
        metadata: EventTypesSynced::metadata_for(&ctx(), "orders"),
        application_code: "orders".to_string(),
        created: 3,
        updated: 2,
        deleted: 1,
        synced_codes: s(&["orders:a:b:c"]),
        schemas_created: 4,
        schemas_updated: 5,
        schemas_unchanged: 6,
    });
    check(&e, EXPECTED_EVENT_TYPES_SYNCED);
}

// ── platform_config ─────────────────────────────────────────────────────────

#[test]
fn platform_config_property_set() {
    let e = fixed!(PlatformConfigPropertySet {
        metadata: PlatformConfigPropertySet::metadata_for(&ctx(), "pcf_1"),
        config_id: "pcf_1".to_string(),
        application_code: "orders".to_string(),
        section: "limits".to_string(),
        property: "max_batch".to_string(),
        scope: "CLIENT".to_string(),
        client_id: Some("clt_1".to_string()),
        value_type: "NUMBER".to_string(),
        was_created: true,
    });
    check(&e, EXPECTED_PLATFORM_CONFIG_PROPERTY_SET);
}

#[test]
fn platform_config_access_granted() {
    let e = fixed!(PlatformConfigAccessGranted {
        metadata: PlatformConfigAccessGranted::metadata_for(&ctx(), "pca_1"),
        access_id: "pca_1".to_string(),
        application_code: "orders".to_string(),
        role_code: "orders:viewer".to_string(),
        can_read: true,
        can_write: false,
        was_created: true,
    });
    check(&e, EXPECTED_PLATFORM_CONFIG_ACCESS_GRANTED);
}

// ── principal ───────────────────────────────────────────────────────────────

/// Mirrors `CreateUserUseCase::execute`.
#[test]
fn user_created() {
    let e = fixed!(UserCreated::new(
        &fresh_ctx(),
        "prn_1",
        "Jane@Example.COM",
        "Jane",
        UserScope::Anchor,
        None
    ));
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
    let e = fixed!(ProcessCreated {
        metadata: ProcessCreated::metadata_for(&ctx(), "prc_1"),
        process_id: "prc_1".to_string(),
        code: "orders:fulfillment:ship".to_string(),
        name: "Ship".to_string(),
        description: Some("Ship an order".to_string()),
        application: "orders".to_string(),
        subdomain: "fulfillment".to_string(),
        process_name: "ship".to_string(),
    });
    check(&e, EXPECTED_PROCESS_CREATED);
}

#[test]
fn process_updated() {
    let e = fixed!(ProcessUpdated {
        metadata: ProcessUpdated::metadata_for(&ctx(), "prc_1"),
        process_id: "prc_1".to_string(),
        name: Some("Ship v2".to_string()),
        description: None,
        body_changed: Some(true),
        tags: Some(s(&["x", "y"])),
    });
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
    let e = fixed!(ScheduledJobCreated {
        metadata: ScheduledJobCreated::metadata_for(&ctx(), "sjb_1"),
        scheduled_job_id: "sjb_1".to_string(),
        client_id: Some("clt_1".to_string()),
        code: "nightly".to_string(),
        name: "Nightly".to_string(),
        crons: s(&["0 0 * * *"]),
        timezone: "UTC".to_string(),
        concurrent: false,
        tracks_completion: true,
    });
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

// ── *Synced summary events ──────────────────────────────────────────────────

#[test]
fn roles_synced() {
    let e = fixed!(RolesSynced {
        metadata: RolesSynced::metadata_for(&ctx(), "orders"),
        application_code: "orders".to_string(),
        created: 3,
        updated: 2,
        deleted: 1,
        synced_names: s(&["orders:viewer"]),
    });
    check(&e, EXPECTED_ROLES_SYNCED);
}

#[test]
fn subscriptions_synced() {
    let e = fixed!(SubscriptionsSynced {
        metadata: SubscriptionsSynced::metadata_for(&ctx(), "orders"),
        application_code: "orders".to_string(),
        created: 3,
        updated: 2,
        deleted: 1,
        synced_codes: s(&["orders-webhook"]),
    });
    check(&e, EXPECTED_SUBSCRIPTIONS_SYNCED);
}

#[test]
fn principals_synced() {
    let e = fixed!(PrincipalsSynced {
        metadata: PrincipalsSynced::metadata_for(&ctx(), "orders"),
        application_code: "orders".to_string(),
        created: 3,
        updated: 2,
        deactivated: 1,
        synced_emails: s(&["a@example.com"]),
    });
    check(&e, EXPECTED_PRINCIPALS_SYNCED);
}

#[test]
fn processes_synced() {
    let e = fixed!(ProcessesSynced {
        metadata: ProcessesSynced::metadata_for(&ctx(), "orders"),
        application_code: "orders".to_string(),
        created: 3,
        updated: 2,
        deleted: 1,
        synced_codes: s(&["orders:fulfillment:ship"]),
    });
    check(&e, EXPECTED_PROCESSES_SYNCED);
}

// ── secrets never reach the persisted rows ─────────────────────────────────
//
// These use the real commands, not `CMD`: the audit row serialises the
// command, so a command that carries a plaintext secret leaks it into
// `aud_logs.operation_json`.

fn test_encryption() -> EncryptionService {
    EncryptionService::new(&EncryptionService::generate_key()).unwrap()
}

/// The `aud_logs.operation_json` column of `persisted(..)` output.
fn audit_json(rows: &str) -> serde_json::Value {
    let json: serde_json::Value = serde_json::from_str(rows).expect("rows are JSON");
    json["aud_logs"]["operation_json"].clone()
}

#[track_caller]
fn assert_no_plaintext(rows: &str, plaintext: &str) {
    assert!(
        !rows.contains(plaintext),
        "plaintext secret reached the persisted rows:\n{rows}"
    );
}

const IDP_SECRET: &str = "idp-plaintext-client-secret";

const OAUTH_CLIENT_SECRET: &str = "oauth-plaintext-client-secret";

#[test]
fn oauth_client_secret_rotation_persists_only_the_hash() {
    let enc = test_encryption();
    let cmd = crate::auth::operations::RotateOAuthClientSecretCommand {
        oauth_client_id: "oac_1".to_string(),
        new_client_secret_ref: enc.hash_secret(OAUTH_CLIENT_SECRET),
        grace_seconds: None,
    };
    assert_eq!(
        enc.verify_secret(&cmd.new_client_secret_ref, OAUTH_CLIENT_SECRET),
        (true, false)
    );

    let e = fixed!(
        crate::auth::operations::events::OAuthClientSecretRotated::new(&ctx(), "oac_1", "client-1")
    );
    let rows = persisted(&e, &cmd);
    assert_no_plaintext(&rows, OAUTH_CLIENT_SECRET);
    // The hash itself no longer reaches the audit row either: the
    // audit-redaction rule masks every `*SecretRef` key.
    assert!(!rows.contains("hashed:v1:"));
    assert_eq!(
        audit_json(&rows)["newClientSecretRef"],
        fc_common::audit_redaction::MASK
    );
}

#[test]
fn identity_provider_create_persists_no_plaintext_secret() {
    let enc = test_encryption();
    let cmd = CreateIdentityProviderCommand {
        code: "okta".to_string(),
        name: "Okta".to_string(),
        idp_type: IdentityProviderType::Oidc,
        oidc_issuer_url: Some("https://okta.example.com".to_string()),
        oidc_client_id: Some("client-1".to_string()),
        oidc_client_secret_ref: seal_client_secret(Some(IDP_SECRET.to_string()), Some(&enc))
            .unwrap(),
        oidc_multi_tenant: false,
        oidc_issuer_pattern: None,
        allowed_email_domains: vec![],
    };
    let stored = cmd.oidc_client_secret_ref.as_deref().unwrap();
    assert_eq!(enc.decrypt_ref(stored).unwrap(), IDP_SECRET);

    let e = fixed!(IdentityProviderCreated::new(
        &ctx(),
        "idp_1",
        "okta",
        "Okta",
        "OIDC"
    ));
    let rows = persisted(&e, &cmd);
    assert_no_plaintext(&rows, IDP_SECRET);
    // Neither the plaintext nor the sealed ref reaches the audit row: the
    // audit-redaction rule masks every `*SecretRef` key.
    assert!(!rows.contains("encrypted:"));
    assert_eq!(
        audit_json(&rows)["oidcClientSecretRef"],
        fc_common::audit_redaction::MASK
    );
}

#[test]
fn identity_provider_update_persists_no_plaintext_secret() {
    let enc = test_encryption();
    let cmd = UpdateIdentityProviderCommand {
        idp_id: "idp_1".to_string(),
        name: None,
        oidc_issuer_url: None,
        oidc_client_id: None,
        oidc_client_secret_ref: seal_client_secret(Some(IDP_SECRET.to_string()), Some(&enc))
            .unwrap(),
        oidc_multi_tenant: None,
        oidc_issuer_pattern: None,
        allowed_email_domains: None,
    };
    let e = fixed!(IdentityProviderUpdated::new(&ctx(), "idp_1", None));
    let rows = persisted(&e, &cmd);
    assert_no_plaintext(&rows, IDP_SECRET);
    // Neither the plaintext nor the sealed ref reaches the audit row: the
    // audit-redaction rule masks every `*SecretRef` key.
    assert!(!rows.contains("encrypted:"));
    assert_eq!(
        audit_json(&rows)["oidcClientSecretRef"],
        fc_common::audit_redaction::MASK
    );
}

#[test]
fn identity_provider_secret_without_key_is_refused() {
    let err = seal_client_secret(Some(IDP_SECRET.to_string()), None).unwrap_err();
    assert!(err.to_string().contains("FLOWCATALYST_APP_KEY"));
    // A 400 with Java's code, not a 500.
    assert!(matches!(
        &err,
        crate::shared::error::PlatformError::Coded { status, code, .. }
            if *status == axum::http::StatusCode::BAD_REQUEST && code == "ENCRYPTION_NOT_CONFIGURED"
    ));
    // Blank means "not provided", which needs no key.
    assert_eq!(
        seal_client_secret(Some("  ".to_string()), None).unwrap(),
        None
    );
    assert_eq!(seal_client_secret(None, None).unwrap(), None);
}

/// The service-account commands carry no credential at all; the generated
/// plaintext lives only in the (non-serialised) result fields.
#[test]
fn service_account_commands_persist_no_generated_credentials() {
    const TOKEN: &str = "fc_generatedtoken";
    const SIGNING: &str = "generated-signing-secret";

    let create = CreateServiceAccountCommand {
        code: "orders-bot".to_string(),
        name: "Orders bot".to_string(),
        description: None,
        scope: Some(UserScope::Client),
        client_ids: s(&["clt_1"]),
        application_id: Some("app_1".to_string()),
    };
    let result = CreateServiceAccountResult {
        event: fixed!(ServiceAccountCreated::new(
            &ctx(),
            "sac_1",
            "orders-bot",
            "Orders bot",
            Some("app_1"),
            s(&["clt_1"])
        )),
        auth_token: TOKEN.to_string(),
        signing_secret: SIGNING.to_string(),
    };
    let rows = persisted(&result, &create);
    assert_no_plaintext(&rows, TOKEN);
    assert_no_plaintext(&rows, SIGNING);

    let regen_token = RegenerateAuthTokenCommand {
        service_account_id: "sac_1".to_string(),
    };
    let result = RegenerateAuthTokenResult {
        event: fixed!(ServiceAccountTokenRegenerated::new(
            &ctx(),
            "sac_1",
            "orders-bot"
        )),
        auth_token: TOKEN.to_string(),
    };
    assert_no_plaintext(&persisted(&result, &regen_token), TOKEN);

    let regen_secret = RegenerateSigningSecretCommand {
        service_account_id: "sac_1".to_string(),
    };
    let result = RegenerateSigningSecretResult {
        event: fixed!(ServiceAccountSecretRegenerated::new(
            &ctx(),
            "sac_1",
            "orders-bot"
        )),
        signing_secret: SIGNING.to_string(),
    };
    assert_no_plaintext(&persisted(&result, &regen_secret), SIGNING);
}

/// Setting a SECRET property: the audit row records the command with the
/// value masked (the command's declared AuditMasked field), and the
/// operation name is unchanged. Only an explicit PLAIN type records the
/// value as sent: an omitted type keeps the property's current type, which
/// may be SECRET (owner spec `docs/spec/audit-redaction.md`, Java repo).
#[test]
fn platform_config_secret_persists_no_plaintext_value() {
    const SECRET: &str = "smtp-plaintext-password";
    let cmd = |value_type: Option<ConfigValueType>| SetPlatformConfigPropertyCommand {
        application_code: "orders".to_string(),
        section: "email".to_string(),
        property: "smtp_host".to_string(),
        value: SECRET.to_string(),
        scope: ConfigScope::Global,
        client_id: None,
        value_type,
        description: None,
    };
    let e = fixed!(PlatformConfigPropertySet {
        metadata: PlatformConfigPropertySet::metadata_for(&ctx(), "pcf_1"),
        config_id: "pcf_1".to_string(),
        application_code: "orders".to_string(),
        section: "email".to_string(),
        property: "smtp_host".to_string(),
        scope: "GLOBAL".to_string(),
        client_id: None,
        value_type: "SECRET".to_string(),
        was_created: false,
    });

    for value_type in [Some(ConfigValueType::Secret), None] {
        let rows = persisted(&e, &cmd(value_type));
        assert_no_plaintext(&rows, SECRET);
        let json: serde_json::Value = serde_json::from_str(&rows).unwrap();
        assert_eq!(
            json["aud_logs"]["operation"],
            "SetPlatformConfigPropertyCommand"
        );
        assert_eq!(json["aud_logs"]["operation_json"]["value"], "***");
    }

    let rows = persisted(&e, &cmd(Some(ConfigValueType::Plain)));
    assert!(rows.contains(SECRET), "a PLAIN value is audited as sent");
    assert_eq!(audit_json(&rows)["valueType"], "PLAIN");
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
const EXPECTED_ROLES_SYNCED: &str = r#"{"aud_logs":{"entity_id":"orders","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","correlation_id":"corr-snap","created":3,"deleted":1,"event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:roles:synced","execution_id":"exec-snap","message_group":"platform:application:orders","principal_id":"prn_actor","source":"platform:iam","spec_version":"1.0","subject":"platform.application.orders","syncedNames":["orders:viewer"],"time":"2026-01-02T03:04:05Z","updated":2},"deduplication_id":"platform:iam:roles:synced-evt_0SNAPSHOT0001","event_type":"platform:iam:roles:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:application:orders","source":"platform:iam","spec_version":"1.0","subject":"platform.application.orders","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_SUBSCRIPTIONS_SYNCED: &str = r#"{"aud_logs":{"entity_id":"orders","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","correlation_id":"corr-snap","created":3,"deleted":1,"event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:subscription:synced","execution_id":"exec-snap","message_group":"platform:application:orders","principal_id":"prn_actor","source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","syncedCodes":["orders-webhook"],"time":"2026-01-02T03:04:05Z","updated":2},"deduplication_id":"platform:admin:subscription:synced-evt_0SNAPSHOT0001","event_type":"platform:admin:subscription:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:application:orders","source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_PRINCIPALS_SYNCED: &str = r#"{"aud_logs":{"entity_id":"orders","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","correlation_id":"corr-snap","created":3,"deactivated":1,"event_id":"evt_0SNAPSHOT0001","event_type":"platform:iam:principals:synced","execution_id":"exec-snap","message_group":"platform:application:orders","principal_id":"prn_actor","source":"platform:iam","spec_version":"1.0","subject":"platform.application.orders","syncedEmails":["a@example.com"],"time":"2026-01-02T03:04:05Z","updated":2},"deduplication_id":"platform:iam:principals:synced-evt_0SNAPSHOT0001","event_type":"platform:iam:principals:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:application:orders","source":"platform:iam","spec_version":"1.0","subject":"platform.application.orders","time":"2026-01-02T03:04:05Z"}}"#;
const EXPECTED_PROCESSES_SYNCED: &str = r#"{"aud_logs":{"entity_id":"orders","entity_type":"Application","operation":"SnapshotCommand","operation_json":{"targetId":"cmd-target"},"performed_at":"2026-01-02T03:04:05Z","principal_id":"prn_actor"},"msg_events":{"causation_id":"evt_parent","context_data":[{"key":"principalId","value":"prn_actor"},{"key":"aggregateType","value":"Application"}],"correlation_id":"corr-snap","data":{"applicationCode":"orders","causation_id":"evt_parent","correlation_id":"corr-snap","created":3,"deleted":1,"event_id":"evt_0SNAPSHOT0001","event_type":"platform:admin:processes:synced","execution_id":"exec-snap","message_group":"platform:application:orders","principal_id":"prn_actor","source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","syncedCodes":["orders:fulfillment:ship"],"time":"2026-01-02T03:04:05Z","updated":2},"deduplication_id":"platform:admin:processes:synced-evt_0SNAPSHOT0001","event_type":"platform:admin:processes:synced","id":"evt_0SNAPSHOT0001","message_group":"platform:application:orders","source":"platform:admin","spec_version":"1.0","subject":"platform.application.orders","time":"2026-01-02T03:04:05Z"}}"#;

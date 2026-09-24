//! Use-case tests that need no database (Java `FunctionOperationsTest`,
//! `FunctionDomainOperationsTest` and `FunctionSettingsApiTest`'s X1
//! cases, for the checks made before anything is loaded): validation codes,
//! the 403s a create or claim makes against the command's own owner, and
//! that a secret's value reaches neither the event nor the audit row.
//!
//! The repositories point at a pool that never connects: every case here
//! must fail (or be inspected) before the use case touches the database,
//! and one that did would fail with a connection error instead.
//! Everything after the load is covered by `tests/function_api_test.rs`.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;

use super::access::tests::caller;
use super::events::SecretSet;
use super::*;
use crate::function::entity::{Function, SecretValue};
use crate::function::{FunctionAddress, FunctionOwner, Runtime};
use crate::usecase::unit_of_work::{AuditRow, InMemoryUnitOfWork};
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError};
use crate::UserScope;

fn ops() -> FunctionOperations<InMemoryUnitOfWork> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(1))
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .unwrap();
    FunctionOperations {
        functions: Arc::new(crate::function::repository::FunctionRepository::new(&pool)),
        applications: Arc::new(crate::ApplicationRepository::new(&pool)),
        clients: Arc::new(crate::ClientRepository::new(&pool)),
        settings: Arc::new(
            crate::function::settings_repository::FunctionSettingsRepository::new(&pool, None),
        ),
        policies: Arc::new(crate::function::policy_repository::ClientPolicyRepository::new(&pool)),
        domains: Arc::new(crate::function::domain_repository::FunctionDomainRepository::new(&pool)),
        routes: Arc::new(crate::function::route_repository::FunctionRouteRepository::new(&pool)),
        trigger_sync: TriggerSync,
        unit_of_work: Arc::new(InMemoryUnitOfWork::new()),
    }
}

fn ctx() -> ExecutionContext {
    ExecutionContext::create("prn_1")
}

fn anchor() -> Caller {
    caller(UserScope::Anchor, &["*"], None)
}

fn address() -> FunctionAddress {
    FunctionAddress::parse("billing.invoices.create").unwrap()
}

async fn run_err<C: UseCase>(use_case: C, command: C::Command) -> UseCaseError {
    match use_case.run(command, ctx()).await.into_result() {
        Ok(_) => panic!("expected a failure"),
        Err(e) => e,
    }
}

fn assert_error(err: &UseCaseError, status: u16, code: &str) {
    assert_eq!(
        (err.http_status_code(), err.code()),
        (status, code),
        "{}",
        err.message()
    );
}

// ── CreateFunction ──────────────────────────────────────────────────────────

fn create_command() -> CreateCommand {
    CreateCommand {
        application_code: Some("billing".into()),
        service_name: Some("invoices".into()),
        name: Some("create".into()),
        runtime: Some("wasm".into()),
        description: None,
        client_id: None,
    }
}

#[tokio::test]
async fn create_validates_before_anything_else() {
    let ops = ops();
    let cases = [
        (
            CreateCommand {
                application_code: Some(" ".into()),
                ..create_command()
            },
            "APPLICATION_CODE_REQUIRED",
        ),
        (
            CreateCommand {
                application_code: None,
                ..create_command()
            },
            "APPLICATION_CODE_REQUIRED",
        ),
        (
            CreateCommand {
                service_name: Some("Invoices".into()),
                ..create_command()
            },
            "LABEL_INVALID",
        ),
        (
            CreateCommand {
                name: None,
                ..create_command()
            },
            "LABEL_INVALID",
        ),
        (
            CreateCommand {
                runtime: Some("python".into()),
                ..create_command()
            },
            "RUNTIME_INVALID",
        ),
    ];
    for (command, code) in cases {
        let err = run_err(ops.create(anchor()), command).await;
        assert_error(&err, 400, code);
    }
    let err = run_err(
        ops.create(anchor()),
        CreateCommand {
            service_name: Some("-x".into()),
            ..create_command()
        },
    )
    .await;
    assert!(err.message().starts_with("serviceName must be a DNS label"));
}

#[tokio::test]
async fn create_for_a_client_out_of_scope_is_403_scope_forbidden() {
    let ops = ops();
    let client = caller(UserScope::Client, &["clt_1"], None);
    let err = run_err(
        ops.create(client.clone()),
        CreateCommand {
            client_id: Some("clt_2".into()),
            ..create_command()
        },
    )
    .await;
    assert_error(&err, 403, "SCOPE_FORBIDDEN");
    // No clientId is a platform function: anchor only.
    let err = run_err(ops.create(client), create_command()).await;
    assert_error(&err, 403, "SCOPE_FORBIDDEN");
    assert_eq!(err.message(), "anchor scope required for this resource");
}

// ── UpdateFunction ──────────────────────────────────────────────────────────

#[test]
fn update_command_audits_the_address_as_a_string_and_omits_absent_fields() {
    let command = UpdateCommand {
        address: address(),
        description: None,
        status: Some("DISABLED".into()),
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({"address": "billing.invoices.create", "status": "DISABLED"})
    );
}

// ── Config ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn config_validation() {
    let ops = ops();
    let too_many: BTreeMap<String, String> =
        (0..101).map(|i| (format!("K{i}"), "v".into())).collect();
    let err = run_err(
        ops.set_config(anchor()),
        SetConfigCommand {
            address: address(),
            values: too_many,
        },
    )
    .await;
    assert_error(&err, 400, "SETTING_TOO_LARGE");
    assert_eq!(
        err.message(),
        "config carries 101 keys, which exceeds the limit of 100"
    );

    let err = run_err(
        ops.set_config(anchor()),
        SetConfigCommand {
            address: address(),
            values: BTreeMap::from([("1BAD".to_string(), "v".to_string())]),
        },
    )
    .await;
    assert_error(&err, 400, "SETTING_KEY_INVALID");

    let err = run_err(
        ops.set_config(anchor()),
        SetConfigCommand {
            address: address(),
            values: BTreeMap::from([("BIG".to_string(), "é".repeat(4097))]),
        },
    )
    .await;
    assert_error(&err, 400, "SETTING_TOO_LARGE");
    assert_eq!(
        err.message(),
        "config['BIG'] is 8194 bytes, which exceeds the limit of 8192"
    );
}

#[tokio::test]
async fn exactly_100_keys_of_8_kib_pass_validation() {
    let values: BTreeMap<String, String> = (0..100)
        .map(|i| (format!("K{i}"), "x".repeat(8192)))
        .collect();
    let command = SetConfigCommand {
        address: address(),
        values,
    };
    assert!(ops().set_config(anchor()).validate(&command).await.is_ok());
}

// ── Secrets ─────────────────────────────────────────────────────────────────

fn secret_command(key: &str, value: &str) -> SetSecretCommand {
    SetSecretCommand {
        address: address(),
        key: key.into(),
        value: SecretValue::new(value),
    }
}

#[tokio::test]
async fn secret_validation() {
    let ops = ops();
    let err = run_err(ops.set_secret(anchor()), secret_command("bad key", "v")).await;
    assert_error(&err, 400, "SETTING_KEY_INVALID");
    let err = run_err(ops.set_secret(anchor()), secret_command("API_KEY", "")).await;
    assert_error(&err, 400, "SETTING_VALUE_REQUIRED");
    assert_eq!(err.message(), "value is required");
    let err = run_err(
        ops.set_secret(anchor()),
        secret_command("API_KEY", &"x".repeat(8193)),
    )
    .await;
    assert_error(&err, 400, "SETTING_TOO_LARGE");
    assert_eq!(
        err.message(),
        "value is 8193 bytes, which exceeds the limit of 8192"
    );

    let err = run_err(
        ops.delete_secret(anchor()),
        DeleteSecretCommand {
            address: address(),
            key: "-nope".into(),
        },
    )
    .await;
    assert_error(&err, 400, "SETTING_KEY_INVALID");
}

/// Java X1: a secret's value is in no event and no audit row, ever.
#[tokio::test]
async fn a_secret_value_reaches_neither_the_event_nor_the_audit_row() {
    const MARKER: &str = "sk_live_MARKER_4242";
    let command = secret_command("API_KEY", MARKER);
    let f = Function::create(
        "app_1",
        address(),
        FunctionOwner::Platform,
        Runtime::Wasm,
        None,
    );
    let event = SecretSet::new(&ctx(), &f, &command.key);

    let event_json = serde_json::to_string(&event).unwrap();
    assert!(!event_json.contains(MARKER), "{event_json}");

    let audit = AuditRow::from_event(&event, &command)
        .operation_json
        .expect("operation_json");
    assert_eq!(
        audit,
        json!({"address": "billing.invoices.create", "key": "API_KEY", "value": "***"})
    );

    // Through a unit of work too, and in every formatting of the command.
    let uow = InMemoryUnitOfWork::new();
    let _ = uow.emit_event(event, &command).await;
    let recorded = uow.committed_audits.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .to_string();
    assert!(!recorded.contains(MARKER), "{recorded}");
    assert!(!format!("{command:?}").contains(MARKER));
    assert!(!serde_json::to_string(&command).unwrap().contains(MARKER));
    assert_eq!(command.value.expose(), MARKER);
}

// ── PutFunctionPolicy ───────────────────────────────────────────────────────

fn signer(issuer: Option<&str>, subject: Option<&str>, runtimes: &[&str]) -> SignerInput {
    SignerInput {
        issuer: issuer.map(str::to_string),
        subject: subject.map(str::to_string),
        runtimes: runtimes.iter().map(|r| r.to_string()).collect(),
    }
}

fn policy(signers: Vec<SignerInput>) -> PutPolicyCommand {
    PutPolicyCommand {
        owner: FunctionOwner::Platform,
        signers,
        max_duration_ms: None,
        max_concurrency: None,
        max_wasm_memory_mb: None,
        max_db_pool_size: None,
    }
}

#[tokio::test]
async fn policy_validation() {
    let ops = ops();
    let cases = [
        (
            policy(vec![signer(Some(" "), Some("s"), &["jvm"])]),
            "SIGNER_INVALID",
        ),
        (
            policy(vec![signer(Some("i"), None, &["jvm"])]),
            "SIGNER_INVALID",
        ),
        (
            policy(vec![signer(Some("i"), Some("s"), &[])]),
            "RUNTIME_INVALID",
        ),
        (
            policy(vec![signer(Some("i"), Some("s"), &["cobol"])]),
            "RUNTIME_INVALID",
        ),
        (
            policy(vec![
                signer(Some("i"), Some("s"), &["jvm"]),
                signer(Some("i"), Some("s"), &["WASM"]),
            ]),
            "SIGNER_DUPLICATE",
        ),
        (
            PutPolicyCommand {
                max_duration_ms: Some(0),
                ..policy(vec![])
            },
            "CEILING_INVALID",
        ),
        (
            PutPolicyCommand {
                max_db_pool_size: Some(-1),
                ..policy(vec![])
            },
            "CEILING_INVALID",
        ),
    ];
    for (command, code) in cases {
        let err = run_err(ops.put_policy(), command).await;
        assert_error(&err, 400, code);
    }
    let err = run_err(
        ops.put_policy(),
        PutPolicyCommand {
            max_wasm_memory_mb: Some(0),
            ..policy(vec![])
        },
    )
    .await;
    assert_eq!(err.message(), "maxWasmMemoryMb must be a positive integer");
    // Runtimes are case-insensitive, as a manifest's are.
    let ok = policy(vec![signer(Some("i"), Some("s"), &["JVM", "wasm"])]);
    assert!(ops.put_policy().validate(&ok).await.is_ok());
}

#[test]
fn policy_command_audits_the_owner_by_its_wire_name() {
    let command = PutPolicyCommand {
        max_duration_ms: Some(9000),
        ..policy(vec![signer(Some("i"), Some("s"), &["jvm"])])
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({"owner": "platform", "signers": [{"issuer": "i", "subject": "s", "runtimes": ["jvm"]}],
               "maxDurationMs": 9000})
    );
}

// ── Domains ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn claim_validation_and_scope() {
    let ops = ops();
    for hostname in [
        None,
        Some("localhost"),
        Some("*.acme.com"),
        Some("acme.com."),
    ] {
        let err = run_err(
            ops.claim_domain(anchor()),
            ClaimCommand {
                owner: FunctionOwner::Platform,
                hostname: hostname.map(str::to_string),
            },
        )
        .await;
        assert_error(&err, 400, "HOSTNAME_INVALID");
    }
    let client = caller(UserScope::Client, &["clt_1"], None);
    let err = run_err(
        ops.claim_domain(client.clone()),
        ClaimCommand {
            owner: FunctionOwner::Client("clt_2".into()),
            hostname: Some("acme.com".into()),
        },
    )
    .await;
    assert_error(&err, 403, "SCOPE_FORBIDDEN");
    let err = run_err(
        ops.claim_domain(client),
        ClaimCommand {
            owner: FunctionOwner::Platform,
            hostname: Some("acme.com".into()),
        },
    )
    .await;
    assert_error(&err, 403, "SCOPE_FORBIDDEN");
}

#[tokio::test]
async fn release_of_an_invalid_hostname_is_400_before_any_load() {
    let err = run_err(
        ops().release_domain(anchor()),
        ReleaseCommand {
            hostname: "not a host".into(),
        },
    )
    .await;
    assert_error(&err, 400, "HOSTNAME_INVALID");
}

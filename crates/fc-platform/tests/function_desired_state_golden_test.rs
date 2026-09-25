//! The desired-state document against what Java's own code writes
//! (`tests/data/function/desired-state-golden.json`, written by
//! `tests/java/…/operations/DesiredStateGoldenGen.java`: Java's
//! `DesiredState.build` over a Java-migrated database loaded with
//! `desired-state-fixture.sql`, with the classes changed since the pin
//! `0118cdca` rebuilt from the pinned sources).
//!
//! The same fixture rows, loaded into a Rust-migrated database, must give
//! the same document for every pool (compared as JSON values: key order and
//! number spelling are not part of it, and the `ETag` is opaque to the
//! hosts, so it need not be Java's), and the same bytes on every build:
//! live, candidate
//! and alias-only entries, warm and lazy, signer present and absent, the
//! webhook signing secret (oldest active account; none without an account
//! or with a blank secret), platform and client owners, declared-only
//! config and secrets with `missingSettings`, `db[].secretRef`, JSON
//! escaping, `unload` (deduplicated, sorted, the live window's edge
//! included, a stale host ignored), `publicRoutes` with alias prefixes, a
//! corrupt candidate skipped, and a corrupt live version failing only its
//! own pool. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;

use fc_platform::function::desired_state::{etag, DesiredStateBuilder};
use fc_platform::function::settings_repository::FunctionSettingsRepository;
use fc_platform::function::DnsLabel;
use fc_platform::service_account::outbound_credentials::OutboundCredentialsResolver;
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::shared::error::PlatformError;
use support::TestApp;

fn data(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/function")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

async fn builder(app: &TestApp, app_key: &str) -> DesiredStateBuilder {
    sqlx::raw_sql(&data("desired-state-fixture.sql"))
        .execute(&app.pool)
        .await
        .expect("fixture");
    let encryption = Some(Arc::new(EncryptionService::new(app_key).unwrap()));
    DesiredStateBuilder {
        functions: app.repos.function_repo.clone(),
        versions: app.repos.function_version_repo.clone(),
        hosts: app.repos.function_host_repo.clone(),
        settings: Arc::new(FunctionSettingsRepository::new(
            &app.pool,
            encryption.clone(),
        )),
        routes: app.repos.function_route_repo.clone(),
        credentials: Arc::new(OutboundCredentialsResolver::new(
            app.repos.service_account_repo.clone(),
            encryption,
        )),
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn every_pools_document_is_javas() {
    let golden: Value = serde_json::from_str(&data("desired-state-golden.json")).unwrap();
    let app = TestApp::setup().await;
    let desired = builder(&app, golden["appKey"].as_str().unwrap()).await;
    let now: DateTime<Utc> = golden["now"].as_str().unwrap().parse().unwrap();

    let pools = golden["pools"].as_object().unwrap();
    assert_eq!(pools.len(), 4);
    for (pool, expected) in pools {
        let label = DnsLabel::parse("pool", pool).unwrap();
        let built = desired.build(&label, now).await;
        match expected["status"].as_u64().unwrap() {
            200 => {
                let document = built.unwrap_or_else(|e| panic!("{pool}: {e:?}"));
                let body = document.to_bytes();
                let ours: Value = serde_json::from_slice(&body).unwrap();
                let java: Value = serde_json::from_str(expected["body"].as_str().unwrap()).unwrap();
                assert_eq!(ours, java, "{pool}: the document");
                // The same state, built again: the same bytes and so the same
                // ETag (spec §8 P14).
                let again = desired.build(&label, now).await.unwrap().to_bytes();
                assert_eq!(etag(&again), etag(&body), "{pool}");
            }
            500 => match built {
                Err(PlatformError::Coded {
                    status,
                    code,
                    message,
                    ..
                }) => {
                    assert_eq!(status.as_u16(), 500, "{pool}");
                    assert_eq!(code, expected["error"].as_str().unwrap(), "{pool}");
                    assert!(
                        message.starts_with(&format!(
                            "function version {} has a corrupt row: ",
                            expected["rowId"].as_str().unwrap()
                        )),
                        "{pool}: {message}"
                    );
                }
                other => panic!("{pool}: expected CORRUPT_ROW, got {other:?}"),
            },
            other => panic!("{pool}: unexpected golden status {other}"),
        }
    }
}

/// A corrupt live version whose pool cannot even be peeked fails every
/// pool's build: it cannot be ruled out of any (Java
/// `aCorruptLiveVersionWhosePoolCannotEvenBeReadFailsEveryPoolsBuild`).
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_corrupt_live_version_with_no_readable_pool_fails_every_pool() {
    let golden: Value = serde_json::from_str(&data("desired-state-golden.json")).unwrap();
    let app = TestApp::setup().await;
    let desired = builder(&app, golden["appKey"].as_str().unwrap()).await;
    sqlx::raw_sql("UPDATE fn_versions SET manifest = '[]'::jsonb WHERE id = 'fnv_F09V1'")
        .execute(&app.pool)
        .await
        .unwrap();
    for pool in ["edge", "batch", "other"] {
        let label = DnsLabel::parse("pool", pool).unwrap();
        match desired.build(&label, Utc::now()).await {
            Err(PlatformError::Coded { code, message, .. }) => {
                assert_eq!(code, "CORRUPT_ROW", "{pool}");
                assert!(message.contains("fnv_F09V1"), "{message}");
            }
            other => panic!("{pool}: expected CORRUPT_ROW, got {other:?}"),
        }
    }
}

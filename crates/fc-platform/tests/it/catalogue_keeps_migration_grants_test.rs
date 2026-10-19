//! The permission catalogue must contain every grant a migration adds to a
//! built-in role. `POST /bff/roles/sync-platform` (and the boot sync) makes an
//! existing `CODE` role's permissions equal the catalogue's, so a grant that
//! only a migration knows about (Rust 091 / 092, Go 079 / 080: the F19 pool
//! sync and the six F20 admin-page permissions of `platform:application-service`)
//! is stripped by the next sync. Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::role::entity::roles;
use support::{read_json, TestApp};

const MIGRATION_091: &str =
    include_str!("../../../../migrations/091_go_application_service_dispatch_pool_sync.sql");
const MIGRATION_092: &str =
    include_str!("../../../../migrations/092_go_application_service_admin_pages.sql");

/// The permission literals a migration file grants (`'platform:...'`).
fn granted(sql: &str) -> BTreeSet<String> {
    sql.lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .flat_map(|l| l.split('\'').skip(1).step_by(2))
        .filter(|s| s.starts_with("platform:") && s.matches(':').count() == 3)
        .map(str::to_string)
        .collect()
}

#[test]
fn the_catalogue_holds_what_migrations_091_and_092_grant() {
    let mut expected = granted(MIGRATION_091);
    expected.extend(granted(MIGRATION_092));
    assert_eq!(expected.len(), 7, "{expected:?}");
    let role = roles::application_service();
    for permission in &expected {
        assert!(
            role.permissions.contains(permission),
            "catalogue application-service lacks {permission}, which a migration grants"
        );
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sync_platform_keeps_the_grants_the_migrations_added() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;

    // A deployment that existed before the grants: the stored role holds the
    // old catalogue only; then the migrations' grant statements run.
    let mut expected = granted(MIGRATION_091);
    expected.extend(granted(MIGRATION_092));
    let mut old = roles::application_service();
    for p in &expected {
        old.permissions.remove(p);
    }
    app.repos.role_repo.insert(&old).await.expect("insert role");
    for sql in [MIGRATION_091, MIGRATION_092] {
        sqlx::raw_sql(sql)
            .execute(&app.pool)
            .await
            .expect("run the migration's grants");
    }

    let stored = || async {
        let role = app
            .repos
            .role_repo
            .find_by_name("platform:application-service")
            .await
            .unwrap()
            .expect("role");
        role.permissions.into_iter().collect::<BTreeSet<_>>()
    };
    let before = stored().await;
    for p in &expected {
        assert!(before.contains(p), "migration did not grant {p}");
    }

    let (status, body): (StatusCode, Value) = read_json(
        app.post("/bff/roles/sync-platform", &admin, json!({}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let after = stored().await;
    for p in &expected {
        assert!(after.contains(p), "sync-platform stripped {p}");
    }
    assert_eq!(
        after,
        roles::application_service()
            .permissions
            .into_iter()
            .collect(),
        "the stored role equals the catalogue"
    );
}

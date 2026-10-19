//! The dispatch-job write actions as their own permissions (Go `ad37de21`,
//! `seed/roles_test.go` and `migrate/dispatch_job_write_permissions_pg_test.go`):
//! the built-in role catalogue and migration 095 (Go 085). Requires a Postgres
//! for the migration test (see `support::db`).

use crate::support;

use std::collections::BTreeSet;
use std::iter::once;

use fc_platform::role::entity::{roles, AuthRole};
use support::TestApp;

const MIGRATION_095: &str =
    include_str!("../../../../migrations/095_go_dispatch_job_write_permissions.sql");

const VIEW: &str = "platform:messaging:dispatch-job:view";
const REDRIVE: &str = "platform:messaging:dispatch-job:redrive-completed";
const WRITES: [&str; 4] = [
    "platform:messaging:dispatch-job:requeue",
    "platform:messaging:dispatch-job:cancel",
    "platform:messaging:dispatch-job:complete",
    "platform:messaging:dispatch-job:edit-payload",
];

/// The built-in messaging-admin role holds the four ordinary write permissions;
/// redrive-completed (re-delivering jobs that already succeeded) is held by no
/// built-in role but super-admin's wildcard; the read-only roles hold none.
#[test]
fn the_built_in_roles_hold_the_write_actions_as_go_seeds_them() {
    let all = roles::all();
    let mut seen_messaging_admin = false;
    for role in &all {
        assert!(
            !role.permissions.contains(REDRIVE),
            "role {} holds {REDRIVE}; no built-in role should",
            role.name
        );
        match role.name.as_str() {
            "platform:messaging-admin" => {
                seen_messaging_admin = true;
                for p in once(VIEW).chain(WRITES) {
                    assert!(role.permissions.contains(p), "messaging-admin lacks {p}");
                }
            }
            "platform:viewer" | "platform:admin-readonly" | "platform:application-service" => {
                for p in WRITES {
                    assert!(
                        !role.permissions.contains(p),
                        "{} must not hold {p}",
                        role.name
                    );
                }
            }
            _ => {}
        }
    }
    assert!(seen_messaging_admin);
}

/// Migration 095: every role that holds `dispatch-job:view` is granted the four
/// ordinary write permissions so nobody loses an ability when the single view
/// permission is split; `platform:viewer` (read-only by intent) is not; roles
/// without view get nothing; redrive-completed is granted to no one; running it
/// twice is a no-op.
#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_095_grants_the_writes_to_view_holders_once() {
    let app = TestApp::setup().await;
    let roles_under_test: [(&str, &[&str]); 6] = [
        (
            "platform:messaging-admin",
            &[
                VIEW,
                "platform:messaging:dispatch-job:view-raw",
                "platform:messaging:event:view",
            ],
        ),
        ("platform:viewer", &[VIEW, "platform:messaging:event:view"]),
        ("custom:ops", &[VIEW]),
        ("shop:support", &[VIEW, "shop:orders:order:view"]),
        ("platform:iam-admin", &["platform:iam:user:view"]),
        ("platform:super-admin", &["platform:*:*:*"]),
    ];
    // The test database already holds the built-in roles (the app seeds them
    // at start); start from a known state for the ones under test.
    for (name, held) in roles_under_test {
        if let Some(existing) = app.repos.role_repo.find_by_name(name).await.unwrap() {
            app.repos.role_repo.delete(&existing.id).await.unwrap();
        }
        let (application, code) = name.split_once(':').unwrap();
        let mut role = AuthRole::new(application, code, name);
        for p in held {
            role = role.with_permission(*p);
        }
        app.repos.role_repo.insert(&role).await.unwrap();
    }
    let perms = |name: &'static str| {
        let app = &app;
        async move {
            app.repos
                .role_repo
                .find_by_name(name)
                .await
                .unwrap()
                .expect("role")
                .permissions
                .into_iter()
                .collect::<BTreeSet<String>>()
        }
    };
    let mut before = Vec::new();
    for (name, _) in roles_under_test {
        before.push((name, perms(name).await));
    }

    for _ in 0..2 {
        sqlx::raw_sql(MIGRATION_095)
            .execute(&app.pool)
            .await
            .expect("run migration 095");
    }

    let with_writes = |held: &BTreeSet<String>| {
        let mut v = held.clone();
        v.extend(WRITES.map(str::to_string));
        v
    };
    for (name, held) in &before {
        let now = perms(name).await;
        match *name {
            "platform:messaging-admin" | "custom:ops" | "shop:support" => {
                assert_eq!(
                    now,
                    with_writes(held),
                    "{name} keeps what it had and gains the four writes"
                );
            }
            // The read-only built-in gains nothing; neither does a role without view.
            _ => assert_eq!(&now, held, "{name} is unchanged"),
        }
    }
    let (redrive,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM iam_role_permissions WHERE permission = $1")
            .bind(REDRIVE)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(redrive, 0, "redrive-completed is granted to no one");
}

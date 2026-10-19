//! The last all-applications administrator guard and the self-restriction
//! confirmation (Go `principal/lastadmin/lastadmin_pg_test.go` and
//! `principal/api/self_restriction_pg_test.go`, commits `9966a2fa`, `01fead7e`).
//!
//! An all-applications administrator is an ACTIVE USER of tier ANCHOR with the
//! all-applications flag. No write may leave none: assign application access,
//! deactivate, update, delete, scope change and platform user sync all answer
//! `409 LAST_ALL_APPLICATIONS_ADMIN`. Each test owns its database, so "the
//! administrators" are only the ones the test creates (the callers' tokens are
//! not stored, so they count for nothing).

use crate::support;

use axum::http::StatusCode;
use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform_core::shared::id::PrincipalId;
use serde_json::{json, Value};
use std::time::Duration;
use support::{read_json, TestApp};
use tokio::time::timeout;

const LAST_ADMIN_LOCK_KEY: i64 = 0x4643_4144_4d49_4e01;

struct Fx {
    app: TestApp,
    /// A caller that is stored nowhere: it holds ADMIN_ALL and is an anchor.
    token: String,
}

impl Fx {
    async fn new() -> Self {
        let app = TestApp::setup().await;
        let token = app.anchor_admin_token().await;
        Self { app, token }
    }

    /// An active ANCHOR USER with all_applications = true.
    async fn admin(&self, name: &str) -> PrincipalId {
        let user = Principal::new_user(format!("{name}@lastadmin.test"), UserScope::Anchor);
        assert!(user.is_all_applications_admin());
        self.app.repos.principal_repo.insert(&user).await.unwrap();
        user.id
    }

    async fn assign(&self, id: &PrincipalId, all_apps: Option<bool>) -> (StatusCode, Value) {
        let mut body = json!({ "applicationIds": [] });
        if let Some(flag) = all_apps {
            body["allApplications"] = json!(flag);
        }
        read_json(
            self.app
                .put(
                    &format!("/api/principals/{id}/application-access"),
                    &self.token,
                    body,
                )
                .await,
        )
        .await
    }

    async fn post(&self, path: &str) -> (StatusCode, Value) {
        read_json(self.app.post(path, &self.token, json!({})).await).await
    }

    async fn put(&self, path: &str, body: Value) -> (StatusCode, Value) {
        read_json(self.app.put(path, &self.token, body).await).await
    }

    async fn delete(&self, id: &PrincipalId) -> (StatusCode, Value) {
        read_json(
            self.app
                .delete(&format!("/api/principals/{id}"), &self.token)
                .await,
        )
        .await
    }

    async fn is_admin(&self, id: &PrincipalId) -> bool {
        self.app
            .repos
            .principal_repo
            .find_by_id(id)
            .await
            .unwrap()
            .expect("principal")
            .is_all_applications_admin()
    }

    async fn admin_count(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM iam_principals
              WHERE type = 'USER' AND scope = 'ANCHOR' AND active AND all_applications",
        )
        .fetch_one(&self.app.pool)
        .await
        .unwrap()
    }
}

#[track_caller]
fn require_last_admin(got: &(StatusCode, Value)) {
    assert_eq!(got.0, StatusCode::CONFLICT, "{}", got.1);
    assert_eq!(got.1["error"], "LAST_ALL_APPLICATIONS_ADMIN", "{}", got.1);
    assert!(
        got.1["message"]
            .as_str()
            .unwrap()
            .contains("Grant all-applications access to another"),
        "{}",
        got.1
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn assigning_away_the_last_admin_is_refused_until_another_exists() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;
    require_last_admin(&fx.assign(&a, Some(false)).await);
    assert!(
        fx.is_admin(&a).await,
        "a refused write leaves the admin intact"
    );

    let b = fx.admin("b").await;
    let (status, body) = fx.assign(&a, Some(false)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "another active admin exists: {body}"
    );
    // b is now the last one.
    require_last_admin(&fx.assign(&b, Some(false)).await);
    assert!(fx.is_admin(&b).await);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_omitted_flag_and_a_grant_are_unaffected() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;
    let (status, body) = fx.assign(&a, None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an omitted flag leaves the admin intact: {body}"
    );
    assert!(fx.is_admin(&a).await);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_service_principal_does_not_count() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;
    // An anchor SERVICE principal with all_applications = TRUE (the default).
    let mut svc = Principal::new_service("sa_lastadmin", "svc", UserScope::Anchor);
    svc.all_applications = true;
    assert!(!svc.is_all_applications_admin());
    fx.app.repos.principal_repo.insert(&svc).await.unwrap();
    require_last_admin(&fx.assign(&a, Some(false)).await);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_inactive_admin_does_not_count() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;
    let b = fx.admin("b").await;
    // Deactivating b is allowed (a remains); then a is the last ACTIVE admin
    // even though b still carries all_applications = true.
    let (status, body) = fx.post(&format!("/api/principals/{b}/deactivate")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    require_last_admin(&fx.assign(&a, Some(false)).await);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn deactivate_update_and_delete_are_refused_for_the_last_admin() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;

    require_last_admin(&fx.post(&format!("/api/principals/{a}/deactivate")).await);
    require_last_admin(
        &fx.put(&format!("/api/principals/{a}"), json!({ "active": false }))
            .await,
    );
    require_last_admin(&fx.delete(&a).await);
    assert!(fx.is_admin(&a).await);

    // A harmless update (name only) is fine.
    let (status, body) = fx
        .put(
            &format!("/api/principals/{a}"),
            json!({ "name": "Renamed" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // With a second admin all three succeed.
    let b = fx.admin("b").await;
    let (status, body) = fx.post(&format!("/api/principals/{a}/deactivate")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx.post(&format!("/api/principals/{a}/activate")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx.delete(&a).await;
    assert!(status.is_success(), "{status} {body}");
    assert!(fx.is_admin(&b).await);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_scope_change_is_refused_for_the_last_admin() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;
    let client = Client::new("LA Client", "la-client");
    fx.app.repos.client_repo.insert(&client).await.unwrap();
    for mode in ["CHANGE_CLIENT", "TO_PARTNER"] {
        let got = fx
            .put(
                &format!("/api/principals/{a}/client-association"),
                json!({ "clientId": client.id.as_str(), "mode": mode }),
            )
            .await;
        require_last_admin(&got);
    }
    assert!(fx.is_admin(&a).await);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_user_sync_deactivating_the_last_admin_is_refused() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;
    let got = read_json(
        fx.app
            .post(
                "/api/principals/sync",
                &fx.token,
                json!({ "principals": [{ "email": "a@lastadmin.test", "name": "A", "active": false }] }),
            )
            .await,
    )
    .await;
    require_last_admin(&got);
    assert!(fx.is_admin(&a).await);

    // With another administrator the same sync goes through.
    let _b = fx.admin("b").await;
    let (status, body) = read_json(
        fx.app
            .post(
                "/api/principals/sync",
                &fx.token,
                json!({ "principals": [{ "email": "a@lastadmin.test", "name": "A", "active": false }] }),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!fx.is_admin(&a).await);
}

/// Two real requests (two pooled connections) that each demote the OTHER
/// administrator. Repeated to give an unguarded implementation every chance to
/// let both succeed.
#[tokio::test]
#[ignore = "requires Docker"]
async fn concurrent_mutual_demotion_lets_only_one_win() {
    let fx = Fx::new().await;
    for i in 0..15 {
        // Back to a known state: no administrators (raw SQL bypasses the guard).
        sqlx::query(
            "UPDATE iam_principals SET all_applications = FALSE
              WHERE type = 'USER' AND scope = 'ANCHOR' AND all_applications",
        )
        .execute(&fx.app.pool)
        .await
        .unwrap();
        let a = fx.admin(&format!("ca{i}")).await;
        let b = fx.admin(&format!("cb{i}")).await;
        let (ra, rb) = tokio::join!(fx.assign(&a, Some(false)), fx.assign(&b, Some(false)));
        let wins = [&ra, &rb].iter().filter(|r| r.0 == StatusCode::OK).count();
        for r in [&ra, &rb] {
            if r.0 != StatusCode::OK {
                require_last_admin(r);
            }
        }
        assert_eq!(
            wins, 1,
            "iteration {i}: exactly one demotion may win ({ra:?} {rb:?})"
        );
        assert_eq!(
            fx.admin_count().await,
            1,
            "iteration {i}: one administrator must remain"
        );
    }
}

/// The deterministic form of the race: while another transaction holds the
/// guard's advisory lock, a demotion of the last admin must BLOCK, and be
/// refused once the lock is released with the administrator still the last.
/// A guard that never takes the lock would answer at once.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_guard_takes_the_advisory_lock() {
    let fx = Fx::new().await;
    let a = fx.admin("a").await;

    let mut holder = fx.app.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(LAST_ADMIN_LOCK_KEY)
        .execute(&mut *holder)
        .await
        .unwrap();

    let blocked = fx.assign(&a, Some(false));
    tokio::pin!(blocked);
    let early = timeout(Duration::from_millis(500), &mut blocked).await;
    assert!(
        early.is_err(),
        "the demotion must wait for the lock, answered: {early:?}"
    );

    holder.commit().await.unwrap();
    let got = timeout(Duration::from_secs(10), blocked)
        .await
        .expect("the demotion never unblocked");
    require_last_admin(&got);
}

/// Removing your OWN all-applications flag needs `confirmSelfRestriction`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn removing_your_own_flag_needs_confirmation() {
    let fx = Fx::new().await;
    let me = {
        let mut user = Principal::new_user("me@selfrestr.test", UserScope::Anchor);
        user.roles = vec![RoleAssignment::new("platform:test-admin")];
        fx.app.repos.principal_repo.insert(&user).await.unwrap();
        user
    };
    // The role the token's authority comes from.
    let _ = fx.app.anchor_admin_token().await;
    let other = fx.admin("other").await;
    let my_token = fx.app.auth_service.generate_access_token(&me).unwrap();
    let call = |id: PrincipalId, body: Value, token: String| {
        let app = &fx.app;
        async move {
            read_json(
                app.put(
                    &format!("/api/principals/{id}/application-access"),
                    &token,
                    body,
                )
                .await,
            )
            .await
        }
    };

    for confirm in [None, Some(false)] {
        let mut body = json!({ "applicationIds": [], "allApplications": false });
        if let Some(c) = confirm {
            body["confirmSelfRestriction"] = json!(c);
        }
        let (status, err) = call(me.id.clone(), body, my_token.clone()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{confirm:?}: {err}");
        assert_eq!(
            err["error"], "SELF_RESTRICTION_REQUIRES_CONFIRMATION",
            "{err}"
        );
        assert!(
            err["message"]
                .as_str()
                .unwrap()
                .contains("no longer be able to grant all-applications access"),
            "{err}"
        );
        assert!(
            fx.is_admin(&me.id).await,
            "an unconfirmed request changes nothing"
        );
    }

    // Restricting SOMEONE ELSE needs no confirmation.
    let (status, body) = call(
        other.clone(),
        json!({ "applicationIds": [], "allApplications": false }),
        my_token.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!fx.is_admin(&other).await);

    // A self edit that leaves the flag alone, or keeps it true, needs none.
    for body in [
        json!({ "applicationIds": [] }),
        json!({ "applicationIds": [], "allApplications": true }),
    ] {
        let (status, got) = call(me.id.clone(), body, my_token.clone()).await;
        assert_eq!(status, StatusCode::OK, "{got}");
    }
    assert!(fx.is_admin(&me.id).await);

    // Confirmed, and another administrator exists: succeeds.
    let _third = fx.admin("third").await;
    let (status, body) = call(
        me.id.clone(),
        json!({ "applicationIds": [], "allApplications": false, "confirmSelfRestriction": true }),
        my_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!fx.is_admin(&me.id).await);
}

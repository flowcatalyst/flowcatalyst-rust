//! Dispatch-pool ownership (Go F19, `489f52c3`; ported from Go's
//! `sdksync/sync_dispatch_pools_pg_test.go`). The built-in
//! `platform:application-service` role carries
//! `platform:messaging:dispatch-pool:sync`, so an application's deploy can sync
//! its own pools. Pools are global rows matched by code, so the grant is safe
//! only with ownership: an application may create pools and change the pools it
//! owns, never another application's, a platform pool or a client's. Requires
//! Docker (or `FC_TEST_PG_BIN`).

use crate::application_scope_test::{create_app, token_for};
use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::shared::id::ClientId;
use fc_platform::DispatchPool;
use support::{read_json, TestApp};

/// An application service account bound to `apps`, holding exactly the
/// built-in application-service role's permissions.
async fn app_sa(app: &TestApp, name: &str, scope: UserScope, apps: &[&Application]) -> String {
    let mut p = Principal::new_service(name, name, scope);
    if let Some(first) = apps.first() {
        p = p.with_application_id(first.id.clone());
    }
    token_for(app, p, apps).await
}

async fn sync(
    app: &TestApp,
    token: &str,
    app_code: &str,
    remove: bool,
    codes: &[&str],
) -> (StatusCode, Value) {
    let pools: Vec<Value> = codes
        .iter()
        .map(|c| json!({ "code": c, "name": format!("name of {c}"), "concurrency": 3 }))
        .collect();
    let path = if remove {
        format!("/api/applications/{app_code}/dispatch-pools/sync?removeUnlisted=true")
    } else {
        format!("/api/applications/{app_code}/dispatch-pools/sync")
    };
    read_json(app.post(&path, token, json!({ "pools": pools })).await).await
}

/// `(application_id, status, name, concurrency)` of the client-less pool.
async fn pool(app: &TestApp, code: &str) -> Option<(Option<String>, String, String, i32)> {
    sqlx::query_as(
        "SELECT application_id, status, name, concurrency FROM msg_dispatch_pools \
         WHERE code = $1 AND client_id IS NULL",
    )
    .bind(code)
    .fetch_optional(&app.pool)
    .await
    .expect("read pool")
}

fn not_owned(label: &str, (status, body): &(StatusCode, Value)) {
    assert_eq!(*status, StatusCode::FORBIDDEN, "{label}: {body}");
    assert_eq!(body["error"], "DISPATCH_POOL_NOT_OWNED", "{label}: {body}");
}

async fn unowned_pool(app: &TestApp, code: &str) {
    app.repos
        .dispatch_pool_repo
        .insert(&DispatchPool::new(code, "Platform pool"))
        .await
        .expect("insert pool");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_application_service_role_may_sync_pools_and_owns_what_it_creates() {
    let app = TestApp::setup().await;
    let own = create_app(&app, "dpsown").await;
    let sa = app_sa(&app, "dps_own_sa", UserScope::Anchor, &[&own]).await;

    let (status, body) = sync(&app, &sa, "dpsown", false, &["dpsown-high", "dpsown-low"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["created"], 2, "{body}");
    for code in ["dpsown-high", "dpsown-low"] {
        let (owner, ..) = pool(&app, code).await.expect("created");
        assert_eq!(owner.as_deref(), Some(own.id.as_str()), "{code} owned");
    }

    // A re-sync (an update of pools it created) works too.
    let (status, body) = sync(&app, &sa, "dpsown", false, &["dpsown-high"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["updated"], 1, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn another_application_is_refused_and_nothing_is_written() {
    let app = TestApp::setup().await;
    let mine = create_app(&app, "dpsmine").await;
    let _theirs = create_app(&app, "dpstheirs").await;
    let sa = app_sa(&app, "dps_mine_sa", UserScope::Anchor, &[&mine]).await;

    let (status, body) = sync(&app, &sa, "dpstheirs", false, &["dpstheirs-new"]).await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
        "{status}: {body}"
    );
    assert!(pool(&app, "dpstheirs-new").await.is_none());

    // Bound to nothing: the same refusal.
    let unbound = app_sa(&app, "dps_unbound_sa", UserScope::Anchor, &[]).await;
    let (status, _) = sync(&app, &unbound, "dpsmine", false, &["dpsmine-x"]).await;
    assert!(status.is_client_error(), "{status}");
    assert!(pool(&app, "dpsmine-x").await.is_none());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_application_cannot_take_over_a_pool_it_does_not_own() {
    let app = TestApp::setup().await;
    let a = create_app(&app, "dpstakea").await;
    let b = create_app(&app, "dpstakeb").await;
    let sa_a = app_sa(&app, "dps_take_a", UserScope::Anchor, &[&a]).await;
    let sa_b = app_sa(&app, "dps_take_b", UserScope::Anchor, &[&b]).await;

    let (status, body) = sync(&app, &sa_b, "dpstakeb", false, &["dpstakeb-pool"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    unowned_pool(&app, "dpstake-platform").await;

    for code in ["dpstakeb-pool", "dpstake-platform"] {
        let before = pool(&app, code).await.unwrap();
        let resp = sync(&app, &sa_a, "dpstakea", false, &[code]).await;
        not_owned(code, &resp);
        assert_eq!(pool(&app, code).await.unwrap(), before, "{code} untouched");
    }

    // One owned code mixed with a foreign one: all-or-nothing.
    let resp = sync(
        &app,
        &sa_a,
        "dpstakea",
        false,
        &["dpstakea-fresh", "dpstakeb-pool"],
    )
    .await;
    not_owned("mixed", &resp);
    assert!(pool(&app, "dpstakea-fresh").await.is_none());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_sweep_archives_only_the_pools_the_application_owns() {
    let app = TestApp::setup().await;
    let mine = create_app(&app, "dpsarchmine").await;
    let theirs = create_app(&app, "dpsarchtheirs").await;
    let sa = app_sa(&app, "dps_arch_mine", UserScope::Anchor, &[&mine]).await;
    let sa_theirs = app_sa(&app, "dps_arch_theirs", UserScope::Anchor, &[&theirs]).await;

    assert_eq!(
        sync(
            &app,
            &sa_theirs,
            "dpsarchtheirs",
            false,
            &["dpsarchtheirs-pool"]
        )
        .await
        .0,
        StatusCode::OK
    );
    unowned_pool(&app, "dpsarch-platform").await;
    // A client's pool: never swept by an application.
    let client_pool = DispatchPool::new("dpsarch-client", "Client pool")
        .with_client_id(ClientId::parse("clt_0DPSARCHCLIE1").unwrap());
    app.repos
        .dispatch_pool_repo
        .insert(&client_pool)
        .await
        .unwrap();
    assert_eq!(
        sync(
            &app,
            &sa,
            "dpsarchmine",
            false,
            &["dpsarchmine-keep", "dpsarchmine-drop"]
        )
        .await
        .0,
        StatusCode::OK
    );

    let (status, body) = sync(&app, &sa, "dpsarchmine", true, &["dpsarchmine-keep"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], 1, "{body}");
    let status_of = |s: Option<(Option<String>, String, String, i32)>| s.unwrap().1;
    assert_eq!(status_of(pool(&app, "dpsarchmine-drop").await), "ARCHIVED");
    assert_eq!(status_of(pool(&app, "dpsarchmine-keep").await), "ACTIVE");
    assert_eq!(status_of(pool(&app, "dpsarchtheirs-pool").await), "ACTIVE");
    assert_eq!(status_of(pool(&app, "dpsarch-platform").await), "ACTIVE");
    let (client_status,): (String,) =
        sqlx::query_as("SELECT status FROM msg_dispatch_pools WHERE id = $1")
            .bind(client_pool.id.as_str())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(client_status, "ACTIVE", "a client's pool");

    // A non-anchor account holding the permission cannot sweep at all.
    let client_scoped = app_sa(&app, "dps_arch_client", UserScope::Client, &[&mine]).await;
    let (status, body) = sync(
        &app,
        &client_scoped,
        "dpsarchmine",
        true,
        &["dpsarchmine-keep"],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body["error"], "ANCHOR_REQUIRED_FOR_PLATFORM_SWEEP",
        "{body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_admin_still_syncs_any_pool_and_claims_the_unowned_ones() {
    let app = TestApp::setup().await;
    let x = create_app(&app, "dpsadmx").await;
    let y = create_app(&app, "dpsadmy").await;
    let admin = app.anchor_admin_token_all_applications().await;

    assert_eq!(
        sync(&app, &admin, "dpsadmx", false, &["dpsadmx-pool"])
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        sync(&app, &admin, "dpsadmy", false, &["dpsadmy-pool"])
            .await
            .0,
        StatusCode::OK
    );
    // An admin may update a pool another application owns, but never
    // re-assigns an owned pool.
    assert_eq!(
        sync(&app, &admin, "dpsadmx", false, &["dpsadmy-pool"])
            .await
            .0,
        StatusCode::OK
    );
    let (owner, ..) = pool(&app, "dpsadmy-pool").await.unwrap();
    assert_eq!(owner.as_deref(), Some(y.id.as_str()));
    let (owner, ..) = pool(&app, "dpsadmx-pool").await.unwrap();
    assert_eq!(owner.as_deref(), Some(x.id.as_str()));
    // And sweep platform-wide.
    assert_eq!(
        sync(&app, &admin, "dpsadmx", true, &["dpsadmx-pool"])
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(pool(&app, "dpsadmy-pool").await.unwrap().1, "ARCHIVED");

    // A pool made in the admin UI has no owner: the application is refused
    // until one admin sync hands it over.
    let claim = create_app(&app, "dpsclaim").await;
    unowned_pool(&app, "dpsclaim-pool").await;
    let sa = app_sa(&app, "dps_claim_sa", UserScope::Anchor, &[&claim]).await;
    not_owned(
        "unowned",
        &sync(&app, &sa, "dpsclaim", false, &["dpsclaim-pool"]).await,
    );
    assert_eq!(
        sync(&app, &admin, "dpsclaim", false, &["dpsclaim-pool"])
            .await
            .0,
        StatusCode::OK
    );
    let (owner, ..) = pool(&app, "dpsclaim-pool").await.unwrap();
    assert_eq!(owner.as_deref(), Some(claim.id.as_str()), "handed over");
    assert_eq!(
        sync(&app, &sa, "dpsclaim", false, &["dpsclaim-pool"])
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_sync_matches_platform_pools_only_never_a_clients_pool_of_the_same_code() {
    let app = TestApp::setup().await;
    let a = create_app(&app, "dpsclient").await;
    let sa = app_sa(&app, "dps_client_sa", UserScope::Anchor, &[&a]).await;
    let admin = app.anchor_admin_token_all_applications().await;
    let client_pool = DispatchPool::new("dpsclient-shared", "Client pool")
        .with_client_id(ClientId::parse("clt_0DPSCLIENT001").unwrap());
    app.repos
        .dispatch_pool_repo
        .insert(&client_pool)
        .await
        .unwrap();

    // A code a client's pool holds is not available to an application...
    not_owned(
        "client code",
        &sync(&app, &sa, "dpsclient", false, &["dpsclient-shared"]).await,
    );
    // ...an admin gets a new platform pool beside it, and the client's pool
    // is left as it was.
    let (status, body) = sync(&app, &admin, "dpsclient", false, &["dpsclient-shared"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["created"], 1, "{body}");
    let (name,): (String,) = sqlx::query_as("SELECT name FROM msg_dispatch_pools WHERE id = $1")
        .bind(client_pool.id.as_str())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(name, "Client pool");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_owner_is_not_on_the_wire() {
    let app = TestApp::setup().await;
    let a = create_app(&app, "dpswire").await;
    let sa = app_sa(&app, "dps_wire_sa", UserScope::Anchor, &[&a]).await;
    assert_eq!(
        sync(&app, &sa, "dpswire", false, &["dpswire-pool"]).await.0,
        StatusCode::OK
    );
    let admin = app.anchor_admin_token_all_applications().await;
    let (status, body) = read_json(app.get("/api/dispatch-pools", &admin).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = body.to_string();
    assert!(text.contains("dpswire-pool"), "{text}");
    assert!(
        !text.contains("applicationId") && !text.contains("application_id"),
        "{text}"
    );
}

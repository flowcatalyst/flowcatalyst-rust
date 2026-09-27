//! Members Go's contract documents that this platform lacked (the "platform
//! gaps" of `docs/sdks.md`), each end to end: stored, written through its use
//! case, answered on the reads Go answers them on. Where Go documents a member
//! but never fills it, the test pins the evident intent instead (the doc
//! comment says so). Requires Docker:
//!   cargo test -p fc-platform --test go_field_gaps_test -- --ignored

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::application::entity::Application;
use support::{assert_status, read_json, TestApp};

async fn setup() -> TestApp {
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    TestApp::setup().await
}

async fn insert_client(app: &TestApp, identifier: &str) -> String {
    let c = fc_platform::client::entity::Client::new(identifier.to_uppercase(), identifier);
    app.repos.client_repo.insert(&c).await.unwrap();
    c.id
}

async fn get_json(app: &TestApp, path: &str, token: &str) -> Value {
    assert_status(app.get(path, token).await, StatusCode::OK).await
}

// ── Migrations ───────────────────────────────────────────────────────────

/// Each field-gap migration is a no-op when re-run, and its probe
/// recognises it on a database that has the columns but no tracker row.
#[tokio::test]
#[ignore = "requires Docker"]
async fn field_gap_migrations_rerun_as_no_ops_and_are_recognised_when_applied() {
    let app = setup().await;
    let migrations: &[(&str, &str)] = &[(
        "058_app_client_config_overrides",
        include_str!("../../../migrations/058_app_client_config_overrides.sql"),
    )];
    for (id, sql) in migrations {
        sqlx::raw_sql(sql)
            .execute(&app.pool)
            .await
            .unwrap_or_else(|e| panic!("re-running {id}: {e}"));
    }
    sqlx::query("DELETE FROM _schema_migrations")
        .execute(&app.pool)
        .await
        .unwrap();
    fc_platform::shared::database::run_migrations(
        &app.pool,
        fc_platform::shared::database::MigrationProfile::Production,
    )
    .await
    .expect("migrations over an already-migrated schema");
    for (id, _) in migrations {
        let (tracked,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM _schema_migrations WHERE migration_id = $1)",
        )
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
        assert!(tracked, "the probe recognises an applied {id}");
    }
}

// ── 1. Client config: baseUrlOverride, configJson ────────────────────────

/// Go documents `baseUrlOverride` and `configJson` on `ClientConfigResponse`
/// but stores neither. They are stored here, set through the config PUT, and
/// answered by both of Go's reads; enabling or disabling the application
/// keeps them.
#[tokio::test]
#[ignore = "requires Docker"]
async fn client_config_keeps_and_answers_the_base_url_override_and_document() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let application = Application::new("cfgapp", "Cfg App");
    app.repos
        .application_repo
        .insert(&application)
        .await
        .unwrap();
    let client_id = insert_client(&app, "cfgclient").await;
    let base = format!("/api/applications/{}/clients", application.id);
    let one = format!("{base}/{client_id}");

    let (s, _) = read_json(app.post(&format!("{one}/enable"), &token, json!({})).await).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let fresh = get_json(&app, &one, &token).await;
    assert!(fresh.get("baseUrlOverride").is_none(), "{fresh}");
    assert!(fresh.get("configJson").is_none(), "{fresh}");

    let document = json!({"featureFlags": {"beta": true}, "tier": 2});
    let put = assert_status(
        app.put(
            &one,
            &token,
            json!({"baseUrlOverride": "https://acme.example.com", "configJson": document}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(put["baseUrlOverride"], "https://acme.example.com");
    assert_eq!(put["effectiveBaseUrl"], "https://acme.example.com");
    assert_eq!(put["configJson"], document);
    assert_eq!(put["config"], document, "the older member is a copy");

    let read = get_json(&app, &one, &token).await;
    assert_eq!(read["baseUrlOverride"], "https://acme.example.com");
    assert_eq!(read["configJson"], document);
    let list = get_json(&app, &base, &token).await;
    assert_eq!(
        list["items"][0]["baseUrlOverride"],
        "https://acme.example.com"
    );
    assert_eq!(list["items"][0]["configJson"], document);

    // Disabling rewrites the row through the same persist: the overrides stay.
    let (s, _) = read_json(app.post(&format!("{one}/disable"), &token, json!({})).await).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let disabled = get_json(&app, &one, &token).await;
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["baseUrlOverride"], "https://acme.example.com");
    assert_eq!(disabled["configJson"], document);

    // `""` clears the override; an absent document is left as it is.
    let cleared = assert_status(
        app.put(&one, &token, json!({"baseUrlOverride": ""})).await,
        StatusCode::OK,
    )
    .await;
    assert!(cleared.get("configJson").is_some());
    let read = get_json(&app, &one, &token).await;
    assert!(read.get("baseUrlOverride").is_none(), "{read}");
    assert_eq!(read["configJson"], document);

    assert_eq!(
        app.event_count_by_type("platform:iam:application:client-config-updated")
            .await,
        2
    );
}

// ── 2. Event types: clientScoped ─────────────────────────────────────────

/// Go honours `clientScoped` on event-type create and update (`/api`) and on
/// the BFF create; absent on update leaves it as it is. The BFF read answers
/// it (the SPA's subscription editor filters on it); Go's `/api` response
/// does not carry it, and neither does this platform's.
#[tokio::test]
#[ignore = "requires Docker"]
async fn event_type_client_scoped_is_stored_on_create_and_update() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let stored = |id: String| {
        let repo = app.repos.event_type_repo.clone();
        async move {
            repo.find_by_id(&id)
                .await
                .unwrap()
                .expect("event type")
                .client_scoped
        }
    };

    let created = assert_status(
        app.post(
            "/api/event-types",
            &token,
            json!({"code": "gaps:orders:order:placed", "name": "Placed", "clientScoped": true}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    assert!(stored(id.clone()).await);
    let read = get_json(&app, &format!("/api/event-types/{id}"), &token).await;
    assert!(
        read.get("clientScoped").is_none(),
        "Go's /api shape: {read}"
    );

    // Absent leaves it; false clears it.
    let path = format!("/api/event-types/{id}");
    let (s, _) = read_json(app.put(&path, &token, json!({"name": "Placed 2"})).await).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(stored(id.clone()).await);
    let (s, _) = read_json(
        app.put(
            &path,
            &token,
            json!({"name": "Placed 3", "clientScoped": false}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(!stored(id.clone()).await);

    // Absent on create is false.
    let plain = assert_status(
        app.post(
            "/api/event-types",
            &token,
            json!({"code": "gaps:orders:order:shipped", "name": "Shipped"}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert!(!stored(plain["id"].as_str().unwrap().to_string()).await);

    // The BFF create honours it and its answer carries it.
    let bff = assert_status(
        app.post(
            "/bff/event-types",
            &token,
            json!({"code": "gaps:orders:order:returned", "name": "Returned", "clientScoped": true}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(bff["clientScoped"], true, "{bff}");
    let bff_id = bff["id"].as_str().unwrap().to_string();
    let bff_read = get_json(&app, &format!("/bff/event-types/{bff_id}"), &token).await;
    assert_eq!(bff_read["clientScoped"], true);
}

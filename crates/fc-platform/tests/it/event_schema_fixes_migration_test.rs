//! Go's F9 follow-up (`4c23adf`): older seeded 1.0 schemas did not describe
//! the data the use cases emit, and the seeder never replaces an attached
//! 1.0. Migration 070 (Go's 072, extended to every form a seed here ever
//! wrote) replaces each while it is still exactly as seeded: its corrected
//! schemas are the seed's; every seeded form is replaced; an edited one is
//! kept; a legacy `v1` version is treated as the seeder treats it; re-running
//! changes nothing; the seed then attaches no second schema. Requires Docker
//! (or `FC_TEST_PG_BIN`).

use std::collections::BTreeMap;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::support::TestApp;
use fc_platform::seed::platform_event_schemas::schema_for;
use fc_platform::shared::database;

const MIGRATION: &str = include_str!("../../../../migrations/070_platform_event_schema_fixes.sql");

/// The migration's `(code, old_schema, new_schema)` rows.
fn rows() -> Vec<(String, Value, Value)> {
    let lines: Vec<&str> = MIGRATION.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(code) = line.strip_prefix("('").and_then(|l| l.strip_suffix("',")) else {
            continue;
        };
        let literal = |l: &str| -> Value {
            let inner = l
                .trim_start_matches('\'')
                .trim_end_matches(&['\'', ',', ')'][..]);
            serde_json::from_str(inner).expect("schema literal")
        };
        out.push((
            code.to_string(),
            literal(lines[i + 1]),
            literal(lines[i + 2]),
        ));
    }
    out
}

async fn migrate(pool: &PgPool) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::raw_sql(MIGRATION)
        .execute(&mut *tx)
        .await
        .expect("run migration 070");
    tx.commit().await.expect("commit");
}

/// Insert `code` as an event type with a `version` schema; returns its id.
async fn event_type(pool: &PgPool, n: usize, code: &str, version: &str, schema: &Value) -> String {
    let parts: Vec<&str> = code.split(':').collect();
    let id = format!("etc_f070{n:09}");
    sqlx::query(
        "INSERT INTO msg_event_types
           (id, code, name, status, source, client_scoped, application, subdomain, aggregate,
            created_at, updated_at)
         VALUES ($1, $2, $2, 'CURRENT', 'UI', false, $3, $4, $5, NOW(), NOW())",
    )
    .bind(&id)
    .bind(code)
    .bind(parts[0])
    .bind(parts[1])
    .bind(parts[2])
    .execute(pool)
    .await
    .expect("insert event type");
    sqlx::query(
        "INSERT INTO msg_event_type_spec_versions
           (id, event_type_id, version, mime_type, schema_content, schema_type, status)
         VALUES ($1, $2, $3, 'application/schema+json', $4, 'JSON_SCHEMA', 'CURRENT')",
    )
    .bind(format!("sch_f070{n:09}"))
    .bind(&id)
    .bind(version)
    .bind(schema)
    .execute(pool)
    .await
    .expect("insert schema");
    id
}

/// code -> every (version, schema) it has.
async fn schemas(pool: &PgPool) -> BTreeMap<String, Vec<(String, Value)>> {
    let rows: Vec<(String, String, Value)> = sqlx::query_as(
        "SELECT et.code, sv.version, sv.schema_content FROM msg_event_type_spec_versions sv
           JOIN msg_event_types et ON et.id = sv.event_type_id ORDER BY et.code, sv.version",
    )
    .fetch_all(pool)
    .await
    .expect("schemas");
    let mut out: BTreeMap<String, Vec<(String, Value)>> = BTreeMap::new();
    for (code, version, schema) in rows {
        out.entry(code).or_default().push((version, schema));
    }
    out
}

/// No database: each row replaces a different seeded form by exactly the
/// seed's schema, and the dispatch-pool and event-type forms are Go's.
#[test]
fn the_corrected_schemas_are_the_seeds() {
    let rows = rows();
    assert_eq!(rows.len(), 32);
    for (code, old, new) in &rows {
        assert_eq!(Some(new), schema_for(code).as_ref(), "{code}");
        assert_ne!(old, new, "{code}");
    }
    let pool = rows
        .iter()
        .find(|(c, _, _)| c == "platform:admin:dispatch-pool:created")
        .unwrap();
    assert!(pool.1["properties"].get("dispatchPoolId").is_some());
    assert_eq!(pool.2["required"], json!(["poolId", "code", "name"]));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_070_replaces_the_seeded_schemas_only() {
    let app = TestApp::setup().await;
    let pool = &app.pool;
    let rows = rows();

    // Every seeded form, each on a type of its own code (a code with two
    // seeded forms gets the first; the second is the legacy `v1` case below).
    let mut seen = Vec::new();
    for (n, (code, old, _)) in rows.iter().enumerate() {
        if seen.contains(code) {
            continue;
        }
        seen.push(code.clone());
        event_type(pool, n, code, "1.0", old).await;
    }
    // Someone edited one: kept.
    let edited = json!({"type": "object", "properties": {"poolId": {"type": "string"}}});
    sqlx::query(
        "UPDATE msg_event_type_spec_versions sv SET schema_content = $1
           FROM msg_event_types et
          WHERE et.id = sv.event_type_id AND et.code = 'platform:admin:dispatch-pool:updated'",
    )
    .bind(&edited)
    .execute(pool)
    .await
    .expect("edit");
    // A legacy `v1` seeded schema is replaced too; a 1.1 is not a seeded one.
    let (_, user_old, _) = rows
        .iter()
        .find(|(c, _, _)| c == "platform:iam:user:created")
        .unwrap();
    let id: String = sqlx::query_scalar(
        "SELECT id FROM msg_event_types WHERE code = 'platform:iam:user:created'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    for (sv, version) in [("sch_f070v1", "v1"), ("sch_f070v11", "1.1")] {
        sqlx::query(
            "INSERT INTO msg_event_type_spec_versions
               (id, event_type_id, version, mime_type, schema_content, schema_type, status)
             VALUES ($1, $2, $3, 'application/schema+json', $4, 'JSON_SCHEMA', 'CURRENT')",
        )
        .bind(sv)
        .bind(&id)
        .bind(version)
        .bind(user_old)
        .execute(pool)
        .await
        .expect("insert other version");
    }

    migrate(pool).await;

    let after = schemas(pool).await;
    for code in &seen {
        let versions = &after[code];
        let one = versions.iter().find(|(v, _)| v == "1.0").unwrap();
        if code == "platform:admin:dispatch-pool:updated" {
            assert_eq!(one.1, edited, "an edited schema is kept");
        } else {
            assert_eq!(Some(&one.1), schema_for(code).as_ref(), "{code}");
        }
    }
    let user = &after["platform:iam:user:created"];
    let version = |v: &str| user.iter().find(|(x, _)| x == v).unwrap().1.clone();
    assert_eq!(Some(version("v1")), schema_for("platform:iam:user:created"));
    assert_eq!(&version("1.1"), user_old);

    // Running it again changes nothing; the seed attaches no second schema.
    migrate(pool).await;
    assert_eq!(schemas(pool).await, after);
    database::seed_platform_event_types(pool)
        .await
        .expect("seed");
    let seeded = schemas(pool).await;
    for code in &seen {
        assert_eq!(seeded[code], after[code], "{code}");
    }
}

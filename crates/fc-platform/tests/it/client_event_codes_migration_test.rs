//! Go's F3 (`25c3ed6`, docs/platform-bug-fixes.md in flowcatalyst-go): a
//! database an older seed populated holds the client events as
//! `platform:iam:client:*`, which nothing emits. Migration 068 (Go's 070)
//! renames them to the emitted `platform:admin:client:*` codes (subscription
//! bindings follow, a schema still as seeded is replaced by one describing
//! the emitted data), the seed then adds the rest, and running either again
//! changes nothing. Where the emitted code already exists, both rows are left
//! alone. Requires Docker (or `FC_TEST_PG_BIN`).

use std::collections::BTreeMap;

use serde_json::Value;
use sqlx::PgPool;

use crate::support::TestApp;
use fc_platform::shared::database;

const MIGRATION: &str = include_str!("../../../../migrations/068_client_event_type_codes.sql");

const EVENTS: [&str; 6] = [
    "created",
    "updated",
    "activated",
    "suspended",
    "deleted",
    "note-added",
];

/// The 1.0 schema an older seed wrote for `platform:iam:client:<event>`.
fn old_schema(event: &str) -> &'static str {
    match event {
        "created" => {
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"description":{"type":["string","null"]},"identifier":{"type":"string"},"name":{"type":"string"}},"required":["clientId","name","identifier"],"type":"object"}"#
        }
        "updated" => {
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"description":{"type":["string","null"]},"name":{"type":["string","null"]}},"required":["clientId"],"type":"object"}"#
        }
        "activated" => {
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"previousStatus":{"type":"string"}},"required":["clientId","previousStatus"],"type":"object"}"#
        }
        "suspended" => {
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"reason":{"type":"string"}},"required":["clientId","reason"],"type":"object"}"#
        }
        "deleted" => {
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"identifier":{"type":"string"},"name":{"type":"string"}},"required":["clientId","name","identifier"],"type":"object"}"#
        }
        "note-added" => {
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"author":{"type":"string"},"category":{"type":"string"},"clientId":{"type":"string"},"text":{"type":"string"}},"required":["clientId","category","text","author"],"type":"object"}"#
        }
        other => panic!("no seeded schema for {other}"),
    }
}

/// Run the migration's SQL in one transaction, as the runner does.
async fn migrate(pool: &PgPool) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::raw_sql(MIGRATION)
        .execute(&mut *tx)
        .await
        .expect("run migration 068");
    tx.commit().await.expect("commit");
}

/// Insert `code` as an event type with a 1.0 schema; returns its id.
async fn event_type(pool: &PgPool, n: usize, code: &str, schema: &str) -> String {
    let parts: Vec<&str> = code.split(':').collect();
    let id = format!("etc_c068{n:09}");
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
         VALUES ($1, $2, '1.0', 'application/schema+json', $3::jsonb, 'JSON_SCHEMA', 'CURRENT')",
    )
    .bind(format!("sch_c068{n:09}"))
    .bind(&id)
    .bind(schema)
    .execute(pool)
    .await
    .expect("insert schema");
    id
}

/// code -> (id, subdomain) of every client event type.
async fn client_types(pool: &PgPool) -> BTreeMap<String, (String, String)> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT code, id, subdomain FROM msg_event_types WHERE code LIKE 'platform:%:client:%'",
    )
    .fetch_all(pool)
    .await
    .expect("client types");
    rows.into_iter()
        .map(|(code, id, sub)| (code, (id, sub)))
        .collect()
}

async fn schema_of(pool: &PgPool, code: &str) -> Value {
    sqlx::query_scalar(
        "SELECT sv.schema_content FROM msg_event_type_spec_versions sv
           JOIN msg_event_types et ON et.id = sv.event_type_id
          WHERE et.code = $1 AND sv.version = '1.0'",
    )
    .bind(code)
    .fetch_one(pool)
    .await
    .expect("schema")
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_068_renames_the_seeded_client_event_types() {
    let app = TestApp::setup().await;
    let pool = &app.pool;

    // What an older seed wrote: the six iam codes, each with its 1.0 schema.
    let mut ids = BTreeMap::new();
    for (n, ev) in EVENTS.iter().enumerate() {
        let code = format!("platform:iam:client:{ev}");
        ids.insert(*ev, event_type(pool, n, &code, old_schema(ev)).await);
    }
    // A subscription made to "Client Created" from the catalogue.
    sqlx::query(
        "INSERT INTO msg_subscription_event_types (subscription_id, event_type_id, event_type_code, spec_version)
         SELECT 'sub_c068000000001', id, code, '1.0' FROM msg_event_types
          WHERE code = 'platform:iam:client:created'",
    )
    .execute(pool)
    .await
    .expect("binding");
    // Someone edited the "updated" schema: an edited schema is kept.
    let edited: Value = serde_json::json!({
        "type": "object",
        "properties": { "clientId": { "type": "string" } },
        "required": ["clientId"]
    });
    sqlx::query(
        "UPDATE msg_event_type_spec_versions sv SET schema_content = $1
           FROM msg_event_types et
          WHERE et.id = sv.event_type_id AND et.code = 'platform:iam:client:updated'",
    )
    .bind(&edited)
    .execute(pool)
    .await
    .expect("edit schema");

    migrate(pool).await;

    // Renamed in place: ids kept, subdomain admin, no iam code left.
    let after = client_types(pool).await;
    for ev in EVENTS {
        let code = format!("platform:admin:client:{ev}");
        let (id, sub) = after
            .get(&code)
            .unwrap_or_else(|| panic!("{code}: {after:?}"));
        assert_eq!(id, &ids[ev], "{code} keeps its id");
        assert_eq!(sub, "admin", "{code}");
        assert!(!after.contains_key(&format!("platform:iam:client:{ev}")));
    }
    // The binding follows the rename.
    let bound: String = sqlx::query_scalar(
        "SELECT event_type_code FROM msg_subscription_event_types WHERE subscription_id = 'sub_c068000000001'",
    )
    .fetch_one(pool)
    .await
    .expect("binding");
    assert_eq!(bound, "platform:admin:client:created");
    // A schema as seeded now describes the emitted data; an edited one is kept.
    let activated = schema_of(pool, "platform:admin:client:activated").await;
    assert_eq!(activated["required"], serde_json::json!(["clientId"]));
    assert!(activated["properties"].get("previousStatus").is_none());
    let note = schema_of(pool, "platform:admin:client:note-added").await;
    assert!(note["properties"].get("author").is_none(), "{note}");
    assert_eq!(
        schema_of(pool, "platform:admin:client:updated").await,
        edited
    );

    // The seed adds the rest (deactivated, applications-updated) once.
    database::seed_platform_event_types(pool)
        .await
        .expect("seed");
    database::seed_platform_event_types(pool)
        .await
        .expect("re-seed");
    let seeded = client_types(pool).await;
    for code in [
        "platform:admin:client:deactivated",
        "platform:iam:client:applications-updated",
    ] {
        assert!(seeded.contains_key(code), "{code}: {seeded:?}");
    }
    assert_eq!(seeded.len(), 8, "{seeded:?}");
    for ev in EVENTS {
        let code = format!("platform:admin:client:{ev}");
        assert_eq!(
            seeded[&code].0, ids[ev],
            "the seed keeps the renamed {code}"
        );
    }

    // Running the migration again changes nothing.
    migrate(pool).await;
    assert_eq!(client_types(pool).await, seeded);
}

/// Where the emitted code already is an event type (a database the seeder
/// here filled with both sets), both rows are left as they are.
#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_068_leaves_both_rows_when_the_emitted_code_exists() {
    let app = TestApp::setup().await;
    let pool = &app.pool;
    let old = event_type(
        pool,
        1,
        "platform:iam:client:created",
        old_schema("created"),
    )
    .await;
    let new = event_type(pool, 2, "platform:admin:client:created", "{}").await;

    migrate(pool).await;

    let after = client_types(pool).await;
    assert_eq!(after["platform:iam:client:created"].0, old);
    assert_eq!(after["platform:admin:client:created"].0, new);
    assert_eq!(after.len(), 2, "{after:?}");
}

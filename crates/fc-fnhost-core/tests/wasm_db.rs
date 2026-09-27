//! `flowcatalyst:function/db` end to end: the committed `pdk_db` guest
//! (written with `fc-function-pdk`) through the real listener, loader and
//! pools. The load failures need no database; the rest need Docker:
//!
//! ```text
//! cargo test -p fc-fnhost-core --test wasm_db -- --include-ignored
//! ```

mod support;

use std::time::Duration;

use serde_json::{json, Value};
use support::postgres::postgres;
use support::wasm::{entry, guest, manifest, WasmHarness, ADDR};

fn db_manifest(pool_size: i32) -> Value {
    manifest(json!({
        "endpoints": [{"path": "/*", "auth": "none", "timeoutMs": 2000}],
        "db": [{"name": "main", "secretRef": "DB_DSN", "poolSize": pool_size}],
    }))
}

async fn start(dsn: &str) -> WasmHarness {
    WasmHarness::start(vec![entry(
        ADDR,
        1,
        &guest("pdk_db"),
        db_manifest(2),
        json!({"secrets": {"DB_DSN": dsn}}),
    )])
    .await
}

fn table(name: &str) -> String {
    format!("w_{name}_{}", std::process::id())
}

async fn post(h: &WasmHarness, path: &str) -> Value {
    let reply = h.post(path, b"", &[]).await;
    assert_eq!(reply.status, 200, "{path}: {}", reply.text());
    reply.json()
}

// ── load: no database needed ─────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_database_this_host_cannot_use_fails_the_load() {
    for (dsn, why) in [
        ("mysql://u:hunter2@db/x", "not PostgreSQL"),
        ("", "no value"),
    ] {
        let h = start(dsn).await;
        let states = h.heartbeat_states();
        assert_eq!(states.len(), 1, "{why}");
        assert!(
            states[0].2.contains("DB_UNSUPPORTED"),
            "{why}: {:?}",
            states[0]
        );
        assert!(!states[0].2.contains("hunter2"), "{:?}", states[0]);
        assert_eq!(h.get("/open").await.status, 503, "{why}: not loaded");
        assert_eq!(h.runtime.db_pools().pool_count(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_pool_too_many_fails_the_load_and_unloading_closes_the_pool() {
    let h = WasmHarness::start_with(
        vec![entry(
            ADDR,
            1,
            &guest("pdk_db"),
            db_manifest(2),
            json!({"secrets": {"DB_DSN": "postgres://u:p@127.0.0.1:1/one"}}),
        )],
        support::wasm::Options {
            db: fc_fnhost_core::db::DbSettings {
                max_pools: 1,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await;
    assert_eq!(h.heartbeat_states()[0].2, "LOADED");
    assert_eq!(h.runtime.db_pools().pool_count(), 1, "lazy: joined at load");
    // A second function on another database: one pool too many.
    let other = "app.orders.other";
    h.publish(vec![
        entry(
            ADDR,
            1,
            &guest("pdk_db"),
            db_manifest(2),
            json!({"secrets": {"DB_DSN": "postgres://u:p@127.0.0.1:1/one"}}),
        ),
        entry(
            other,
            1,
            &guest("pdk_db"),
            db_manifest(2),
            json!({"secrets": {"DB_DSN": "postgres://u:p@127.0.0.1:1/two"}}),
        ),
    ])
    .await;
    let states = h.heartbeat_states();
    let failed = states.iter().find(|s| s.0 == other).unwrap();
    assert!(failed.2.contains("DB_POOL_LIMIT"), "{failed:?}");
    // Unloading the first function closes its pool.
    h.publish(vec![]).await;
    let start = std::time::Instant::now();
    while h.runtime.db_pools().pool_count() != 0 {
        assert!(start.elapsed() < Duration::from_secs(40), "never closed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ── against PostgreSQL ───────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker"]
async fn a_function_queries_executes_and_commits_through_the_host() {
    let h = start(postgres()).await;
    let t = table("crud");
    assert_eq!(post(&h, &format!("/setup?table={t}")).await["updated"], 0);
    assert_eq!(
        post(&h, &format!("/items?table={t}&id=1&name=it%27s%20fine")).await["updated"],
        1
    );
    assert_eq!(
        post(&h, &format!("/tx/commit?table={t}&id=2")).await["done"],
        "/tx/commit"
    );
    let listed = h.guest_json(&format!("/items?table={t}")).await;
    assert_eq!(
        listed,
        json!({"rows": [{"id": 1, "name": "it's fine"}, {"id": 2, "name": "tx"}], "count": 2, "truncated": false})
    );
    let types = h.guest_json("/types").await;
    assert_eq!(
        types["rows"][0],
        json!({"i": 7, "f": 1.5, "b": true, "d": "12.50", "s": "text", "z": null,
               "u": "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11", "later": true})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker"]
async fn a_dropped_or_leaked_transaction_is_rolled_back_and_its_connection_returned() {
    let h = start(postgres()).await;
    let t = table("tx");
    post(&h, &format!("/setup?table={t}")).await;
    assert_eq!(
        post(&h, &format!("/tx/drop?table={t}&id=1")).await["done"],
        "/tx/drop"
    );
    // The guest returns with its transaction still open: the invocation's
    // end rolls it back.
    assert_eq!(
        post(&h, &format!("/tx/leak?table={t}&id=2")).await["done"],
        "/tx/leak"
    );
    let listed = h.guest_json(&format!("/items?table={t}")).await;
    assert_eq!(listed["rows"], json!([]));
    // Both connections came back: a pool of 2 still serves two
    // transactions at once, and the leaked one's row can be inserted again.
    assert_eq!(
        post(&h, &format!("/tx/commit?table={t}&id=2")).await["done"],
        "/tx/commit"
    );
    assert_eq!(h.guest_json(&format!("/items?table={t}")).await["count"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker"]
async fn database_errors_reach_the_guest_as_javas_codes() {
    let h = start(postgres()).await;
    let t = table("codes");
    post(&h, &format!("/setup?table={t}")).await;
    post(&h, &format!("/items?table={t}&id=1&name=a")).await;
    for (path, code) in [
        (format!("/items?table={t}&id=1&name=b"), "DB_CONSTRAINT"),
        (format!("/setup?table={t}"), "DB_SYNTAX"),
    ] {
        assert_eq!(post(&h, &path).await["error"], code, "{path}");
    }
    for (path, code) in [
        ("/sql?q=SELEC%201", "DB_SYNTAX"),
        ("/sql?q=SELECT%201%2F0", "DB_ERROR"),
        ("/open?db=other", "DB_NOT_DECLARED"),
    ] {
        assert_eq!(h.guest_json(path).await["error"], code, "{path}");
    }
    assert_eq!(h.guest_json("/open").await["opened"], "main");

    // A statement's timeout is the time left: it ends with the invocation
    // (504), and its connection comes back for the next call.
    let started = std::time::Instant::now();
    assert_eq!(h.get("/sql?q=SELECT%20pg_sleep(5)").await.status, 504);
    assert!(started.elapsed() < Duration::from_secs(4));
    for _ in 0..3 {
        assert_eq!(h.guest_json(&format!("/items?table={t}")).await["count"], 1);
    }
}

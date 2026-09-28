//! Function database access against a real PostgreSQL (Java `WasmDbTest`
//! and `DbSessionTest`, plus the Rust host's pool rules): query and execute,
//! transactions committed, rolled back and dropped, a connection returned
//! mid-transaction, the caps, the deadline, Java's error codes, the
//! per-function share and the per-invocation cap, pool sharing and secret
//! refresh. Every test needs Docker (one container for the file):
//!
//! ```text
//! cargo test -p fc-fnhost-core --test it db_postgres:: -- --ignored
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fc_fnhost_core::db::{
    DbBindings, DbErrorCode, DbPools, DbSession, DbSettings, NoResolver, Param, SecretResolver,
};
use fc_function_abi::FunctionAddress;
use serde_json::{json, Value};
use support::postgres::postgres;

use crate::support;
use fc_fnhost_core::db::RowsAnswer;
use sqlx::postgres::PgConnection;
use std::process;
use tokio::time;

fn address(name: &str) -> FunctionAddress {
    FunctionAddress::parse(&format!("app.db.{name}")).unwrap()
}

fn pools() -> Arc<DbPools> {
    DbPools::new(DbSettings::default(), Arc::new(NoResolver))
}

/// One version of `function` with its `db[]` entry `main` (and `pool_size`).
async fn bind(pools: &Arc<DbPools>, function: &str, pool_size: u32) -> Arc<DbBindings> {
    let lease = pools
        .join(&address(function), postgres(), pool_size)
        .await
        .unwrap();
    Arc::new(HashMap::from([("main".to_owned(), Arc::new(lease))]))
}

/// A pool of its own (another host's, in effect): the same database.
async fn pools_elsewhere() -> Arc<DbBindings> {
    let pools = pools();
    let lease = pools
        .join(&address("elsewhere"), postgres(), 1)
        .await
        .unwrap();
    // The lease keeps the pool open; the DbPools may go.
    Arc::new(HashMap::from([("main".to_owned(), Arc::new(lease))]))
}

fn session(bindings: &Arc<DbBindings>, timeout: Duration) -> DbSession {
    DbSession::new(Some(bindings.clone()), Instant::now() + timeout)
}

fn rows(answer: &RowsAnswer) -> Vec<Value> {
    serde_json::from_str::<Vec<Value>>(&answer.json).unwrap()
}

fn text(s: &str) -> Param {
    Param::Text(s.into())
}

/// A table name no other test uses.
fn table(name: &str) -> String {
    format!("t_{name}_{}", process::id())
}

/// Connections of `bindings`' pool borrowed right now.
fn borrowed(bindings: &Arc<DbBindings>) -> u32 {
    let pool = bindings["main"].entry().pool();
    pool.size() - pool.num_idle() as u32
}

async fn eventually_returned(bindings: &Arc<DbBindings>) {
    let start = Instant::now();
    while borrowed(bindings) != 0 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "a connection was never returned"
        );
        time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn query_and_execute_bind_parameters_and_never_interpolate_them() {
    let pools = pools();
    let b = bind(&pools, "q", 2).await;
    let mut s = session(&b, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    let t = table("q");
    db.execute(
        &format!("CREATE TABLE {t} (id int PRIMARY KEY, name text)"),
        &[],
    )
    .await
    .unwrap();
    let hostile = "x'); DROP TABLE nothing; --";
    assert_eq!(
        db.execute(
            &format!("INSERT INTO {t} VALUES (?, ?), (?, ?)"),
            &[
                Param::Integer(1),
                text(hostile),
                Param::Integer(2),
                text("b")
            ]
        )
        .await
        .unwrap(),
        2
    );
    let answer = db
        .query(
            &format!("SELECT id, name FROM {t} WHERE name = ? ORDER BY id"),
            &[text(hostile)],
        )
        .await
        .unwrap();
    assert_eq!(rows(&answer), vec![json!({"id": 1, "name": hostile})]);
    assert_eq!((answer.count, answer.truncated), (1, false));
    // A statement that returns nothing answers no rows.
    let none = db
        .query(
            &format!("UPDATE {t} SET name = name WHERE id = ?"),
            &[Param::Integer(9)],
        )
        .await
        .unwrap();
    assert_eq!((none.json.as_str(), none.count), ("[]", 0));
    eventually_returned(&b).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn rows_follow_javas_json_mapping() {
    let pools = pools();
    let b = bind(&pools, "map", 1).await;
    let mut s = session(&b, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    let answer = db
        .query(
            "SELECT 1::int2 AS a, 2::int4 AS b, 9007199254740993::int8 AS c, 1.5::float8 AS d,
                    'NaN'::float8 AS e, 0.1::float4 AS f, 12.50::numeric AS g, true AS h,
                    '2024-01-02 03:04:05.12+00'::timestamptz AS i,
                    '2024-01-02 03:04:05'::timestamp AS j, '2024-01-02'::date AS k,
                    '04:05:06.5'::time AS l, '04:05:06+05:30'::timetz AS m,
                    '\\x00ff'::bytea AS n, '{\"x\":[1,true]}'::jsonb AS o, '[1]'::json AS p,
                    NULL::int AS q, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid AS r,
                    '1 year 2 mons 3 days 04:05:06'::interval AS s, ARRAY[1,NULL,3] AS t,
                    ARRAY['a b','c'] AS u, 'infinity'::timestamptz AS v, 'hé'::text AS w,
                    '10.0.0.1'::inet AS x, 42::oid AS y, 1 AS dup, 2 AS dup",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        rows(&answer)[0],
        json!({
            "a": 1, "b": 2, "c": 9007199254740993i64, "d": 1.5, "e": "NaN", "f": 0.1,
            "g": "12.50", "h": true, "i": "2024-01-02T03:04:05.12Z", "j": "2024-01-02T03:04:05",
            "k": "2024-01-02", "l": "04:05:06.5", "m": "04:05:06+05:30", "n": "AP8=",
            "o": {"x": [1, true]}, "p": [1], "q": null,
            "r": "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11", "s": "1 year 2 mons 3 days 04:05:06",
            "t": "{1,NULL,3}", "u": "{\"a b\",c}", "v": "infinity", "w": "hé",
            "x": "10.0.0.1", "y": 42, "dup": 2
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn text_parameters_take_the_type_their_placeholder_needs() {
    let pools = pools();
    let b = bind(&pools, "types", 1).await;
    let mut s = session(&b, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    let t = table("types");
    db.execute(
        &format!(
            "CREATE TABLE {t} (id uuid, at timestamptz, doc jsonb, day date, n numeric, flag bool, big int8)"
        ),
        &[],
    )
    .await
    .unwrap();
    db.execute(
        &format!("INSERT INTO {t} VALUES (?, ?, ?, ?, ?, ?, ?)"),
        &[
            text("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"),
            text("2024-05-06T07:08:09+02:00"),
            text(r#"{"k": "v"}"#),
            text("2024-05-06"),
            Param::Decimal("2.500".into()),
            text("true"),
            Param::Integer(7),
        ],
    )
    .await
    .unwrap();
    let answer = db
        .query(
            &format!("SELECT * FROM {t} WHERE id = ? AND doc ?? 'k' AND n = ?"),
            &[
                text("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"),
                Param::Float(2.5),
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        rows(&answer),
        vec![json!({
            "id": "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11", "at": "2024-05-06T05:08:09Z",
            "doc": {"k": "v"}, "day": "2024-05-06", "n": "2.500", "flag": true, "big": 7
        })]
    );
    // Java's explicit types: `SELECT ?` keeps an integer an integer, and a
    // text a text.
    let typed = db
        .query(
            "SELECT ? AS i, ? AS s, ? AS d, ? AS b, ? AS z",
            &[
                Param::Integer(5),
                text("5"),
                Param::Decimal("1.50".into()),
                Param::Boolean(false),
                Param::Null,
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        rows(&typed),
        vec![json!({"i": 5, "s": "5", "d": "1.50", "b": false, "z": null})]
    );
    // The same text in two placeholder types, and a cast through text.
    let cast = db
        .query(
            "SELECT ?::text::interval AS i, ?::int + 1 AS n",
            &[text("2 days"), text("41")],
        )
        .await
        .unwrap();
    assert_eq!(rows(&cast), vec![json!({"i": "2 days", "n": 42})]);
    let bad = db
        .query(
            &format!("SELECT 1 FROM {t} WHERE id = ?"),
            &[text("not-a-uuid")],
        )
        .await
        .unwrap_err();
    assert_eq!(bad.code, DbErrorCode::BadRequest);
    assert!(!bad.message.contains("not-a-uuid"), "{}", bad.message);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn a_committed_transaction_is_visible_and_a_dropped_one_is_rolled_back() {
    let pools = pools();
    let b = bind(&pools, "tx", 2).await;
    let mut s = session(&b, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    let t = table("tx");
    db.execute(&format!("CREATE TABLE {t} (v text)"), &[])
        .await
        .unwrap();
    let count_sql = format!("SELECT count(*)::int AS n FROM {t}");

    let mut tx = db.begin().await.unwrap();
    tx.execute(&format!("INSERT INTO {t} VALUES (?)"), &[text("committed")])
        .await
        .unwrap();
    assert_eq!(
        rows(&db.query(&count_sql, &[]).await.unwrap())[0]["n"],
        json!(0),
        "not visible before commit"
    );
    tx.commit().await.unwrap();
    assert_eq!(
        rows(&db.query(&count_sql, &[]).await.unwrap())[0]["n"],
        json!(1)
    );

    let mut tx = db.begin().await.unwrap();
    tx.execute(&format!("INSERT INTO {t} VALUES ('rolled back')"), &[])
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    // The pool resets a released connection (`after_release`) before it is
    // idle again, so wait for it before counting the next borrow.
    eventually_returned(&b).await;

    let mut tx = db.begin().await.unwrap();
    tx.execute(&format!("INSERT INTO {t} VALUES ('dropped')"), &[])
        .await
        .unwrap();
    assert_eq!(borrowed(&b), 1, "the open transaction holds its connection");
    drop(tx);
    eventually_returned(&b).await;
    assert_eq!(
        rows(&db.query(&count_sql, &[]).await.unwrap())[0]["n"],
        json!(1),
        "rollback and drop kept nothing"
    );
    assert_eq!(
        b["main"].share().available(),
        2,
        "every share permit is back"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn a_connection_returned_mid_transaction_is_rolled_back_and_reset() {
    // One connection, so every borrow gets the same one back.
    let pools = pools();
    let b = bind(&pools, "reset", 1).await;
    let t = table("reset");
    let mut s = session(&b, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    db.execute(&format!("CREATE TABLE {t} (v int)"), &[])
        .await
        .unwrap();
    // Java's bug: BEGIN sent as a statement leaves the pooled connection in
    // a transaction, and the next borrower's statements run inside it, never
    // committed. Here the reset rolls it back, so the INSERT that follows
    // autocommits.
    db.execute("BEGIN", &[]).await.unwrap();
    db.execute(&format!("INSERT INTO {t} VALUES (1)"), &[])
        .await
        .unwrap();
    // Session settings leak the same way.
    db.execute("SET search_path TO nowhere", &[]).await.unwrap();
    db.execute("SET TIME ZONE 'Asia/Tokyo'", &[]).await.unwrap();
    // A failed transaction, left behind: without the reset, every later
    // statement fails "current transaction is aborted".
    db.execute("BEGIN", &[]).await.unwrap();
    let _ = db.execute("SELECT 1/0", &[]).await.unwrap_err();
    eventually_returned(&b).await;

    let state = db
        .query(
            "SELECT current_setting('search_path') AS path, current_setting('TimeZone') AS tz",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        rows(&state),
        vec![json!({"path": "\"$user\", public", "tz": "UTC"})]
    );
    // Committed: visible from another pool's connection.
    let elsewhere = pools_elsewhere().await;
    let mut s2 = session(&elsewhere, Duration::from_secs(10));
    let n = s2
        .open("main")
        .unwrap()
        .query(&format!("SELECT count(*)::int AS n FROM public.{t}"), &[])
        .await
        .unwrap();
    assert_eq!(rows(&n), vec![json!({"n": 1})]);
    // The statement cache was forgotten with the server's prepared
    // statements: the same SQL runs again on the reset connection.
    for _ in 0..3 {
        db.query("SELECT ? AS x", &[Param::Integer(1)])
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn a_query_answers_at_most_ten_thousand_rows_or_eight_mib() {
    let pools = pools();
    let b = bind(&pools, "caps", 1).await;
    let mut s = session(&b, Duration::from_secs(30));
    let db = s.open("main").unwrap();
    let many = db
        .query("SELECT g AS n FROM generate_series(1, 10001) g", &[])
        .await
        .unwrap();
    assert_eq!((many.count, many.truncated), (10_000, true));
    assert_eq!(rows(&many).last().unwrap()["n"], 10_000);
    let exact = db
        .query("SELECT g AS n FROM generate_series(1, 10000) g", &[])
        .await
        .unwrap();
    assert_eq!((exact.count, exact.truncated), (10_000, false));
    // 1 MiB a row: 7 rows fit in 8 MiB of row JSON, the 8th does not.
    let big = db
        .query(
            "SELECT repeat('x', 1048576) AS s FROM generate_series(1, 20)",
            &[],
        )
        .await
        .unwrap();
    assert_eq!((big.count, big.truncated), (7, true));
    // The rest of the result was drained: the connection still works.
    assert_eq!(
        rows(&db.query("SELECT 1 AS one", &[]).await.unwrap()),
        vec![json!({"one": 1})]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn a_statement_stops_at_the_invocation_deadline() {
    let pools = pools();
    let b = bind(&pools, "deadline", 1).await;
    let mut s = session(&b, Duration::from_millis(400));
    let db = s.open("main").unwrap();
    let started = Instant::now();
    let err = db.query("SELECT pg_sleep(5)", &[]).await.unwrap_err();
    assert_eq!(err.code, DbErrorCode::Timeout, "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    eventually_returned(&b).await;
    time::sleep(Duration::from_millis(450)).await;
    let late = db.query("SELECT 1", &[]).await.unwrap_err();
    assert_eq!(late.code, DbErrorCode::Timeout);
    assert_eq!(late.message, "no time left before the invocation deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn failures_carry_javas_codes() {
    let pools = pools();
    let b = bind(&pools, "codes", 2).await;
    let mut s = session(&b, Duration::from_secs(10));
    assert_eq!(
        s.open("other").err().unwrap().code,
        DbErrorCode::NotDeclared
    );
    let db = s.open("main").unwrap();
    let t = table("codes");
    db.execute(&format!("CREATE TABLE {t} (id int PRIMARY KEY)"), &[])
        .await
        .unwrap();
    db.execute(&format!("INSERT INTO {t} VALUES (1)"), &[])
        .await
        .unwrap();
    let code = |sql: String, params: Vec<Param>| {
        let db = &db;
        async move { db.execute(&sql, &params).await.unwrap_err().code }
    };
    assert_eq!(
        code(format!("INSERT INTO {t} VALUES (1)"), vec![]).await,
        DbErrorCode::Constraint
    );
    assert_eq!(code("SELEC 1".into(), vec![]).await, DbErrorCode::Syntax);
    assert_eq!(
        code("SELECT * FROM no_such_table".into(), vec![]).await,
        DbErrorCode::Syntax
    );
    assert_eq!(code("SELECT 1/0".into(), vec![]).await, DbErrorCode::Error);
    assert_eq!(
        code("SELECT ?".into(), vec![]).await,
        DbErrorCode::BadRequest
    );
    assert_eq!(code(" ".into(), vec![]).await, DbErrorCode::BadRequest);
    // A commit after a failed statement did not commit.
    let mut tx = db.begin().await.unwrap();
    tx.execute(&format!("INSERT INTO {t} VALUES (2)"), &[])
        .await
        .unwrap();
    assert_eq!(
        tx.execute(&format!("INSERT INTO {t} VALUES (1)"), &[])
            .await
            .unwrap_err()
            .code,
        DbErrorCode::Constraint
    );
    assert_eq!(tx.commit().await.unwrap_err().code, DbErrorCode::Error);
    let n = db
        .query(&format!("SELECT count(*)::int AS n FROM {t}"), &[])
        .await
        .unwrap();
    assert_eq!(rows(&n), vec![json!({"n": 1})]);

    // Nothing listening: unavailable.
    let unreachable_pools = pools;
    let lease = unreachable_pools
        .join(&address("down"), "postgres://u:p@127.0.0.1:1/x", 1)
        .await
        .unwrap();
    let down = Arc::new(HashMap::from([("main".to_owned(), Arc::new(lease))]));
    let mut s = session(&down, Duration::from_millis(500));
    let err = s
        .open("main")
        .unwrap()
        .query("SELECT 1", &[])
        .await
        .unwrap_err();
    assert_eq!(err.code, DbErrorCode::Unavailable, "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn a_function_holds_at_most_its_share_of_a_shared_pool() {
    let pools = pools();
    // One pool (size 3, the larger declaration) shared by a function
    // declaring 1 and one declaring 3.
    let slow = bind(&pools, "slow", 1).await;
    let other = bind(&pools, "other", 3).await;
    assert_eq!(pools.pool_count(), 1);
    assert_eq!(slow["main"].entry().size(), 3);

    // One of `slow`'s invocations holds its whole share…
    let mut first = session(&slow, Duration::from_secs(10));
    let held = first.open("main").unwrap().begin().await.unwrap();
    // …so its next invocation waits for it, until its own deadline.
    let mut second = session(&slow, Duration::from_millis(300));
    let started = Instant::now();
    let waited = second
        .open("main")
        .unwrap()
        .query("SELECT 1", &[])
        .await
        .unwrap_err();
    assert_eq!(waited.code, DbErrorCode::Timeout);
    assert!(started.elapsed() >= Duration::from_millis(250));
    // The other function still gets the pool's other connections.
    let mut theirs = session(&other, Duration::from_secs(10));
    let db = theirs.open("main").unwrap();
    let a = db.begin().await.unwrap();
    let c = db.begin().await.unwrap();
    drop((a, c, held));
    eventually_returned(&slow).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn one_invocation_holds_at_most_its_cap_and_never_waits_on_itself() {
    let pools = DbPools::new(
        DbSettings {
            max_connections_per_invocation: 2,
            ..DbSettings::default()
        },
        Arc::new(NoResolver),
    );
    let b = bind(&pools, "cap", 3).await;
    let mut s = session(&b, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    let one = db.begin().await.unwrap();
    let two = db.begin().await.unwrap();
    let third = db.begin().await.err().unwrap();
    assert_eq!(third.code, DbErrorCode::BadRequest, "{third}");
    // A statement outside them still has the third connection.
    db.query("SELECT 1", &[]).await.unwrap();
    drop(one);
    let _again = db.begin().await.unwrap();
    drop(two);

    // With a share of 1, a statement beside the open transaction would wait
    // on it: refused at once instead.
    let small = bind(&pools, "small", 1).await;
    let mut s = session(&small, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    let _tx = db.begin().await.unwrap();
    let started = Instant::now();
    let refused = db.query("SELECT 1", &[]).await.unwrap_err();
    assert_eq!(refused.code, DbErrorCode::BadRequest);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn the_invocation_ending_rolls_back_what_it_left_open() {
    let pools = pools();
    let b = bind(&pools, "end", 2).await;
    let t = table("end");
    {
        let mut s = session(&b, Duration::from_secs(10));
        let db = s.open("main").unwrap();
        db.execute(&format!("CREATE TABLE {t} (v int)"), &[])
            .await
            .unwrap();
        let mut tx = db.begin().await.unwrap();
        tx.execute(&format!("INSERT INTO {t} VALUES (1)"), &[])
            .await
            .unwrap();
        // The invocation's store drops everything: the session, the handle.
        drop((tx, db, s));
    }
    eventually_returned(&b).await;
    let mut s = session(&b, Duration::from_secs(10));
    let n = s
        .open("main")
        .unwrap()
        .query(&format!("SELECT count(*)::int AS n FROM {t}"), &[])
        .await
        .unwrap();
    assert_eq!(rows(&n), vec![json!({"n": 0})]);
}

/// A resolver whose secret the test rotates.
struct Rotating(parking_lot::Mutex<String>);

#[async_trait]
impl SecretResolver for Rotating {
    fn handles(&self, scheme: &str) -> bool {
        scheme == "test-sm"
    }

    async fn resolve(&self, _reference: &str) -> Result<String, String> {
        Ok(self.0.lock().clone())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn a_rotated_reference_reaches_the_pool_without_a_reload() {
    // A role of its own, whose password the test rotates.
    let admin = pools();
    let ab = bind(&admin, "admin", 1).await;
    let mut s = session(&ab, Duration::from_secs(10));
    let db = s.open("main").unwrap();
    let role = format!("rotating_{}", process::id());
    db.execute(&format!("DROP ROLE IF EXISTS {role}"), &[])
        .await
        .unwrap();
    db.execute(&format!("CREATE ROLE {role} LOGIN PASSWORD 'first'"), &[])
        .await
        .unwrap();
    let dsn =
        |password: &str| postgres().replace("postgres:postgres@", &format!("{role}:{password}@"));

    let secret = Arc::new(Rotating(parking_lot::Mutex::new(dsn("first"))));
    let pools = DbPools::new(
        DbSettings {
            max_connections_per_invocation: 1,
            secret_refresh: Duration::from_millis(200),
            ..DbSettings::default()
        },
        secret.clone(),
    );
    let lease = pools
        .join(&address("rotating"), "test-sm://orders", 1)
        .await
        .unwrap();
    let b = Arc::new(HashMap::from([("main".to_owned(), Arc::new(lease))]));
    let who = |b: Arc<DbBindings>| async move {
        let mut s = session(&b, Duration::from_secs(10));
        let db = s.open("main").unwrap();
        db.query("SELECT current_user AS u", &[]).await
    };
    assert!(who(b.clone()).await.is_ok());

    // Rotate: the database first, then the secret (RDS's order).
    db.execute(&format!("ALTER ROLE {role} PASSWORD 'second'"), &[])
        .await
        .unwrap();
    *secret.0.lock() = dsn("second");
    time::sleep(Duration::from_millis(700)).await;
    // Every new connection of the pool now logs in with the new password
    // (an existing one stays valid: PostgreSQL never ends a session for a
    // password change).
    use sqlx::Connection as _;
    let options = b["main"].entry().pool().connect_options();
    let fresh = PgConnection::connect_with(&options)
        .await
        .expect("the refreshed credentials log in");
    fresh.close().await.unwrap();
    assert_eq!(
        rows(&who(b.clone()).await.unwrap()),
        vec![json!({"u": role})]
    );
    // A second function naming the same reference joins the same pool.
    let _second = pools
        .join(&address("rotating2"), "test-sm://orders", 1)
        .await
        .unwrap();
    assert_eq!(pools.pool_count(), 1);
}

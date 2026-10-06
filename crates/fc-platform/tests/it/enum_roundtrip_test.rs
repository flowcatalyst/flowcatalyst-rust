//! String enums against a real PostgreSQL: an enum binds as text (alone and in
//! an array), reads back as the enum, and a stored spelling no variant has is
//! refused by the enum's own `Decode` and, as a `Stored` column, comes back
//! as the error that names its table, column and row.
//!
//!   cargo test -p fc-platform --test it enum_roundtrip_test:: -- --ignored

use crate::support::start_db;
use fc_platform_core::shared::enum_str::Stored;
use fc_platform_iam::client::entity::ClientStatus;
use sqlx::PgPool;

#[tokio::test]
#[ignore = "requires Docker"]
async fn enums_round_trip_through_text_and_stored_names_a_corrupt_row() {
    let (_db, url) = start_db("enum").await;
    let pool = PgPool::connect(&url).await.expect("connect");

    sqlx::query("CREATE TABLE t (id VARCHAR(17) PRIMARY KEY, status VARCHAR(20))")
        .execute(&pool)
        .await
        .unwrap();
    for (id, status) in [
        ("a", ClientStatus::Active),
        ("b", ClientStatus::Inactive),
        ("c", ClientStatus::Suspended),
    ] {
        sqlx::query("INSERT INTO t (id, status) VALUES ($1, $2)")
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
    }
    // The spelling is the enum's, not something the SQL decides.
    let stored: String = sqlx::query_scalar("SELECT status FROM t WHERE id = 'b'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, ClientStatus::Inactive.as_str());

    let (got,): (ClientStatus,) = sqlx::query_as("SELECT status FROM t WHERE id = 'a'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(got, ClientStatus::Active);

    let wanted = vec![ClientStatus::Active, ClientStatus::Suspended];
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM t WHERE status = ANY($1) ORDER BY id")
            .bind(&wanted)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ids, ["a", "c"]);

    let null: Option<ClientStatus> = sqlx::query_scalar("SELECT NULL::varchar")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(null, None);

    // A value no variant spells: the enum's Decode refuses it ...
    sqlx::query("INSERT INTO t (id, status) VALUES ('d', 'active')")
        .execute(&pool)
        .await
        .unwrap();
    let plain = sqlx::query_scalar::<_, ClientStatus>("SELECT status FROM t WHERE id = 'd'")
        .fetch_one(&pool)
        .await;
    assert!(plain.is_err());

    // ... and a Stored column reads it, then names the row.
    let (id, status): (String, Stored<ClientStatus>) =
        sqlx::query_as("SELECT id, status FROM t WHERE id = 'd'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let err = status
        .decode("tnt_clients", "status", &id)
        .unwrap_err()
        .to_string();
    for part in ["tnt_clients.status", "row d", "\"active\""] {
        assert!(err.contains(part), "{err} should mention {part}");
    }
    let (_, ok): (String, Stored<ClientStatus>) =
        sqlx::query_as("SELECT id, status FROM t WHERE id = 'a'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        ok.decode("tnt_clients", "status", "a").unwrap(),
        ClientStatus::Active
    );
}

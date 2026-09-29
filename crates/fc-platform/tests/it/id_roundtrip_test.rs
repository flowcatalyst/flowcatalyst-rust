//! `Id<K>` against a real PostgreSQL: it binds and reads as `varchar`, in
//! arrays too, and a stored id of another kind fails to decode.
//!
//!   cargo test -p fc-platform --test it id_roundtrip_test:: -- --ignored

use fc_platform_core::shared::id::{ClientId, PrincipalId};
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

#[tokio::test]
#[ignore = "requires Docker"]
async fn ids_round_trip_through_varchar_and_arrays() {
    let container = Postgres::default().start().await.expect("start postgres");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let pool = PgPool::connect(&format!(
        "postgresql://postgres:postgres@{host}:{port}/postgres"
    ))
    .await
    .expect("connect");

    sqlx::query("CREATE TABLE t (id VARCHAR(17) PRIMARY KEY, owner VARCHAR(17))")
        .execute(&pool)
        .await
        .unwrap();

    let a = ClientId::generate();
    let b = ClientId::generate();
    let owner = PrincipalId::generate();
    for id in [&a, &b] {
        sqlx::query("INSERT INTO t (id, owner) VALUES ($1, $2)")
            .bind(id)
            .bind(&owner)
            .execute(&pool)
            .await
            .unwrap();
    }

    let (got,): (ClientId,) = sqlx::query_as("SELECT id FROM t WHERE id = $1")
        .bind(&a)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(got, a);

    let wanted = vec![a.clone(), b.clone()];
    let rows: Vec<(ClientId, PrincipalId)> =
        sqlx::query_as("SELECT id, owner FROM t WHERE id = ANY($1) ORDER BY id")
            .bind(&wanted)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|(_, o)| *o == owner));

    let null: Option<PrincipalId> = sqlx::query_scalar("SELECT NULL::varchar")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(null.is_none());

    // The column holds a `prn_` id; reading it as a ClientId is an error.
    let wrong = sqlx::query_scalar::<_, ClientId>("SELECT owner FROM t LIMIT 1")
        .fetch_one(&pool)
        .await;
    assert!(wrong.is_err(), "a prn_ id must not decode as a ClientId");
}

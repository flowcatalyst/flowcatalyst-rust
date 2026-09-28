//! fc-server's outbox role (`FC_OUTBOX_ENABLED`) reads a MySQL outbox:
//! consumer applications keep their outbox tables in MySQL as well as
//! Postgres. The role runs `fc_outbox::setup::connect`, as
//! fc-outbox-processor does; this build must carry the MySQL backend.

use fc_outbox::{setup, OutboxBackend, OutboxTableConfig};

#[tokio::test]
async fn the_outbox_role_carries_the_mysql_backend() {
    // The MySQL driver parses the URL and refuses its bad option at once,
    // rather than the backend being "not built into this binary".
    let err = match setup::connect(
        OutboxBackend::Mysql,
        "mysql://root@127.0.0.1:1/app?ssl-mode=bogus",
        OutboxTableConfig::default(),
    )
    .await
    {
        Ok(_) => panic!("the URL is invalid"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("ssl_mode"), "{err}");
}

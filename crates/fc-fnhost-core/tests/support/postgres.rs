//! One PostgreSQL container per test binary (Docker), kept alive by a
//! thread of its own for as long as the tests run.

#![allow(dead_code)]

use std::sync::OnceLock;

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

/// `postgres://postgres:postgres@127.0.0.1:<port>/postgres`.
pub fn postgres() -> &'static str {
    static DSN: OnceLock<String> = OnceLock::new();
    DSN.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async move {
                let container = Postgres::default()
                    .start()
                    .await
                    .expect("Docker is running");
                let port = container.get_host_port_ipv4(5432).await.unwrap();
                tx.send(format!(
                    "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
                ))
                .unwrap();
                std::future::pending::<()>().await;
                drop(container);
            });
        });
        rx.recv().unwrap()
    })
}

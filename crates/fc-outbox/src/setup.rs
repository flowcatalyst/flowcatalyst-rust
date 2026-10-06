//! The outbox processor's start-up, shared by the `fc-outbox-processor`
//! binary and `fc-server`'s outbox role (`FC_OUTBOX_ENABLED`): which
//! database and tables to read, and Go's group admin API
//! (`FC_OUTBOX_ADMIN_PORT`).
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `FC_OUTBOX_BACKEND` / `FC_OUTBOX_DB_TYPE` | `postgres` | `sqlite`, `postgres`, `mysql` or `mongo` |
//! | `FC_OUTBOX_DB_URL` (mongo also `FC_OUTBOX_MONGO_URI`) | - | The application database |
//! | `FC_OUTBOX_MONGO_DB` | `flowcatalyst` | MongoDB database name |
//! | `FC_OUTBOX_EVENTS_TABLE` | `outbox_messages` | Table for EVENT items |
//! | `FC_OUTBOX_DISPATCH_JOBS_TABLE` | `outbox_messages` | Table for DISPATCH_JOB items |
//! | `FC_OUTBOX_AUDIT_LOGS_TABLE` | `outbox_messages` | Table for AUDIT_LOG items |
//! | `FC_OUTBOX_ADMIN_PORT` | `0` (off) | Serve the group admin API on `127.0.0.1` |

use std::sync::Arc;

use anyhow::Result;
use fc_common::config::{env_first, env_first_opt, env_or, env_or_parse};
use tracing::info;

#[cfg(feature = "mongo")]
use crate::mongo::MongoOutboxRepository;
#[cfg(any(feature = "mysql", test))]
use crate::mysql::MySqlOutboxRepository;
#[cfg(any(feature = "postgres", test))]
use crate::postgres::PostgresOutboxRepository;
use crate::repository::{OutboxRepository, OutboxTableConfig};
#[cfg(any(feature = "sqlite", test))]
use crate::sqlite::SqliteOutboxRepository;
use crate::{OutboxBackend, UnknownOutboxBackend};
#[cfg(any(feature = "mysql", test))]
use sqlx::mysql::MySqlPoolOptions;
#[cfg(any(feature = "postgres", test))]
use sqlx::postgres::PgPoolOptions;
#[cfg(any(feature = "sqlite", test))]
use sqlx::sqlite::SqlitePoolOptions;

/// `FC_OUTBOX_BACKEND` (Go's name), then `FC_OUTBOX_DB_TYPE`; `postgres`
/// when neither is set.
pub fn backend_from_env() -> Result<OutboxBackend, UnknownOutboxBackend> {
    env_first(&["FC_OUTBOX_BACKEND", "FC_OUTBOX_DB_TYPE"], "postgres").parse()
}

/// The tables each item type is read from (all `outbox_messages` unless
/// configured).
pub fn table_config_from_env() -> OutboxTableConfig {
    OutboxTableConfig {
        events_table: env_or("FC_OUTBOX_EVENTS_TABLE", "outbox_messages"),
        dispatch_jobs_table: env_or("FC_OUTBOX_DISPATCH_JOBS_TABLE", "outbox_messages"),
        audit_logs_table: env_or("FC_OUTBOX_AUDIT_LOGS_TABLE", "outbox_messages"),
    }
}

/// The application database: `FC_OUTBOX_DB_URL`, and for `mongo` also Go's
/// `FC_OUTBOX_MONGO_URI`.
pub fn database_url_from_env(backend: OutboxBackend) -> Option<String> {
    match backend {
        OutboxBackend::Mongo => env_first_opt(&["FC_OUTBOX_MONGO_URI", "FC_OUTBOX_DB_URL"]),
        OutboxBackend::Sqlite | OutboxBackend::Postgres | OutboxBackend::Mysql => {
            env_first_opt(&["FC_OUTBOX_DB_URL"])
        }
    }
}

/// `FC_OUTBOX_ADMIN_PORT`; `0` (the default) serves no admin API.
pub fn admin_port_from_env() -> u16 {
    env_or_parse("FC_OUTBOX_ADMIN_PORT", 0)
}

/// Connects to the application database at `url`, creates the outbox
/// tables (or collection indexes) when missing, and returns the repository.
/// A backend this build was compiled without is an error naming it.
pub async fn connect(
    backend: OutboxBackend,
    url: &str,
    table_config: OutboxTableConfig,
) -> Result<Arc<dyn OutboxRepository>> {
    let repo: Arc<dyn OutboxRepository> = match backend {
        #[cfg(any(feature = "sqlite", test))]
        OutboxBackend::Sqlite => {
            let pool = SqlitePoolOptions::new()
                .max_connections(5)
                .connect(url)
                .await?;
            Arc::new(SqliteOutboxRepository::with_config(pool, table_config))
        }
        #[cfg(any(feature = "postgres", test))]
        OutboxBackend::Postgres => {
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(url)
                .await?;
            Arc::new(PostgresOutboxRepository::with_config(pool, table_config))
        }
        #[cfg(any(feature = "mysql", test))]
        OutboxBackend::Mysql => {
            let pool = MySqlPoolOptions::new()
                .max_connections(10)
                .connect(url)
                .await?;
            Arc::new(MySqlOutboxRepository::with_config(pool, table_config))
        }
        #[cfg(feature = "mongo")]
        OutboxBackend::Mongo => {
            let db_name = env_or("FC_OUTBOX_MONGO_DB", "flowcatalyst");
            let client = mongodb::Client::with_uri_str(url).await?;
            Arc::new(MongoOutboxRepository::with_config(
                client,
                &db_name,
                table_config,
            ))
        }
        #[allow(unreachable_patterns)]
        other => anyhow::bail!(
            "the {other} outbox backend is not built into this binary (fc-outbox-processor has \
             every backend)"
        ),
    };
    repo.init_schema().await?;
    info!(backend = %backend, "Outbox repository initialized");
    Ok(repo)
}

/// A Postgres outbox on a pool the caller already holds (Go's fc-server
/// reads the outbox from its own database when no outbox URL is given).
/// Creates the tables when missing.
#[cfg(any(feature = "postgres", test))]
pub async fn postgres_on_pool(
    pool: sqlx::PgPool,
    table_config: OutboxTableConfig,
) -> Result<Arc<dyn OutboxRepository>> {
    let repo = PostgresOutboxRepository::with_config(pool, table_config);
    repo.init_schema().await?;
    Ok(Arc::new(repo))
}

#[cfg(feature = "admin")]
pub use admin::{admin_router, serve_admin};

/// Go's outbox `AdminHandler`: operate on message groups.
///
/// | Route | Effect |
/// |---|---|
/// | `GET /outbox/groups` | Paused and Blocked groups |
/// | `GET /outbox/groups/blocked` | Blocked groups only |
/// | `POST /outbox/groups/{group}/pause` | Stop sending the group |
/// | `POST /outbox/groups/{group}/resume` | Resume a Paused group |
/// | `POST /outbox/groups/{group}/unblock` | Re-queue the blocking item and run the group again (404 if not Blocked) |
/// | `POST /outbox/groups/{group}/skip` | Leave the blocking item failed and advance (404 if not Blocked) |
#[cfg(feature = "admin")]
mod admin {
    use std::future::Future;
    use std::net::SocketAddr;
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use tracing::info;

    use crate::EnhancedOutboxProcessor;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    type Processor = Arc<EnhancedOutboxProcessor>;

    /// Go's `AdminHandler` routes and answers.
    pub fn admin_router(processor: Processor) -> Router {
        Router::new()
            .route("/outbox/groups", get(admin_groups))
            .route("/outbox/groups/blocked", get(admin_blocked))
            .route("/outbox/groups/{group}/pause", post(admin_pause))
            .route("/outbox/groups/{group}/resume", post(admin_resume))
            .route("/outbox/groups/{group}/unblock", post(admin_unblock))
            .route("/outbox/groups/{group}/skip", post(admin_skip))
            .with_state(processor)
    }

    /// Binds `127.0.0.1:port` (localhost only, as Go) and serves the admin
    /// API until `shutdown` resolves. Returns once bound.
    pub async fn serve_admin(
        port: u16,
        processor: Processor,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<JoinHandle<()>> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = TcpListener::bind(addr).await?;
        info!("Outbox admin API listening on http://{}", addr);
        let app = admin_router(processor);
        Ok(tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await
                .ok();
        }))
    }

    async fn admin_groups(State(p): State<Processor>) -> Json<serde_json::Value> {
        Json(serde_json::json!({ "groups": p.group_states() }))
    }

    async fn admin_blocked(State(p): State<Processor>) -> Json<serde_json::Value> {
        Json(serde_json::json!({ "blocked": p.blocked_groups() }))
    }

    async fn admin_pause(
        State(p): State<Processor>,
        Path(group): Path<String>,
    ) -> Json<serde_json::Value> {
        p.pause_group(&group);
        Json(serde_json::json!({ "status": "PAUSED" }))
    }

    async fn admin_resume(
        State(p): State<Processor>,
        Path(group): Path<String>,
    ) -> Json<serde_json::Value> {
        p.resume_group(&group);
        Json(serde_json::json!({ "status": "RUNNING" }))
    }

    async fn admin_unblock(
        State(p): State<Processor>,
        Path(group): Path<String>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        if p.unblock_group(&group).await {
            (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "UNBLOCKED" })),
            )
        } else {
            not_blocked()
        }
    }

    async fn admin_skip(
        State(p): State<Processor>,
        Path(group): Path<String>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        if p.skip_group(&group) {
            (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "SKIPPED" })),
            )
        } else {
            not_blocked()
        }
    }

    fn not_blocked() -> (StatusCode, Json<serde_json::Value>) {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "group not blocked" })),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::repository::{ClaimedBatch, OutboxRepository, OutboxTableConfig};
        use crate::{DispatchOutcome, EnhancedProcessorConfig, OutboxDispatcher};
        use async_trait::async_trait;
        use axum::body;
        use axum::body::Body;
        use axum::http::Request;
        use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
        use std::sync::Mutex;
        use std::time::Duration;
        use tokio::time;

        /// A repository with one PENDING item that fails for good.
        #[derive(Default)]
        struct OneItem {
            requeued: Mutex<Vec<String>>,
            claimed: Mutex<bool>,
            config: OutboxTableConfig,
        }

        #[async_trait]
        impl OutboxRepository for OneItem {
            async fn claim_pending(&self, _limit: u32) -> anyhow::Result<ClaimedBatch> {
                let mut claimed = self.claimed.lock().unwrap();
                let mut batch = ClaimedBatch::default();
                if !*claimed {
                    *claimed = true;
                    let now = chrono::Utc::now();
                    batch.push_row(
                        "i1".into(),
                        OutboxItemType::Event,
                        Some("g".into()),
                        "{}",
                        0,
                        None,
                        now,
                        now,
                    );
                }
                Ok(batch)
            }
            async fn mark_success(&self, _: OutboxItemType, _: &[String]) -> anyhow::Result<()> {
                Ok(())
            }
            async fn mark_failed(
                &self,
                _: OutboxItemType,
                _: &[String],
                _: OutboxStatus,
                _: &str,
                _: bool,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn release(&self, _: OutboxItemType, _: &[String]) -> anyhow::Result<()> {
                Ok(())
            }
            async fn requeue(&self, _: OutboxItemType, ids: &[String]) -> anyhow::Result<()> {
                self.requeued.lock().unwrap().extend(ids.iter().cloned());
                Ok(())
            }
            async fn recover_stuck(&self, _: Duration) -> anyhow::Result<u64> {
                Ok(0)
            }
            async fn init_schema(&self) -> anyhow::Result<()> {
                Ok(())
            }
            fn table_config(&self) -> &OutboxTableConfig {
                &self.config
            }
        }

        struct Refuse;

        #[async_trait]
        impl OutboxDispatcher for Refuse {
            async fn send_batch(&self, items: &[OutboxItem]) -> Vec<DispatchOutcome> {
                items
                    .iter()
                    .map(|_| DispatchOutcome::failed(OutboxStatus::Forbidden, "403"))
                    .collect()
            }
        }

        async fn call(app: &Router, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
            use tower::ServiceExt;
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, serde_json::from_slice(&bytes).unwrap())
        }

        #[tokio::test]
        async fn the_admin_api_answers_as_gos() {
            let repo = Arc::new(OneItem::default());
            let processor = Arc::new(EnhancedOutboxProcessor::with_dispatcher(
                EnhancedProcessorConfig::default(),
                repo.clone(),
                Arc::new(Refuse),
            ));
            let app = admin_router(processor.clone());

            assert_eq!(
                call(&app, "POST", "/outbox/groups/g/unblock").await,
                (
                    StatusCode::NOT_FOUND,
                    serde_json::json!({"error": "group not blocked"})
                )
            );

            processor.poll_once().await.unwrap();
            for _ in 0..200 {
                if !processor.blocked_groups().is_empty() {
                    break;
                }
                time::sleep(Duration::from_millis(5)).await;
            }
            assert_eq!(
                call(&app, "GET", "/outbox/groups/blocked").await,
                (
                    StatusCode::OK,
                    serde_json::json!({"blocked": [
                        {"group": "g", "status": "BLOCKED", "blockedItemId": "i1", "error": "403"}
                    ]})
                )
            );
            assert_eq!(
                call(&app, "POST", "/outbox/groups/g/unblock").await,
                (StatusCode::OK, serde_json::json!({"status": "UNBLOCKED"}))
            );
            assert_eq!(*repo.requeued.lock().unwrap(), vec!["i1"]);

            assert_eq!(
                call(&app, "POST", "/outbox/groups/p/pause").await,
                (StatusCode::OK, serde_json::json!({"status": "PAUSED"}))
            );
            assert_eq!(
                call(&app, "GET", "/outbox/groups").await,
                (
                    StatusCode::OK,
                    serde_json::json!({"groups": [{"group": "p", "status": "PAUSED"}]})
                )
            );
            assert_eq!(
                call(&app, "POST", "/outbox/groups/p/resume").await,
                (StatusCode::OK, serde_json::json!({"status": "RUNNING"}))
            );
            assert_eq!(
                call(&app, "POST", "/outbox/groups/p/skip").await.0,
                StatusCode::NOT_FOUND
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::process;

    #[tokio::test]
    async fn a_sqlite_outbox_is_connected_and_its_table_created() {
        let dir = env::temp_dir().join(format!("fc-outbox-setup-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite://{}?mode=rwc", dir.join("outbox.db").display());
        let repo = connect(OutboxBackend::Sqlite, &url, OutboxTableConfig::default())
            .await
            .unwrap();
        assert!(repo.claim_pending(10).await.unwrap().items.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}

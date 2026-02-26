use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::watch;
use tracing::{debug, error, info};

use crate::health::StreamHealth;

/// Projects events from `msg_event_projection_feed` into `msg_events_read`.
///
/// Runs a polling loop that picks up unprocessed feed rows, inserts them into
/// the read model via a single SQL CTE, and marks them as processed.
pub struct EventProjectionService {
    pool: PgPool,
    batch_size: u32,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    health: Arc<StreamHealth>,
}

impl EventProjectionService {
    pub fn new(pool: PgPool, batch_size: u32) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            pool,
            batch_size,
            shutdown_tx,
            shutdown_rx,
            health: Arc::new(StreamHealth::new("event-projection".to_string())),
        }
    }

    /// Returns the health tracker for this service.
    pub fn health(&self) -> Arc<StreamHealth> {
        self.health.clone()
    }

    /// Starts the projection loop in a background tokio task.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let pool = self.pool.clone();
        let batch_size = self.batch_size;
        let mut shutdown_rx = self.shutdown_rx.clone();
        let health = self.health.clone();

        tokio::spawn(async move {
            health.set_running(true);
            info!(
                "Event projection started (batch_size={})",
                batch_size
            );

            loop {
                if *shutdown_rx.borrow() {
                    break;
                }

                let sleep_ms = match poll_once(&pool, batch_size).await {
                    Ok(count) => {
                        if count > 0 {
                            health.add_processed(count as u64);
                            debug!("Projected {} events", count);
                        }
                        adaptive_sleep(count, batch_size)
                    }
                    Err(e) => {
                        error!("Event projection error: {}", e);
                        health.record_error();
                        5000
                    }
                };

                if sleep_ms > 0 {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
                        _ = shutdown_rx.changed() => { break; }
                    }
                }
            }

            health.set_running(false);
            info!("Event projection stopped");
        })
    }

    /// Signals the projection loop to stop.
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn poll_once(pool: &PgPool, batch_size: u32) -> anyhow::Result<u32> {
    let row: (i64,) = sqlx::query_as(
        r#"
        WITH batch AS (
            SELECT id, event_id, payload
            FROM msg_event_projection_feed
            WHERE processed = 0
            ORDER BY id
            LIMIT $1
        ),
        projected AS (
            INSERT INTO msg_events_read (
                id, spec_version, type, source, subject, time, data,
                correlation_id, causation_id, deduplication_id, message_group,
                client_id, application, subdomain, aggregate, projected_at
            )
            SELECT
                b.event_id,
                b.payload->>'specVersion',
                b.payload->>'type',
                b.payload->>'source',
                b.payload->>'subject',
                (b.payload->>'time')::timestamptz,
                b.payload->>'data',
                b.payload->>'correlationId',
                b.payload->>'causationId',
                b.payload->>'deduplicationId',
                b.payload->>'messageGroup',
                b.payload->>'clientId',
                split_part(b.payload->>'type', ':', 1),
                NULLIF(split_part(b.payload->>'type', ':', 2), ''),
                NULLIF(split_part(b.payload->>'type', ':', 3), ''),
                NOW()
            FROM batch b
            ON CONFLICT (id) DO NOTHING
        )
        UPDATE msg_event_projection_feed
        SET processed = 1, processed_at = NOW()
        WHERE id IN (SELECT id FROM batch)
        "#,
    )
    .bind(batch_size as i64)
    .fetch_one(pool)
    .await
    .map_err(|e| anyhow::anyhow!("event projection query failed: {}", e))?;

    Ok(row.0 as u32)
}

/// Returns how long to sleep (ms) based on how many rows were processed.
fn adaptive_sleep(count: u32, batch_size: u32) -> u64 {
    if count >= batch_size {
        0
    } else if count > 0 {
        100
    } else {
        1000
    }
}

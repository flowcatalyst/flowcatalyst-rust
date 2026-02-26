use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::watch;
use tracing::{debug, error, info};

use crate::health::StreamHealth;

/// Projects dispatch jobs from `msg_dispatch_job_projection_feed` into `msg_dispatch_jobs_read`.
///
/// Handles both INSERT (new jobs) and UPDATE (status changes) operations via
/// a single SQL CTE per poll cycle.
pub struct DispatchJobProjectionService {
    pool: PgPool,
    batch_size: u32,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    health: Arc<StreamHealth>,
}

impl DispatchJobProjectionService {
    pub fn new(pool: PgPool, batch_size: u32) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            pool,
            batch_size,
            shutdown_tx,
            shutdown_rx,
            health: Arc::new(StreamHealth::new("dispatch-job-projection".to_string())),
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
                "Dispatch job projection started (batch_size={})",
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
                            debug!("Projected {} dispatch jobs", count);
                        }
                        adaptive_sleep(count, batch_size)
                    }
                    Err(e) => {
                        error!("Dispatch job projection error: {}", e);
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
            info!("Dispatch job projection stopped");
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
            SELECT id, dispatch_job_id, operation, payload
            FROM msg_dispatch_job_projection_feed
            WHERE processed = 0
            ORDER BY id
            LIMIT $1
        ),
        projected_inserts AS (
            INSERT INTO msg_dispatch_jobs_read (
                id, external_id, source, kind, code, subject, event_id, correlation_id,
                target_url, protocol, service_account_id, client_id, subscription_id,
                mode, dispatch_pool_id, message_group, sequence, timeout_seconds,
                status, max_retries, retry_strategy, scheduled_for, expires_at,
                attempt_count, last_attempt_at, completed_at, duration_millis, last_error,
                idempotency_key, is_completed, is_terminal,
                application, subdomain, aggregate,
                created_at, updated_at, projected_at
            )
            SELECT
                b.dispatch_job_id,
                b.payload->>'externalId', b.payload->>'source', b.payload->>'kind',
                b.payload->>'code', b.payload->>'subject', b.payload->>'eventId',
                b.payload->>'correlationId', b.payload->>'targetUrl', b.payload->>'protocol',
                b.payload->>'serviceAccountId', b.payload->>'clientId',
                b.payload->>'subscriptionId', b.payload->>'mode',
                b.payload->>'dispatchPoolId', b.payload->>'messageGroup',
                (b.payload->>'sequence')::int, (b.payload->>'timeoutSeconds')::int,
                b.payload->>'status',
                COALESCE((b.payload->>'maxRetries')::int, 3),
                b.payload->>'retryStrategy',
                (b.payload->>'scheduledFor')::timestamptz,
                (b.payload->>'expiresAt')::timestamptz,
                COALESCE((b.payload->>'attemptCount')::int, 0),
                (b.payload->>'lastAttemptAt')::timestamptz,
                (b.payload->>'completedAt')::timestamptz,
                (b.payload->>'durationMillis')::bigint,
                b.payload->>'lastError', b.payload->>'idempotencyKey',
                (b.payload->>'isCompleted')::boolean, (b.payload->>'isTerminal')::boolean,
                split_part(b.payload->>'code', ':', 1),
                NULLIF(split_part(b.payload->>'code', ':', 2), ''),
                NULLIF(split_part(b.payload->>'code', ':', 3), ''),
                COALESCE((b.payload->>'createdAt')::timestamptz, NOW()),
                COALESCE((b.payload->>'updatedAt')::timestamptz, NOW()),
                NOW()
            FROM batch b
            WHERE b.operation = 'INSERT'
            ON CONFLICT (id) DO UPDATE SET
                status = EXCLUDED.status,
                attempt_count = EXCLUDED.attempt_count,
                last_attempt_at = EXCLUDED.last_attempt_at,
                completed_at = EXCLUDED.completed_at,
                duration_millis = EXCLUDED.duration_millis,
                last_error = EXCLUDED.last_error,
                is_completed = EXCLUDED.is_completed,
                is_terminal = EXCLUDED.is_terminal,
                updated_at = EXCLUDED.updated_at,
                projected_at = NOW()
        ),
        projected_updates AS (
            UPDATE msg_dispatch_jobs_read AS t
            SET
                status = COALESCE(src.payload->>'status', t.status),
                attempt_count = COALESCE((src.payload->>'attemptCount')::int, t.attempt_count),
                last_attempt_at = COALESCE((src.payload->>'lastAttemptAt')::timestamptz, t.last_attempt_at),
                completed_at = COALESCE((src.payload->>'completedAt')::timestamptz, t.completed_at),
                duration_millis = COALESCE((src.payload->>'durationMillis')::bigint, t.duration_millis),
                last_error = COALESCE(src.payload->>'lastError', t.last_error),
                is_completed = COALESCE((src.payload->>'isCompleted')::boolean, t.is_completed),
                is_terminal = COALESCE((src.payload->>'isTerminal')::boolean, t.is_terminal),
                updated_at = COALESCE((src.payload->>'updatedAt')::timestamptz, t.updated_at),
                projected_at = NOW()
            FROM (
                SELECT DISTINCT ON (dispatch_job_id) dispatch_job_id, payload
                FROM batch
                WHERE operation = 'UPDATE'
                ORDER BY dispatch_job_id, id DESC
            ) src
            WHERE t.id = src.dispatch_job_id
        )
        UPDATE msg_dispatch_job_projection_feed
        SET processed = 1, processed_at = NOW()
        WHERE id IN (SELECT id FROM batch)
        "#,
    )
    .bind(batch_size as i64)
    .fetch_one(pool)
    .await
    .map_err(|e| anyhow::anyhow!("dispatch job projection query failed: {}", e))?;

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

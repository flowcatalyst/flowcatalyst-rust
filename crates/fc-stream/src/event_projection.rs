use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::health::StreamHealth;

/// Health-tracker name for the event projection (reported by the stream
/// health endpoints).
pub const HEALTH_NAME: &str = "event-projection";

/// Projects events from `msg_events` into `msg_events_read` until `cancel`
/// fires.
///
/// Reads rows where `projected_at IS NULL`, inserts them into the read model
/// with parsed application/subdomain/aggregate fields, and stamps `projected_at`.
/// Cancellation is only observed between batches, so a batch in flight
/// always completes.
pub async fn run(
    pool: PgPool,
    batch_size: u32,
    health: Arc<StreamHealth>,
    cancel: CancellationToken,
) {
    health.set_running(true);
    info!("Event projection started (batch_size={})", batch_size);

    loop {
        if cancel.is_cancelled() {
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
                _ = cancel.cancelled() => { break; }
            }
        }
    }

    health.set_running(false);
    info!("Event projection stopped");
}

async fn poll_once(pool: &PgPool, batch_size: u32) -> anyhow::Result<u32> {
    // Select unprojected events, insert into read model, and stamp projected_at
    // in a single atomic CTE. The RETURNING clause gives us the count.
    let rows = sqlx::query_as::<_, (i32,)>(
        r#"
        WITH batch AS (
            SELECT id, created_at
            FROM msg_events
            WHERE projected_at IS NULL
            ORDER BY created_at
            LIMIT $1
        ),
        projected AS (
            INSERT INTO msg_events_read (
                id, spec_version, type, source, subject, time, data,
                correlation_id, causation_id, deduplication_id, message_group,
                client_id, application, subdomain, aggregate, created_at, projected_at
            )
            SELECT
                e.id,
                e.spec_version,
                e.type,
                e.source,
                e.subject,
                e.time,
                e.data::text,
                e.correlation_id,
                e.causation_id,
                e.deduplication_id,
                e.message_group,
                e.client_id,
                split_part(e.type, ':', 1),
                NULLIF(split_part(e.type, ':', 2), ''),
                NULLIF(split_part(e.type, ':', 3), ''),
                e.created_at,
                NOW()
            FROM msg_events e
            JOIN batch b ON b.id = e.id AND b.created_at = e.created_at
            ON CONFLICT (id, created_at) DO NOTHING
        )
        UPDATE msg_events m
        SET projected_at = NOW()
        FROM batch b
        WHERE m.id = b.id AND m.created_at = b.created_at
        RETURNING 1
        "#,
    )
    .bind(batch_size as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow::anyhow!("event projection query failed: {}", e))?;

    Ok(rows.len() as u32)
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- adaptive_sleep tests ---

    #[test]
    fn adaptive_sleep_idle_when_no_rows() {
        // count == 0 → long sleep (1000ms)
        assert_eq!(adaptive_sleep(0, 100), 1000);
        assert_eq!(adaptive_sleep(0, 1), 1000);
        assert_eq!(adaptive_sleep(0, 500), 1000);
    }

    #[test]
    fn adaptive_sleep_short_when_partial_batch() {
        // 0 < count < batch_size → short sleep (100ms)
        assert_eq!(adaptive_sleep(1, 100), 100);
        assert_eq!(adaptive_sleep(50, 100), 100);
        assert_eq!(adaptive_sleep(99, 100), 100);
    }

    #[test]
    fn adaptive_sleep_immediate_when_full_batch() {
        // count >= batch_size → no sleep (0ms), more rows likely waiting
        assert_eq!(adaptive_sleep(100, 100), 0);
        assert_eq!(adaptive_sleep(200, 100), 0); // over batch_size
        assert_eq!(adaptive_sleep(1, 1), 0); // exactly batch_size of 1
    }

    #[test]
    fn adaptive_sleep_boundary_at_batch_size() {
        let batch = 50;
        assert_eq!(adaptive_sleep(batch - 1, batch), 100); // just under
        assert_eq!(adaptive_sleep(batch, batch), 0); // exactly at
        assert_eq!(adaptive_sleep(batch + 1, batch), 0); // just over
    }

    // --- run() tests ---

    #[test]
    fn health_name_is_stable() {
        assert_eq!(HEALTH_NAME, "event-projection");
    }

    #[tokio::test]
    async fn run_exits_without_polling_when_already_cancelled() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let health = Arc::new(StreamHealth::new(HEALTH_NAME.to_string()));
        let cancel = CancellationToken::new();
        cancel.cancel();

        tokio::time::timeout(
            Duration::from_secs(1),
            run(pool, 100, health.clone(), cancel),
        )
        .await
        .expect("run should return promptly on a cancelled token");
        assert!(!health.is_running());
    }
}

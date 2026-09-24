//! Event fan-out projection.
//!
//! Sibling of [`event_projection`](crate::event_projection): both read
//! `msg_events` independently. Event projection owns `projected_at` and
//! writes to `msg_events_read`; fan-out owns `fanned_out_at` and writes
//! `msg_dispatch_jobs`. Independent partial indexes
//! (`idx_msg_events_unprojected`, `idx_msg_events_unfanned`) keep the two
//! claim queries cheap.
//!
//! Lives in fc-stream rather than fc-platform because it's pure stream
//! processing — no HTTP, no domain entities. Running it inside an HTTP
//! server would put it on every API node and create needless contention
//! on the claim query.
//!
//! At-least-once semantics: dispatch job inserts and `fanned_out_at` stamp
//! happen in one transaction. `FOR UPDATE SKIP LOCKED` on the claim makes
//! it safe to run multiple stream nodes against the same DB.
//!
//! Pure SQL: deliberately does not depend on `fc-platform`. The
//! subscription set is loaded with a small projection query and cached
//! locally; dispatch jobs are written via a raw `INSERT ... SELECT FROM
//! UNNEST(...)` against the table directly.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use fc_common::tsid::{self, EntityType};
use fc_common::{DispatchMode, DispatchStatus};
use sqlx::{PgPool, Postgres, Transaction};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::health::StreamHealth;

/// Configuration for the fan-out projection.
#[derive(Debug, Clone)]
pub struct EventFanOutConfig {
    /// Max events claimed per poll cycle.
    pub batch_size: u32,
    /// How often to refresh the in-memory subscription cache from the DB.
    /// Subs change rarely; a short refresh keeps fan-out coherent without
    /// querying every poll.
    pub subscription_refresh: Duration,
}

impl Default for EventFanOutConfig {
    fn default() -> Self {
        Self {
            batch_size: 200,
            subscription_refresh: Duration::from_secs(5),
        }
    }
}

/// Health-tracker name for the fan-out (reported by the stream health
/// endpoints).
pub const HEALTH_NAME: &str = "event-fan-out";

/// Fans `msg_events` rows out to `msg_dispatch_jobs` based on active
/// subscriptions, until `cancel` fires. Cancellation is only observed
/// between cycles, so a cycle in flight always completes.
pub async fn run(
    pool: PgPool,
    config: EventFanOutConfig,
    health: Arc<StreamHealth>,
    cancel: CancellationToken,
) {
    health.set_running(true);
    info!(batch_size = config.batch_size, "Event fan-out started");

    let mut subs_cache = SubscriptionCache::new(config.subscription_refresh);

    loop {
        if cancel.is_cancelled() {
            break;
        }

        if subs_cache.needs_refresh() {
            match load_active_subscriptions(&pool).await {
                Ok(subs) => subs_cache.replace(subs),
                Err(e) => {
                    warn!(error = %e, "Failed to refresh subscriptions; using stale cache");
                }
            }
        }

        let sleep_ms = match poll_once(&pool, subs_cache.subs(), &config).await {
            Ok(report) => {
                if report.events > 0 {
                    health.add_processed(report.events as u64);
                    debug!(events = report.events, jobs = report.jobs, "Fan-out cycle");
                }
                adaptive_sleep(report.events, config.batch_size)
            }
            Err(e) => {
                error!(error = %e, "Event fan-out cycle failed");
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
    info!("Event fan-out stopped");
}

struct CycleReport {
    events: u32,
    jobs: u32,
}

/// One poll cycle: claim a batch of unfanned events, build dispatch job
/// rows in memory, persist jobs + stamp `fanned_out_at` in a single
/// transaction.
async fn poll_once(
    pool: &PgPool,
    subscriptions: &[CachedSubscription],
    config: &EventFanOutConfig,
) -> anyhow::Result<CycleReport> {
    if subscriptions.is_empty() {
        // Nothing to fan out to. Still claim and stamp events so they
        // don't accumulate forever in the partial index.
        let claimed = claim_events_no_subs(pool, config.batch_size).await?;
        return Ok(CycleReport {
            events: claimed,
            jobs: 0,
        });
    }

    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;

    let claimed = claim_events(&mut tx, config.batch_size).await?;
    if claimed.is_empty() {
        tx.rollback().await.ok();
        return Ok(CycleReport { events: 0, jobs: 0 });
    }

    let mut jobs: Vec<NewJobRow> = Vec::new();

    for event in &claimed {
        for sub in subscriptions {
            if !sub.matches_event_type(&event.event_type) {
                continue;
            }
            if !sub.matches_client(event.client_id.as_deref()) {
                continue;
            }
            jobs.push(NewJobRow::build(event, sub));
        }
    }

    let job_count = jobs.len();
    if !jobs.is_empty() {
        insert_dispatch_jobs_tx(&mut tx, &jobs).await?;
    }
    tx.commit().await?;

    Ok(CycleReport {
        events: claimed.len() as u32,
        jobs: job_count as u32,
    })
}

// ── Event claim ───────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct EventClaimRow {
    id: String,
    #[sqlx(rename = "type")]
    event_type: String,
    source: String,
    subject: Option<String>,
    data: Option<serde_json::Value>,
    correlation_id: Option<String>,
    message_group: Option<String>,
    client_id: Option<String>,
    created_at: DateTime<Utc>,
}

async fn claim_events(
    tx: &mut Transaction<'_, Postgres>,
    batch_size: u32,
) -> anyhow::Result<Vec<EventClaimRow>> {
    let rows = sqlx::query_as::<_, EventClaimRow>(
        r#"
        WITH batch AS (
            SELECT id, created_at
            FROM msg_events
            WHERE fanned_out_at IS NULL
            ORDER BY created_at
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE msg_events e
        SET fanned_out_at = NOW()
        FROM batch b
        WHERE e.id = b.id AND e.created_at = b.created_at
        RETURNING
            e.id, e.type, e.source, e.subject, e.data,
            e.correlation_id, e.message_group, e.client_id, e.created_at
        "#,
    )
    .bind(batch_size as i64)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Fast path when there are no subscriptions: stamp events as fanned-out
/// without producing any jobs. Avoids holding a transaction open.
async fn claim_events_no_subs(pool: &PgPool, batch_size: u32) -> anyhow::Result<u32> {
    let row: (i64,) = sqlx::query_as(
        r#"
        WITH batch AS (
            SELECT id, created_at
            FROM msg_events
            WHERE fanned_out_at IS NULL
            ORDER BY created_at
            LIMIT $1
        )
        UPDATE msg_events e
        SET fanned_out_at = NOW()
        FROM batch b
        WHERE e.id = b.id AND e.created_at = b.created_at
        RETURNING (SELECT COUNT(*) FROM batch)
        "#,
    )
    .bind(batch_size as i64)
    .fetch_optional(pool)
    .await?
    .unwrap_or((0,));
    Ok(row.0 as u32)
}

// ── Subscription cache (raw SQL, no fc-platform dep) ──────────────────

#[derive(Debug, Clone)]
struct CachedSubscription {
    id: String,
    client_id: Option<String>,
    target: String,
    mode: DispatchMode,
    data_only: bool,
    dispatch_pool_id: Option<String>,
    service_account_id: Option<String>,
    max_retries: i32,
    timeout_seconds: i32,
    sequence: i32,
    connection_id: Option<String>,
    /// Wildcard-supporting `:`-separated event type patterns
    event_type_patterns: Vec<String>,
}

impl CachedSubscription {
    fn matches_event_type(&self, code: &str) -> bool {
        self.event_type_patterns
            .iter()
            .any(|p| pattern_matches(p, code))
    }

    fn matches_client(&self, event_client: Option<&str>) -> bool {
        match (&self.client_id, event_client) {
            (None, _) => true,
            (Some(sub), Some(evt)) => sub == evt,
            (Some(_), None) => false,
        }
    }
}

/// `:`-separated wildcard match (segment count must agree, `*` is wildcard).
fn pattern_matches(pattern: &str, code: &str) -> bool {
    let pp: Vec<&str> = pattern.split(':').collect();
    let cp: Vec<&str> = code.split(':').collect();
    if pp.len() != cp.len() {
        return false;
    }
    pp.iter().zip(cp.iter()).all(|(p, e)| *p == "*" || p == e)
}

#[derive(sqlx::FromRow)]
struct SubsRow {
    id: String,
    client_id: Option<String>,
    target: String,
    mode: String,
    data_only: bool,
    dispatch_pool_id: Option<String>,
    service_account_id: Option<String>,
    max_retries: i32,
    timeout_seconds: i32,
    sequence: i32,
    connection_id: Option<String>,
    event_type_code: Option<String>,
}

async fn load_active_subscriptions(pool: &PgPool) -> anyhow::Result<Vec<CachedSubscription>> {
    let rows: Vec<SubsRow> = sqlx::query_as::<_, SubsRow>(
        r#"
        SELECT
            s.id,
            s.client_id,
            s.target,
            s.mode,
            s.data_only,
            s.dispatch_pool_id,
            s.service_account_id,
            s.max_retries,
            s.timeout_seconds,
            s.sequence,
            s.connection_id,
            e.event_type_code
        FROM msg_subscriptions s
        LEFT JOIN msg_subscription_event_types e ON e.subscription_id = s.id
        WHERE s.status = 'ACTIVE'
        ORDER BY s.id
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut by_id: HashMap<String, CachedSubscription> = HashMap::new();
    for r in rows {
        let entry = by_id
            .entry(r.id.clone())
            .or_insert_with(|| CachedSubscription {
                id: r.id.clone(),
                client_id: r.client_id.clone(),
                target: r.target.clone(),
                mode: DispatchMode::from_str(&r.mode),
                data_only: r.data_only,
                dispatch_pool_id: r.dispatch_pool_id.clone(),
                service_account_id: r.service_account_id.clone(),
                max_retries: r.max_retries,
                timeout_seconds: r.timeout_seconds,
                sequence: r.sequence,
                connection_id: r.connection_id.clone(),
                event_type_patterns: Vec::new(),
            });
        if let Some(p) = r.event_type_code {
            entry.event_type_patterns.push(p);
        }
    }
    Ok(by_id.into_values().collect())
}

/// Tiny TTL cache for the active subscription set. Re-fetched periodically
/// rather than per-cycle to amortise the round-trip.
struct SubscriptionCache {
    subs: Vec<CachedSubscription>,
    last_refreshed: std::time::Instant,
    ttl: Duration,
}

impl SubscriptionCache {
    fn new(ttl: Duration) -> Self {
        Self {
            subs: Vec::new(),
            // Force initial refresh.
            last_refreshed: std::time::Instant::now() - ttl - Duration::from_millis(1),
            ttl,
        }
    }
    fn needs_refresh(&self) -> bool {
        self.last_refreshed.elapsed() >= self.ttl
    }
    fn replace(&mut self, subs: Vec<CachedSubscription>) {
        self.subs = subs;
        self.last_refreshed = std::time::Instant::now();
    }
    fn subs(&self) -> &[CachedSubscription] {
        &self.subs
    }
}

// ── Dispatch job insert ───────────────────────────────────────────────

/// One row to be inserted into `msg_dispatch_jobs`. Carries only the
/// columns fan-out actually sets — every other column (kind='EVENT',
/// retry_strategy='exponential', external_id=NULL, etc.) takes the
/// table default. `protocol` is set explicitly to 'HTTP_WEBHOOK' even
/// though the column defaults to the same value, to match the TS
/// fan-out path and keep the transport discriminator visible at the
/// call site.
struct NewJobRow {
    id: String,
    code: String,
    source: String,
    subject: Option<String>,
    event_id: String,
    correlation_id: Option<String>,
    target_url: String,
    protocol: &'static str,
    payload: String,
    data_only: bool,
    service_account_id: Option<String>,
    client_id: Option<String>,
    subscription_id: String,
    connection_id: Option<String>,
    mode: &'static str,
    dispatch_pool_id: Option<String>,
    message_group: Option<String>,
    sequence: i32,
    timeout_seconds: i32,
    status: &'static str,
    max_retries: i32,
    /// `{event.id}:{subscription.id}` — used by downstream consumers to
    /// dedupe redeliveries of the same fan-out pairing.
    idempotency_key: String,
    /// Inherits the source event's created_at so the scheduler's
    /// `ORDER BY created_at` preserves source order within a message
    /// group, and so events and their dispatch jobs land in the same
    /// monthly partition.
    created_at: DateTime<Utc>,
}

impl NewJobRow {
    fn build(event: &EventClaimRow, sub: &CachedSubscription) -> Self {
        let payload = serde_json::to_string(&event.data.clone().unwrap_or(serde_json::Value::Null))
            .unwrap_or_default();
        Self {
            id: tsid::generate(EntityType::DispatchJob),
            code: event.event_type.clone(),
            source: event.source.clone(),
            subject: event.subject.clone(),
            event_id: event.id.clone(),
            correlation_id: event.correlation_id.clone(),
            target_url: sub.target.clone(),
            protocol: "HTTP_WEBHOOK",
            payload,
            data_only: sub.data_only,
            service_account_id: sub.service_account_id.clone(),
            client_id: event.client_id.clone(),
            subscription_id: sub.id.clone(),
            connection_id: sub.connection_id.clone(),
            mode: dispatch_mode_str(sub.mode),
            dispatch_pool_id: sub.dispatch_pool_id.clone(),
            message_group: event.message_group.clone(),
            sequence: sub.sequence,
            timeout_seconds: sub.timeout_seconds,
            status: DispatchStatus::Pending.as_str(),
            max_retries: sub.max_retries,
            idempotency_key: format!("{}:{}", event.id, sub.id),
            created_at: event.created_at,
        }
    }
}

fn dispatch_mode_str(m: DispatchMode) -> &'static str {
    match m {
        DispatchMode::Immediate => "IMMEDIATE",
        DispatchMode::NextOnError => "NEXT_ON_ERROR",
        DispatchMode::BlockOnError => "BLOCK_ON_ERROR",
    }
}

async fn insert_dispatch_jobs_tx(
    tx: &mut Transaction<'_, Postgres>,
    jobs: &[NewJobRow],
) -> anyhow::Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }

    let mut ids = Vec::with_capacity(jobs.len());
    let mut codes = Vec::with_capacity(jobs.len());
    let mut sources = Vec::with_capacity(jobs.len());
    let mut subjects: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut event_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut correlation_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut target_urls = Vec::with_capacity(jobs.len());
    let mut protocols = Vec::with_capacity(jobs.len());
    let mut payloads = Vec::with_capacity(jobs.len());
    let mut data_onlys = Vec::with_capacity(jobs.len());
    let mut service_account_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut client_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut subscription_ids = Vec::with_capacity(jobs.len());
    let mut connection_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut modes = Vec::with_capacity(jobs.len());
    let mut dispatch_pool_ids: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut message_groups: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut sequences = Vec::with_capacity(jobs.len());
    let mut timeout_secs = Vec::with_capacity(jobs.len());
    let mut statuses = Vec::with_capacity(jobs.len());
    let mut max_retries_vec = Vec::with_capacity(jobs.len());
    let mut idempotency_keys = Vec::with_capacity(jobs.len());
    let mut created_ats = Vec::with_capacity(jobs.len());

    for j in jobs {
        ids.push(j.id.clone());
        codes.push(j.code.clone());
        sources.push(j.source.clone());
        subjects.push(j.subject.clone());
        event_ids.push(Some(j.event_id.clone()));
        correlation_ids.push(j.correlation_id.clone());
        target_urls.push(j.target_url.clone());
        protocols.push(j.protocol.to_string());
        payloads.push(j.payload.clone());
        data_onlys.push(j.data_only);
        service_account_ids.push(j.service_account_id.clone());
        client_ids.push(j.client_id.clone());
        subscription_ids.push(j.subscription_id.clone());
        connection_ids.push(j.connection_id.clone());
        modes.push(j.mode.to_string());
        dispatch_pool_ids.push(j.dispatch_pool_id.clone());
        message_groups.push(j.message_group.clone());
        sequences.push(j.sequence);
        timeout_secs.push(j.timeout_seconds);
        statuses.push(j.status.to_string());
        max_retries_vec.push(j.max_retries);
        idempotency_keys.push(j.idempotency_key.clone());
        created_ats.push(j.created_at);
    }

    sqlx::query(
        r#"
        INSERT INTO msg_dispatch_jobs (
            id, code, source, subject, event_id, correlation_id,
            target_url, protocol, payload, data_only, service_account_id, client_id,
            subscription_id, connection_id, mode, dispatch_pool_id, message_group,
            sequence, timeout_seconds, status, max_retries, idempotency_key,
            created_at, updated_at
        )
        SELECT * FROM UNNEST(
            $1::varchar[], $2::varchar[], $3::varchar[], $4::varchar[],
            $5::varchar[], $6::varchar[],
            $7::varchar[], $8::varchar[], $9::text[], $10::bool[], $11::varchar[], $12::varchar[],
            $13::varchar[], $14::varchar[], $15::varchar[], $16::varchar[], $17::varchar[],
            $18::int[], $19::int[], $20::varchar[], $21::int[], $22::varchar[],
            $23::timestamptz[], $23::timestamptz[]
        )
        "#,
    )
    .bind(&ids)
    .bind(&codes)
    .bind(&sources)
    .bind(&subjects)
    .bind(&event_ids)
    .bind(&correlation_ids)
    .bind(&target_urls)
    .bind(&protocols)
    .bind(&payloads)
    .bind(&data_onlys)
    .bind(&service_account_ids)
    .bind(&client_ids)
    .bind(&subscription_ids)
    .bind(&connection_ids)
    .bind(&modes)
    .bind(&dispatch_pool_ids)
    .bind(&message_groups)
    .bind(&sequences)
    .bind(&timeout_secs)
    .bind(&statuses)
    .bind(&max_retries_vec)
    .bind(&idempotency_keys)
    .bind(&created_ats)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Sleep duration based on cycle yield: 0 if the batch was full (more
/// rows likely waiting), 100ms for a partial batch, 1s when idle.
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

    #[test]
    fn pattern_matches_exact() {
        assert!(pattern_matches("a:b:c", "a:b:c"));
        assert!(!pattern_matches("a:b:c", "a:b:d"));
    }

    #[test]
    fn pattern_matches_wildcard() {
        assert!(pattern_matches("a:*:c", "a:b:c"));
        assert!(pattern_matches("*:*:*", "a:b:c"));
        assert!(!pattern_matches("a:*:c", "a:b:d"));
    }

    #[test]
    fn pattern_matches_segment_count_must_agree() {
        assert!(!pattern_matches("a:b:c", "a:b"));
        assert!(!pattern_matches("a:b", "a:b:c"));
        assert!(!pattern_matches("*:*", "a:b:c"));
    }

    #[test]
    fn matches_client_none_in_sub_matches_anything() {
        let sub = sample_sub(None);
        assert!(sub.matches_client(None));
        assert!(sub.matches_client(Some("clt_x")));
    }

    #[test]
    fn matches_client_explicit_must_match() {
        let sub = sample_sub(Some("clt_x".to_string()));
        assert!(sub.matches_client(Some("clt_x")));
        assert!(!sub.matches_client(Some("clt_y")));
        assert!(!sub.matches_client(None));
    }

    fn sample_sub(client_id: Option<String>) -> CachedSubscription {
        CachedSubscription {
            id: "sub_1".into(),
            client_id,
            target: "https://example.com".into(),
            mode: DispatchMode::Immediate,
            data_only: true,
            dispatch_pool_id: None,
            service_account_id: None,
            max_retries: 3,
            timeout_seconds: 30,
            sequence: 99,
            connection_id: None,
            event_type_patterns: vec![],
        }
    }

    #[test]
    fn dispatch_mode_strs_match_db_enum() {
        assert_eq!(dispatch_mode_str(DispatchMode::Immediate), "IMMEDIATE");
        assert_eq!(
            dispatch_mode_str(DispatchMode::NextOnError),
            "NEXT_ON_ERROR"
        );
        assert_eq!(
            dispatch_mode_str(DispatchMode::BlockOnError),
            "BLOCK_ON_ERROR"
        );
    }

    #[test]
    fn adaptive_sleep_idle() {
        assert_eq!(adaptive_sleep(0, 100), 1000);
    }

    #[test]
    fn adaptive_sleep_partial() {
        assert_eq!(adaptive_sleep(50, 100), 100);
    }

    #[test]
    fn adaptive_sleep_full() {
        assert_eq!(adaptive_sleep(100, 100), 0);
        assert_eq!(adaptive_sleep(150, 100), 0);
    }
}

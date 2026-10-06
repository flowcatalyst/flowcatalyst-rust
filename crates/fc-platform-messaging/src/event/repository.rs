//! Event Repository — PostgreSQL via SQLx
//!
//! Direct SQL queries with explicit control over what's fetched.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{ContextData, Event, EventFilterOptions, EventRead, CLOUDEVENTS_SPEC_VERSION};
use fc_platform_core::shared::api_common::DecodedCursor;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::error::Result;
use std::collections::HashSet;
use std::slice;

/// Row mapping for msg_events table
struct EventRow {
    id: String,
    spec_version: Option<String>,
    event_type: String,
    source: String,
    subject: Option<String>,
    time: DateTime<Utc>,
    data: Option<serde_json::Value>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    deduplication_id: Option<String>,
    message_group: Option<String>,
    client_id: Option<String>,
    context_data: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

impl From<EventRow> for Event {
    fn from(r: EventRow) -> Self {
        let context_data: Vec<ContextData> = r
            .context_data
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        Self {
            id: r.id,
            event_type: r.event_type,
            source: r.source,
            subject: r.subject,
            time: r.time,
            data: r.data.unwrap_or(serde_json::Value::Null),
            spec_version: r
                .spec_version
                .unwrap_or_else(|| CLOUDEVENTS_SPEC_VERSION.to_string()),
            message_group: r.message_group,
            correlation_id: r.correlation_id,
            causation_id: r.causation_id,
            deduplication_id: r.deduplication_id,
            client_id: r.client_id,
            context_data,
            created_at: r.created_at,
        }
    }
}

/// Row mapping for msg_events_read table
#[derive(sqlx::FromRow)]
struct EventReadRow {
    id: String,
    #[sqlx(rename = "type")]
    event_type: String,
    source: String,
    subject: Option<String>,
    time: DateTime<Utc>,
    application: Option<String>,
    subdomain: Option<String>,
    aggregate: Option<String>,
    message_group: Option<String>,
    correlation_id: Option<String>,
    client_id: Option<String>,
    projected_at: DateTime<Utc>,
}

impl From<EventReadRow> for EventRead {
    fn from(r: EventReadRow) -> Self {
        Self {
            id: r.id,
            event_type: r.event_type,
            source: r.source,
            subject: r.subject,
            time: r.time,
            application: r.application,
            subdomain: r.subdomain,
            aggregate: r.aggregate,
            message_group: r.message_group,
            correlation_id: r.correlation_id,
            client_id: r.client_id,
            client_name: None,
            projected_at: r.projected_at,
        }
    }
}

/// One `msg_events_read` row in full: Go's `FindByID` reads every column
/// (event/repository.go), for `GET /api/events/{id}`, plus the event's
/// context data from its `msg_events` row (the projection has no column for
/// it; Go documents `contextData` on this read but never fills it).
#[derive(Debug, Clone)]
pub struct EventReadDetail {
    pub id: String,
    pub spec_version: Option<String>,
    pub event_type: String,
    pub source: String,
    pub subject: Option<String>,
    pub time: DateTime<Utc>,
    pub data: Option<String>,
    pub deduplication_id: Option<String>,
    pub client_id: Option<String>,
    pub message_group: Option<String>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub application: Option<String>,
    pub subdomain: Option<String>,
    pub aggregate: Option<String>,
    pub projected_at: Option<DateTime<Utc>>,
    /// `msg_events.context_data` as stored (`[{key, value}]`); `None` when the
    /// write-side row is gone or carries none.
    pub context_data: Option<serde_json::Value>,
}

impl EventReadDetail {
    /// The context entries, read leniently as the write-side read does: a
    /// document that is not `[{key, value}]` reads as none.
    pub fn context_entries(&self) -> Vec<ContextData> {
        self.context_data
            .clone()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }
}

/// Go's event list filters (`event.FilterParams`): singular equality
/// filters for SDK callers, CSV multi-filters for the SPA, and
/// `accessible` scoping a non-anchor caller to platform events plus its
/// clients' events.
#[derive(Debug, Default)]
pub struct EventReadFilter<'a> {
    pub event_type: Option<&'a str>,
    pub types: &'a [String],
    pub source: Option<&'a str>,
    pub subject: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub client_ids: &'a [String],
    pub accessible: Option<&'a [String]>,
    pub applications: &'a [String],
    pub subdomains: &'a [String],
    pub aggregates: &'a [String],
    pub correlation_id: Option<&'a str>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: i64,
    pub offset: i64,
}

pub struct EventRepository {
    pool: PgPool,
}

/// Each event's `created_at`, strictly increasing in batch order at the
/// column's microsecond precision. The stream fan-out reads events `ORDER BY
/// created_at` and gives their dispatch jobs the event's `created_at`, which
/// the scheduler orders a message group by — so a batch must keep its order
/// there. Go stamps each event with its own `time.Now()` (event.New); one
/// `NOW()` for the whole batch made every member of a group tie, and the
/// group went out in whatever order the fan-out happened to read it
/// (delivery run 3, `router-restart`: g3 and g4 delivered 10 first).
fn batch_created_at(times: impl Iterator<Item = DateTime<Utc>>) -> Vec<DateTime<Utc>> {
    use chrono::DurationRound;
    let tick = chrono::TimeDelta::microseconds(1);
    let mut out: Vec<DateTime<Utc>> = Vec::new();
    for t in times {
        let t = t.duration_trunc(tick).unwrap_or(t);
        let t = match out.last() {
            Some(prev) if t <= *prev => *prev + tick,
            _ => t,
        };
        out.push(t);
    }
    out
}

impl EventRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Store one event, idempotently: see [`Self::insert_many`].
    pub async fn insert(&self, event: &Event) -> Result<()> {
        self.insert_many(slice::from_ref(event)).await?;
        Ok(())
    }

    /// Store a batch of events idempotently, as Go's `InsertBatch`
    /// (flowcatalyst-go internal/platform/event/repository.go:31-140): an
    /// event whose deduplication id is already stored, or repeats an earlier
    /// one in the batch, is dropped (one lookup for the whole batch); the
    /// rest go in with one UNNEST `INSERT … ON CONFLICT DO NOTHING`. The
    /// unique index is `(deduplication_id, created_at)` on the partitioned
    /// table, so the conflict clause alone only catches a repeat with the
    /// same `created_at`: the lookup is the dedup and the clause the last
    /// line of defence. An event with no deduplication id is always stored.
    ///
    /// Returns how many events were stored.
    pub async fn insert_many(&self, events: &[Event]) -> Result<u64> {
        let events = self.drop_duplicates(events).await?;
        if events.is_empty() {
            return Ok(0);
        }

        let mut ids = Vec::with_capacity(events.len());
        let mut spec_versions = Vec::with_capacity(events.len());
        let mut types = Vec::with_capacity(events.len());
        let mut sources = Vec::with_capacity(events.len());
        let mut subjects: Vec<Option<String>> = Vec::with_capacity(events.len());
        let mut times = Vec::with_capacity(events.len());
        let mut datas: Vec<serde_json::Value> = Vec::with_capacity(events.len());
        let mut correlation_ids: Vec<Option<String>> = Vec::with_capacity(events.len());
        let mut causation_ids: Vec<Option<String>> = Vec::with_capacity(events.len());
        let mut deduplication_ids: Vec<Option<String>> = Vec::with_capacity(events.len());
        let mut message_groups: Vec<Option<String>> = Vec::with_capacity(events.len());
        let mut client_ids: Vec<Option<String>> = Vec::with_capacity(events.len());
        let mut context_datas: Vec<Option<serde_json::Value>> = Vec::with_capacity(events.len());
        let created_ats = batch_created_at(events.iter().map(|e| e.created_at));

        for event in events {
            ids.push(event.id.as_str());
            spec_versions.push(event.spec_version.as_str());
            types.push(event.event_type.as_str());
            sources.push(event.source.as_str());
            subjects.push(event.subject.clone());
            times.push(event.time);
            datas.push(event.data.clone());
            correlation_ids.push(event.correlation_id.clone());
            causation_ids.push(event.causation_id.clone());
            deduplication_ids.push(event.deduplication_id.clone());
            message_groups.push(event.message_group.clone());
            client_ids.push(event.client_id.clone());
            context_datas.push(if event.context_data.is_empty() {
                None
            } else {
                serde_json::to_value(&event.context_data).ok()
            });
        }

        let result = sqlx::query!(
            r#"INSERT INTO msg_events
                (id, spec_version, type, source, subject, time, data,
                 correlation_id, causation_id, deduplication_id,
                 message_group, client_id, context_data, created_at)
            SELECT * FROM UNNEST(
                $1::varchar[], $2::varchar[], $3::varchar[], $4::varchar[],
                $5::varchar[], $6::timestamptz[], $7::jsonb[],
                $8::varchar[], $9::varchar[], $10::varchar[],
                $11::varchar[], $12::varchar[], $13::jsonb[], $14::timestamptz[]
            )
            ON CONFLICT DO NOTHING"#,
            &ids as &[&str],
            &spec_versions as &[&str],
            &types as &[&str],
            &sources as &[&str],
            &&subjects as &[Option<String>] as &[Option<String>],
            &times,
            &datas,
            &&correlation_ids as &[Option<String>] as &[Option<String>],
            &&causation_ids as &[Option<String>] as &[Option<String>],
            &&deduplication_ids as &[Option<String>] as &[Option<String>],
            &&message_groups as &[Option<String>] as &[Option<String>],
            &&client_ids as &[Option<String>] as &[Option<String>],
            &context_datas as &[Option<serde_json::Value>],
            &created_ats
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// The events of `events` to store: those whose deduplication id is not
    /// stored yet, first occurrence winning within the batch (Go
    /// `dropDuplicates`, event/repository.go:96-140).
    async fn drop_duplicates<'e>(&self, events: &'e [Event]) -> Result<Vec<&'e Event>> {
        let dedup_ids: Vec<&str> = events
            .iter()
            .filter_map(|e| e.deduplication_id.as_deref())
            .filter(|d| !d.is_empty())
            .collect();
        let stored: HashSet<String> = if dedup_ids.is_empty() {
            HashSet::new()
        } else {
            // The column is nullable; `= ANY($1)` never matches a NULL.
            sqlx::query!(
                "SELECT DISTINCT deduplication_id FROM msg_events WHERE deduplication_id = ANY($1)",
                &dedup_ids as &[&str]
            )
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .filter_map(|r| r.deduplication_id)
            .collect()
        };
        let mut seen = HashSet::new();
        Ok(events
            .iter()
            .filter(
                |e| match e.deduplication_id.as_deref().filter(|d| !d.is_empty()) {
                    None => true,
                    Some(d) => !stored.contains(d) && seen.insert(d),
                },
            )
            .collect())
    }

    /// Which of `ids` already name a stored event, across every partition
    /// (one query). Ingest uses it to acknowledge a re-sent event whose
    /// caller-supplied id is stored without writing it again.
    pub async fn find_existing_ids(&self, ids: &[String]) -> Result<HashSet<String>> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let rows = sqlx::query!("SELECT DISTINCT id FROM msg_events WHERE id = ANY($1)", ids)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.id).collect())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Event>> {
        let row = sqlx::query_as!(
            EventRow,
            "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Event::from))
    }

    pub async fn find_by_type(&self, event_type: &str, limit: i64) -> Result<Vec<Event>> {
        let rows = sqlx::query_as!(
            EventRow,
            "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events WHERE type = $1 ORDER BY time DESC LIMIT $2",
            event_type,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &str, limit: i64) -> Result<Vec<Event>> {
        let rows = sqlx::query_as!(
            EventRow,
            "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events WHERE client_id = $1 ORDER BY time DESC LIMIT $2",
            client_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_correlation_id(&self, correlation_id: &str) -> Result<Vec<Event>> {
        let rows = sqlx::query_as!(
            EventRow,
            "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events WHERE correlation_id = $1 ORDER BY time DESC",
            correlation_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Event::from).collect())
    }

    /// The stored events carrying any of these deduplication ids, in one
    /// query.
    pub async fn find_by_deduplication_ids(
        &self,
        deduplication_ids: &[String],
    ) -> Result<Vec<Event>> {
        if deduplication_ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query_as!(
            EventRow,
            "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events WHERE deduplication_id = ANY($1)",
            deduplication_ids
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_deduplication_id(&self, deduplication_id: &str) -> Result<Option<Event>> {
        let row = sqlx::query_as!(
            EventRow,
            "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events WHERE deduplication_id = $1",
            deduplication_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Event::from))
    }

    /// Cursor-paginated raw events (write table). Keyset on
    /// `(created_at, id) DESC` matches the existing index pattern. Returns
    /// up to `fetch_limit` rows so the caller can detect `hasMore` cheaply.
    pub async fn find_recent_with_cursor(
        &self,
        cursor: Option<&DecodedCursor>,
        fetch_limit: i64,
    ) -> Result<Vec<Event>> {
        let rows = if let Some(c) = cursor {
            sqlx::query_as!(
                EventRow,
                "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events \
                 WHERE (created_at, id) < ($1, $2) \
                 ORDER BY created_at DESC, id DESC LIMIT $3",
                c.created_at,
                &c.id,
                fetch_limit
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as!(
                EventRow,
                "SELECT id, spec_version, type AS event_type, source, subject, time, \
                    data AS \"data: serde_json::Value\", correlation_id, causation_id, \
                    deduplication_id, message_group, client_id, \
                    context_data AS \"context_data: serde_json::Value\", created_at \
                    FROM msg_events ORDER BY created_at DESC, id DESC LIMIT $1",
                fetch_limit
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn count_all(&self) -> Result<u64> {
        let row = sqlx::query_scalar!("SELECT COUNT(*) AS \"count!\" FROM msg_events")
            .fetch_one(&self.pool)
            .await?;

        Ok(row as u64)
    }

    // ── Read projection methods ──────────────────────────────────────────

    pub async fn find_read_by_id(&self, id: &str) -> Result<Option<EventRead>> {
        let row = sqlx::query_as!(
            EventReadRow,
            "SELECT id, type AS event_type, source, subject, time, application, \
                    subdomain, aggregate, message_group, correlation_id, client_id, \
                    projected_at \
                    \
             FROM msg_events_read WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(EventRead::from))
    }

    /// One read-projection row in full (Go `FindByID`).
    pub async fn find_read_detail_by_id(&self, id: &str) -> Result<Option<EventReadDetail>> {
        // The projection keeps the source row's `created_at` (its partition
        // key), so the join reaches one partition of `msg_events`.
        Ok(sqlx::query_as!(
            EventReadDetail,
            "SELECT r.id, r.spec_version, r.type AS event_type, r.source, r.subject, \
                    r.time, r.data, r.deduplication_id, r.client_id, r.message_group, \
                    r.correlation_id, r.causation_id, r.created_at, r.application, \
                    r.subdomain, r.aggregate, r.projected_at, \
                    e.context_data AS \"context_data?: serde_json::Value\" \
                    \
             FROM msg_events_read r \
             LEFT JOIN msg_events e ON e.id = r.id AND e.created_at = r.created_at \
             WHERE r.id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await?)
    }

    /// The read projection filtered as Go's `FindWithFilters`, newest first.
    pub async fn find_read_filtered(&self, f: &EventReadFilter<'_>) -> Result<Vec<EventRead>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT id, type, source, subject, time, application, subdomain, \
             aggregate, message_group, correlation_id, client_id, projected_at \
             FROM msg_events_read WHERE TRUE",
        );
        let eq = |qb: &mut QueryBuilder<Postgres>, col: &str, v: Option<&str>| {
            if let Some(v) = v {
                qb.push(format!(" AND {col} = ")).push_bind(v.to_string());
            }
        };
        let any = |qb: &mut QueryBuilder<Postgres>, col: &str, v: &[String]| {
            if !v.is_empty() {
                qb.push(format!(" AND {col} = ANY("))
                    .push_bind(v.to_vec())
                    .push(")");
            }
        };
        eq(&mut qb, "type", f.event_type);
        any(&mut qb, "type", f.types);
        eq(&mut qb, "source", f.source);
        eq(&mut qb, "subject", f.subject);
        eq(&mut qb, "client_id", f.client_id);
        any(&mut qb, "client_id", f.client_ids);
        if let Some(ids) = f.accessible {
            qb.push(" AND (client_id IS NULL OR client_id = ANY(")
                .push_bind(ids.to_vec())
                .push("))");
        }
        any(&mut qb, "application", f.applications);
        any(&mut qb, "subdomain", f.subdomains);
        any(&mut qb, "aggregate", f.aggregates);
        eq(&mut qb, "correlation_id", f.correlation_id);
        if let Some(t) = f.since {
            qb.push(" AND created_at >= ").push_bind(t);
        }
        if let Some(t) = f.until {
            qb.push(" AND created_at <= ").push_bind(t);
        }
        qb.push(" ORDER BY created_at DESC LIMIT ")
            .push_bind(f.limit);
        if f.offset > 0 {
            qb.push(" OFFSET ").push_bind(f.offset);
        }
        let rows: Vec<EventReadRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(EventRead::from).collect())
    }

    /// Distinct non-null values of one read-projection column, sorted, at
    /// most 200 (Go `DistinctValues`).
    pub async fn distinct_read_values(&self, column: &str) -> Result<Vec<String>> {
        if !["application", "subdomain", "type"].contains(&column) {
            return Err(PlatformError::internal(format!(
                "event repo: column {column:?} not allowed"
            )));
        }
        Ok(sqlx::query_scalar::<_, String>(&format!(
            "SELECT DISTINCT {column} FROM msg_events_read WHERE {column} IS NOT NULL \
             ORDER BY 1 LIMIT 200"
        ))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Cursor-paginated read of `msg_events_read`. Drops the
    /// `SELECT COUNT(*)` (msg_events_read can be billions of rows) and the
    /// configurable sort — always orders by `(time DESC, id DESC)` so the
    /// keyset comparison is well-defined. Returns `size + 1` rows so the
    /// caller can detect `hasMore` without a count.
    #[allow(clippy::too_many_arguments)]
    pub async fn find_read_with_cursor(
        &self,
        client_ids: &[String],
        applications: &[String],
        subdomains: &[String],
        aggregates: &[String],
        event_types: &[String],
        correlation_id: Option<&str>,
        search: Option<&str>,
        cursor: Option<&DecodedCursor>,
        fetch_limit: i64,
    ) -> Result<Vec<EventRead>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT id, type, source, subject, time, application, subdomain, \
             aggregate, message_group, correlation_id, client_id, projected_at \
             FROM msg_events_read",
        );

        let search_pattern = search
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{}%", s));

        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if !client_ids.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("client_id = ANY(").push_bind(client_ids).push(")");
        }
        if !applications.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("application = ANY(")
                .push_bind(applications)
                .push(")");
        }
        if !subdomains.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("subdomain = ANY(").push_bind(subdomains).push(")");
        }
        if !aggregates.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("aggregate = ANY(").push_bind(aggregates).push(")");
        }
        if !event_types.is_empty() {
            push_where(&mut qb, &mut has_where);
            qb.push("type = ANY(").push_bind(event_types).push(")");
        }
        if let Some(v) = correlation_id {
            push_where(&mut qb, &mut has_where);
            qb.push("correlation_id = ").push_bind(v.to_string());
        }
        if let Some(pattern) = search_pattern {
            push_where(&mut qb, &mut has_where);
            qb.push("(type ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR source ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR subject ILIKE ")
                .push_bind(pattern.clone())
                .push(")");
        }
        // Keyset comparison: rows strictly older than the cursor's
        // (time, id) tuple. Postgres tuple comparison is lexicographic and
        // matches the index on (time DESC, id DESC) directly.
        if let Some(c) = cursor {
            push_where(&mut qb, &mut has_where);
            qb.push("(time, id) < (")
                .push_bind(c.created_at)
                .push(", ")
                .push_bind(c.id.clone())
                .push(")");
        }

        qb.push(" ORDER BY time DESC, id DESC LIMIT ")
            .push_bind(fetch_limit);

        let rows: Vec<EventReadRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(EventRead::from).collect())
    }

    /// Get distinct filter option values from the read model.
    pub async fn read_filter_options(&self) -> Result<EventFilterOptions> {
        // `application!`: the WHERE excludes NULLs of this nullable column.
        let applications = sqlx::query_scalar!(
            "SELECT DISTINCT application AS \"application!\" FROM msg_events_read WHERE application IS NOT NULL ORDER BY application"
        ).fetch_all(&self.pool).await?;

        // `subdomain!`: the WHERE excludes NULLs of this nullable column.
        let subdomains = sqlx::query_scalar!(
            "SELECT DISTINCT subdomain AS \"subdomain!\" FROM msg_events_read WHERE subdomain IS NOT NULL ORDER BY subdomain"
        ).fetch_all(&self.pool).await?;

        // `aggregate!`: the WHERE excludes NULLs of this nullable column.
        let aggregates = sqlx::query_scalar!(
            "SELECT DISTINCT aggregate AS \"aggregate!\" FROM msg_events_read WHERE aggregate IS NOT NULL ORDER BY aggregate"
        ).fetch_all(&self.pool).await?;

        let types = sqlx::query_scalar!("SELECT DISTINCT type FROM msg_events_read ORDER BY type")
            .fetch_all(&self.pool)
            .await?;

        Ok(EventFilterOptions {
            applications,
            subdomains,
            aggregates,
            types,
        })
    }

    pub async fn insert_read_projection(&self, p: &EventRead) -> Result<()> {
        sqlx::query!(
            r#"INSERT INTO msg_events_read
                (id, type, source, subject, time, application, subdomain,
                 aggregate, message_group, correlation_id, client_id, projected_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW())"#,
            &p.id,
            &p.event_type,
            &p.source,
            p.subject.as_ref(),
            p.time,
            p.application.as_ref(),
            p.subdomain.as_ref(),
            p.aggregate.as_ref(),
            p.message_group.as_ref(),
            p.correlation_id.as_ref(),
            p.client_id.as_ref()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_read_projection(&self, p: &EventRead) -> Result<()> {
        sqlx::query!(
            r#"UPDATE msg_events_read SET
                type = $2, source = $3, subject = $4, time = $5,
                application = $6, subdomain = $7, aggregate = $8,
                message_group = $9, correlation_id = $10, client_id = $11,
                projected_at = NOW()
            WHERE id = $1"#,
            &p.id,
            &p.event_type,
            &p.source,
            p.subject.as_ref(),
            p.time,
            p.application.as_ref(),
            p.subdomain.as_ref(),
            p.aggregate.as_ref(),
            p.message_group.as_ref(),
            p.correlation_id.as_ref(),
            p.client_id.as_ref()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod batch_created_at_tests {
    use super::*;
    use chrono::DurationRound;

    /// Events stamped in the same microsecond (or out of order) still get
    /// strictly increasing `created_at`s in batch order, so the fan-out
    /// keeps a batch's message groups in the order they were sent.
    #[test]
    fn a_batch_keeps_its_order_in_created_at() {
        let t = DateTime::parse_from_rfc3339("2026-09-25T10:00:00.000001500Z")
            .unwrap()
            .with_timezone(&Utc);
        let us = chrono::TimeDelta::microseconds(1);
        let got = batch_created_at([t, t, t - us, t + us * 5, t + us * 5].into_iter());
        let base = t.duration_trunc(us).unwrap();
        assert_eq!(
            got,
            vec![base, base + us, base + us * 2, base + us * 5, base + us * 6]
        );
        assert!(got.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn distinct_times_are_kept() {
        let t = Utc::now();
        let ms = chrono::TimeDelta::milliseconds(1);
        let times = vec![t, t + ms, t + ms * 2];
        let got = batch_created_at(times.clone().into_iter());
        let us = chrono::TimeDelta::microseconds(1);
        let want: Vec<_> = times
            .iter()
            .map(|x| x.duration_trunc(us).unwrap())
            .collect();
        assert_eq!(got, want);
    }
}

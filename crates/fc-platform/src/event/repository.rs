//! Event Repository — PostgreSQL via SQLx
//!
//! Direct SQL queries with explicit control over what's fetched.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{ContextData, Event, EventFilterOptions, EventRead, CLOUDEVENTS_SPEC_VERSION};
use crate::shared::error::Result;

/// Row mapping for msg_events table
#[derive(sqlx::FromRow)]
struct EventRow {
    id: String,
    spec_version: Option<String>,
    #[sqlx(rename = "type")]
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
        self.insert_many(std::slice::from_ref(event)).await?;
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

        let result = sqlx::query(
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
        )
        .bind(&ids)
        .bind(&spec_versions)
        .bind(&types)
        .bind(&sources)
        .bind(&subjects as &[Option<String>])
        .bind(&times)
        .bind(&datas)
        .bind(&correlation_ids as &[Option<String>])
        .bind(&causation_ids as &[Option<String>])
        .bind(&deduplication_ids as &[Option<String>])
        .bind(&message_groups as &[Option<String>])
        .bind(&client_ids as &[Option<String>])
        .bind(&context_datas as &[Option<serde_json::Value>])
        .bind(&created_ats)
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
        let stored: std::collections::HashSet<String> = if dedup_ids.is_empty() {
            std::collections::HashSet::new()
        } else {
            sqlx::query_as::<_, (String,)>(
                "SELECT DISTINCT deduplication_id FROM msg_events WHERE deduplication_id = ANY($1)",
            )
            .bind(&dedup_ids)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|(d,)| d)
            .collect()
        };
        let mut seen = std::collections::HashSet::new();
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
    pub async fn find_existing_ids(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashSet<String>> {
        if ids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        let rows =
            sqlx::query_as::<_, (String,)>("SELECT DISTINCT id FROM msg_events WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Event>> {
        let row = sqlx::query_as::<_, EventRow>("SELECT * FROM msg_events WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(Event::from))
    }

    pub async fn find_by_type(&self, event_type: &str, limit: i64) -> Result<Vec<Event>> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT * FROM msg_events WHERE type = $1 ORDER BY time DESC LIMIT $2",
        )
        .bind(event_type)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &str, limit: i64) -> Result<Vec<Event>> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT * FROM msg_events WHERE client_id = $1 ORDER BY time DESC LIMIT $2",
        )
        .bind(client_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_correlation_id(&self, correlation_id: &str) -> Result<Vec<Event>> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT * FROM msg_events WHERE correlation_id = $1 ORDER BY time DESC",
        )
        .bind(correlation_id)
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
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT * FROM msg_events WHERE deduplication_id = ANY($1)",
        )
        .bind(deduplication_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_deduplication_id(&self, deduplication_id: &str) -> Result<Option<Event>> {
        let row =
            sqlx::query_as::<_, EventRow>("SELECT * FROM msg_events WHERE deduplication_id = $1")
                .bind(deduplication_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(Event::from))
    }

    /// Cursor-paginated raw events (write table). Keyset on
    /// `(created_at, id) DESC` matches the existing index pattern. Returns
    /// up to `fetch_limit` rows so the caller can detect `hasMore` cheaply.
    pub async fn find_recent_with_cursor(
        &self,
        cursor: Option<&crate::shared::api_common::DecodedCursor>,
        fetch_limit: i64,
    ) -> Result<Vec<Event>> {
        let rows = if let Some(c) = cursor {
            sqlx::query_as::<_, EventRow>(
                "SELECT * FROM msg_events \
                 WHERE (created_at, id) < ($1, $2) \
                 ORDER BY created_at DESC, id DESC LIMIT $3",
            )
            .bind(c.created_at)
            .bind(&c.id)
            .bind(fetch_limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, EventRow>(
                "SELECT * FROM msg_events ORDER BY created_at DESC, id DESC LIMIT $1",
            )
            .bind(fetch_limit)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(Event::from).collect())
    }

    pub async fn count_all(&self) -> Result<u64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM msg_events")
            .fetch_one(&self.pool)
            .await?;

        Ok(row.0 as u64)
    }

    // ── Read projection methods ──────────────────────────────────────────

    pub async fn find_read_by_id(&self, id: &str) -> Result<Option<EventRead>> {
        let row = sqlx::query_as::<_, EventReadRow>(
            "SELECT id, type, source, subject, time, application, subdomain, \
             aggregate, message_group, correlation_id, client_id, projected_at \
             FROM msg_events_read WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(EventRead::from))
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
        cursor: Option<&crate::shared::api_common::DecodedCursor>,
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
        let applications = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT application FROM msg_events_read WHERE application IS NOT NULL ORDER BY application"
        ).fetch_all(&self.pool).await?;

        let subdomains = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT subdomain FROM msg_events_read WHERE subdomain IS NOT NULL ORDER BY subdomain"
        ).fetch_all(&self.pool).await?;

        let aggregates = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT aggregate FROM msg_events_read WHERE aggregate IS NOT NULL ORDER BY aggregate"
        ).fetch_all(&self.pool).await?;

        let types = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT type FROM msg_events_read ORDER BY type",
        )
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
        sqlx::query(
            r#"INSERT INTO msg_events_read
                (id, type, source, subject, time, application, subdomain,
                 aggregate, message_group, correlation_id, client_id, projected_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW())"#,
        )
        .bind(&p.id)
        .bind(&p.event_type)
        .bind(&p.source)
        .bind(&p.subject)
        .bind(p.time)
        .bind(&p.application)
        .bind(&p.subdomain)
        .bind(&p.aggregate)
        .bind(&p.message_group)
        .bind(&p.correlation_id)
        .bind(&p.client_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_read_projection(&self, p: &EventRead) -> Result<()> {
        sqlx::query(
            r#"UPDATE msg_events_read SET
                type = $2, source = $3, subject = $4, time = $5,
                application = $6, subdomain = $7, aggregate = $8,
                message_group = $9, correlation_id = $10, client_id = $11,
                projected_at = NOW()
            WHERE id = $1"#,
        )
        .bind(&p.id)
        .bind(&p.event_type)
        .bind(&p.source)
        .bind(&p.subject)
        .bind(p.time)
        .bind(&p.application)
        .bind(&p.subdomain)
        .bind(&p.aggregate)
        .bind(&p.message_group)
        .bind(&p.correlation_id)
        .bind(&p.client_id)
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

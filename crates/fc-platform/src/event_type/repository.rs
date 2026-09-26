//! EventType Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{EventType, EventTypeStatus, SpecVersion};
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;

/// Row mapping for msg_event_types table
#[derive(sqlx::FromRow)]
struct EventTypeRow {
    id: String,
    code: String,
    name: String,
    description: Option<String>,
    status: String,
    source: String,
    client_scoped: bool,
    application: String,
    subdomain: String,
    aggregate: String,
    created_by: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<EventTypeRow> for EventType {
    type Error = PlatformError;
    fn try_from(r: EventTypeRow) -> Result<Self> {
        let status = decode(&r.status, "msg_event_types", "status", &r.id)?;
        let source = decode(&r.source, "msg_event_types", "source", &r.id)?;
        let event_name = r.code.split(':').nth(3).unwrap_or("").to_string();
        Ok(Self {
            id: r.id,
            code: r.code,
            name: r.name,
            description: r.description,
            spec_versions: vec![], // loaded separately
            status,
            source,
            client_scoped: r.client_scoped,
            application: r.application,
            subdomain: r.subdomain,
            aggregate: r.aggregate,
            event_name,
            client_id: None, // not stored in DB; derived from context
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

/// Row mapping for msg_event_type_spec_versions table
#[derive(sqlx::FromRow)]
struct SpecVersionRow {
    id: String,
    event_type_id: String,
    version: String,
    mime_type: String,
    schema_content: Option<serde_json::Value>,
    schema_type: String,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<SpecVersionRow> for SpecVersion {
    type Error = PlatformError;
    fn try_from(r: SpecVersionRow) -> Result<Self> {
        let schema_type = decode(
            &r.schema_type,
            "msg_event_type_spec_versions",
            "schema_type",
            &r.id,
        )?;
        let status = decode(&r.status, "msg_event_type_spec_versions", "status", &r.id)?;
        Ok(Self {
            id: r.id,
            event_type_id: r.event_type_id,
            version: r.version,
            mime_type: r.mime_type,
            schema_content: r.schema_content,
            schema_type,
            status,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

pub struct EventTypeRepository {
    pool: PgPool,
}

impl EventTypeRepository {
    /// Bootstrap seeding of the platform's own event-type catalogue, as Go's
    /// `seedPlatformEventTypes`: insert a missing code (source `UI`, status
    /// `CURRENT`), refresh the name of an existing one, and attach the
    /// catalogue schema as spec version `1.0` (status `CURRENT`) when the type
    /// has no `1.0` or legacy `v1` version yet. Never deletes, and never
    /// changes a row's source, status or client scope. Returns how many
    /// types it inserted. Startup-only (no principal, no events), like the
    /// built-in role seeding.
    pub async fn seed_catalogue(
        &self,
        defs: &[crate::event_type::operations::SyncEventTypeInput],
    ) -> Result<usize> {
        use crate::event_type::entity::EventType;
        use crate::shared::tsid::{self, EntityType};

        let codes: Vec<String> = defs.iter().map(|d| d.code.clone()).collect();

        // One read for the ids already present and one for the types that
        // already carry a 1.0 schema.
        let existing: Vec<(String, String)> =
            sqlx::query_as("SELECT code, id FROM msg_event_types WHERE code = ANY($1)")
                .bind(&codes)
                .fetch_all(&self.pool)
                .await?;
        let mut ids: std::collections::HashMap<String, String> = existing.into_iter().collect();
        let versioned: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT et.code FROM msg_event_type_spec_versions sv \
             JOIN msg_event_types et ON et.id = sv.event_type_id \
             WHERE et.code = ANY($1) AND sv.version IN ('1.0', 'v1')",
        )
        .bind(&codes)
        .fetch_all(&self.pool)
        .await?;
        let versioned: std::collections::HashSet<String> =
            versioned.into_iter().map(|(c,)| c).collect();

        let now = chrono::Utc::now();
        let mut new_ids = Vec::new();
        let mut new_codes = Vec::new();
        let mut new_names = Vec::new();
        let mut applications = Vec::new();
        let mut subdomains = Vec::new();
        let mut aggregates = Vec::new();
        let mut renamed_ids = Vec::new();
        let mut renamed_names = Vec::new();
        for d in defs {
            match ids.get(&d.code) {
                Some(id) => {
                    renamed_ids.push(id.clone());
                    renamed_names.push(d.name.clone());
                }
                None => {
                    let et = EventType::new(&d.code, &d.name).map_err(|e| {
                        PlatformError::internal(format!("catalogue event type {}: {e}", d.code))
                    })?;
                    new_ids.push(et.id.clone());
                    new_codes.push(et.code.clone());
                    new_names.push(et.name.clone());
                    applications.push(et.application.clone());
                    subdomains.push(et.subdomain.clone());
                    aggregates.push(et.aggregate.clone());
                }
            }
        }

        if !new_codes.is_empty() {
            sqlx::query(
                "INSERT INTO msg_event_types \
                     (id, code, name, description, status, source, client_scoped, \
                      application, subdomain, aggregate, created_at, updated_at) \
                 SELECT id, code, name, NULL, 'CURRENT', 'UI', false, application, subdomain, \
                        aggregate, $7, $7 \
                 FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[]) \
                      AS t(id, code, name, application, subdomain, aggregate) \
                 ON CONFLICT (code) DO NOTHING",
            )
            .bind(&new_ids)
            .bind(&new_codes)
            .bind(&new_names)
            .bind(&applications)
            .bind(&subdomains)
            .bind(&aggregates)
            .bind(now)
            .execute(&self.pool)
            .await?;
            // A concurrent seeder may have won a code: re-read the real ids.
            let inserted: Vec<(String, String)> =
                sqlx::query_as("SELECT code, id FROM msg_event_types WHERE code = ANY($1)")
                    .bind(&new_codes)
                    .fetch_all(&self.pool)
                    .await?;
            ids.extend(inserted);
        }

        if !renamed_ids.is_empty() {
            sqlx::query(
                "UPDATE msg_event_types AS t SET name = v.name, updated_at = $3 \
                 FROM UNNEST($1::text[], $2::text[]) AS v(id, name) WHERE t.id = v.id",
            )
            .bind(&renamed_ids)
            .bind(&renamed_names)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        let mut sv_ids = Vec::new();
        let mut sv_types = Vec::new();
        let mut sv_schemas = Vec::new();
        for d in defs {
            let (Some(schema), Some(id)) = (d.schema.as_ref(), ids.get(&d.code)) else {
                continue;
            };
            if versioned.contains(&d.code) {
                continue;
            }
            sv_ids.push(tsid::generate(EntityType::Schema));
            sv_types.push(id.clone());
            sv_schemas.push(schema.clone());
        }
        if !sv_ids.is_empty() {
            sqlx::query(
                "INSERT INTO msg_event_type_spec_versions \
                     (id, event_type_id, version, mime_type, schema_content, schema_type, \
                      status, created_at, updated_at) \
                 SELECT id, event_type_id, '1.0', 'application/schema+json', schema_content, \
                        'JSON_SCHEMA', 'CURRENT', $4, $4 \
                 FROM UNNEST($1::text[], $2::text[], $3::jsonb[]) \
                      AS v(id, event_type_id, schema_content) \
                 ON CONFLICT (event_type_id, version) DO NOTHING",
            )
            .bind(&sv_ids)
            .bind(&sv_types)
            .bind(&sv_schemas)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
        Ok(new_codes.len())
    }

    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    async fn load_spec_versions(&self, event_type_id: &str) -> Result<Vec<SpecVersion>> {
        let rows = sqlx::query_as::<_, SpecVersionRow>(
            "SELECT id, event_type_id, version, mime_type, schema_content, schema_type, status, created_at, updated_at \
             FROM msg_event_type_spec_versions WHERE event_type_id = $1 ORDER BY version ASC"
        )
        .bind(event_type_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(SpecVersion::try_from).collect()
    }

    async fn hydrate(&self, mut et: EventType) -> Result<EventType> {
        et.spec_versions = self.load_spec_versions(&et.id).await?;
        Ok(et)
    }

    /// Batch-hydrate spec versions for multiple event types (avoids N+1)
    async fn hydrate_all(&self, rows: Vec<EventTypeRow>) -> Result<Vec<EventType>> {
        if rows.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<String> = rows.iter().map(|m| m.id.clone()).collect();
        let all_specs = sqlx::query_as::<_, SpecVersionRow>(
            "SELECT id, event_type_id, version, mime_type, schema_content, schema_type, status, created_at, updated_at \
             FROM msg_event_type_spec_versions WHERE event_type_id = ANY($1) ORDER BY version ASC"
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;

        let mut spec_map: std::collections::HashMap<String, Vec<SpecVersion>> =
            std::collections::HashMap::new();
        for row in all_specs {
            let event_type_id = row.event_type_id.clone();
            spec_map
                .entry(event_type_id)
                .or_default()
                .push(SpecVersion::try_from(row)?);
        }

        rows.into_iter()
            .map(|row| {
                let id = row.id.clone();
                let mut et = EventType::try_from(row)?;
                if let Some(specs) = spec_map.remove(&id) {
                    et.spec_versions = specs;
                }
                Ok(et)
            })
            .collect()
    }

    pub async fn insert(&self, et: &EventType) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_event_types (id, code, name, description, status, source, client_scoped, application, subdomain, aggregate, created_at, updated_at, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"
        )
        .bind(&et.id)
        .bind(&et.code)
        .bind(&et.name)
        .bind(&et.description)
        .bind(et.status.as_str())
        .bind(et.source.as_str())
        .bind(et.client_scoped)
        .bind(&et.application)
        .bind(&et.subdomain)
        .bind(&et.aggregate)
        .bind(now)
        .bind(now)
        .bind(&et.created_by)
        .execute(&self.pool)
        .await?;

        for sv in &et.spec_versions {
            self.insert_spec_version(sv).await?;
        }
        Ok(())
    }

    pub async fn insert_spec_version(&self, sv: &SpecVersion) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_event_type_spec_versions (id, event_type_id, version, mime_type, schema_content, schema_type, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
        )
        .bind(&sv.id)
        .bind(&sv.event_type_id)
        .bind(&sv.version)
        .bind(&sv.mime_type)
        .bind(&sv.schema_content)
        .bind(sv.schema_type.as_str())
        .bind(sv.status.as_str())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<EventType>> {
        let row = sqlx::query_as::<_, EventTypeRow>("SELECT * FROM msg_event_types WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(EventType::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<EventType>> {
        let row =
            sqlx::query_as::<_, EventTypeRow>("SELECT * FROM msg_event_types WHERE code = $1")
                .bind(code)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(EventType::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<EventType>> {
        let rows =
            sqlx::query_as::<_, EventTypeRow>("SELECT * FROM msg_event_types ORDER BY code ASC")
                .fetch_all(&self.pool)
                .await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_by_application(&self, application: &str) -> Result<Vec<EventType>> {
        let rows = sqlx::query_as::<_, EventTypeRow>(
            "SELECT * FROM msg_event_types WHERE application = $1",
        )
        .bind(application)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_by_status(&self, status: EventTypeStatus) -> Result<Vec<EventType>> {
        let rows = sqlx::query_as::<_, EventTypeRow>(
            "SELECT * FROM msg_event_types WHERE status = $1 ORDER BY code ASC",
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    /// Search event types by code or name (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<EventType>> {
        let pattern = format!("%{}%", term);
        let rows = sqlx::query_as::<_, EventTypeRow>(
            "SELECT * FROM msg_event_types WHERE code ILIKE $1 OR name ILIKE $1",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    /// Find event types with optional combined filters (AND logic).
    /// `client_id` filters by `client_scoped = true` since client_id is not stored on
    /// the event_types table; actual client access is checked post-query in the handler.
    pub async fn find_with_filters(
        &self,
        application: Option<&str>,
        client_id: Option<&str>,
        status: Option<EventTypeStatus>,
        subdomain: Option<&str>,
        aggregate: Option<&str>,
    ) -> Result<Vec<EventType>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM msg_event_types");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(app) = application {
            push_where(&mut qb, &mut has_where);
            qb.push("application = ").push_bind(app.to_string());
        }
        if client_id.is_some() {
            push_where(&mut qb, &mut has_where);
            qb.push("client_scoped = true");
        }
        if let Some(s) = status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(s.as_str());
        }
        if let Some(sd) = subdomain {
            push_where(&mut qb, &mut has_where);
            qb.push("subdomain = ").push_bind(sd.to_string());
        }
        if let Some(ag) = aggregate {
            push_where(&mut qb, &mut has_where);
            qb.push("aggregate = ").push_bind(ag.to_string());
        }

        qb.push(" ORDER BY code ASC");
        let rows: Vec<EventTypeRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_active(&self) -> Result<Vec<EventType>> {
        let rows = sqlx::query_as::<_, EventTypeRow>(
            "SELECT * FROM msg_event_types WHERE status = 'CURRENT' ORDER BY code ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    /// Find active event types without loading spec versions (for filter endpoints)
    pub async fn find_active_shallow(&self) -> Result<Vec<EventType>> {
        let rows = sqlx::query_as::<_, EventTypeRow>(
            "SELECT * FROM msg_event_types WHERE status = 'CURRENT' ORDER BY code ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(EventType::try_from).collect()
    }

    /// The status of each event type named by `codes`, shallow (one query,
    /// no spec versions). A code with no row is absent.
    pub async fn statuses_by_codes(
        &self,
        codes: &[String],
    ) -> Result<std::collections::HashMap<String, EventTypeStatus>> {
        if codes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id, code, status FROM msg_event_types WHERE code = ANY($1)")
                .bind(codes)
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|(id, code, status)| {
                let status = decode(&status, "msg_event_types", "status", &id)?;
                Ok((code, status))
            })
            .collect()
    }

    /// The owning application and status of each event type named by
    /// `codes`, shallow (one query): a function's emit checks ownership with
    /// it. A code with no row is absent.
    pub async fn owners_by_codes(
        &self,
        codes: &[String],
    ) -> Result<std::collections::HashMap<String, (String, EventTypeStatus)>> {
        if codes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT id, code, application, status FROM msg_event_types WHERE code = ANY($1)",
        )
        .bind(codes)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(id, code, application, status)| {
                let status = decode(&status, "msg_event_types", "status", &id)?;
                Ok((code, (application, status)))
            })
            .collect()
    }

    pub async fn exists_by_code(&self, code: &str) -> Result<bool> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM msg_event_types WHERE code = $1")
            .bind(code)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 > 0)
    }

    pub async fn update(&self, et: &EventType) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE msg_event_types SET
                code = $2, name = $3, description = $4, status = $5, source = $6,
                client_scoped = $7, application = $8, subdomain = $9, aggregate = $10,
                updated_at = $11
             WHERE id = $1",
        )
        .bind(&et.id)
        .bind(&et.code)
        .bind(&et.name)
        .bind(&et.description)
        .bind(et.status.as_str())
        .bind(et.source.as_str())
        .bind(et.client_scoped)
        .bind(&et.application)
        .bind(&et.subdomain)
        .bind(&et.aggregate)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_spec_version(&self, sv: &SpecVersion) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE msg_event_type_spec_versions SET
                mime_type = $2, schema_content = $3, schema_type = $4, status = $5,
                updated_at = $6
             WHERE id = $1",
        )
        .bind(&sv.id)
        .bind(&sv.mime_type)
        .bind(&sv.schema_content)
        .bind(sv.schema_type.as_str())
        .bind(sv.status.as_str())
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        // Delete spec versions first
        sqlx::query("DELETE FROM msg_event_type_spec_versions WHERE event_type_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        let result = sqlx::query("DELETE FROM msg_event_types WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<EventType> ───────────────────────────────────────────────────────

impl HasId for EventType {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<EventType> for EventTypeRepository {
    async fn persist(&self, et: &EventType, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO msg_event_types (id, code, name, description, status, source, client_scoped, application, subdomain, aggregate, created_at, updated_at, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                status = EXCLUDED.status,
                source = EXCLUDED.source,
                client_scoped = EXCLUDED.client_scoped,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&et.id)
        .bind(&et.code)
        .bind(&et.name)
        .bind(&et.description)
        .bind(et.status.as_str())
        .bind(et.source.as_str())
        .bind(et.client_scoped)
        .bind(&et.application)
        .bind(&et.subdomain)
        .bind(&et.aggregate)
        .bind(now)
        .bind(now)
        .bind(&et.created_by)
        .execute(&mut **tx.inner).await?;

        sqlx::query("DELETE FROM msg_event_type_spec_versions WHERE event_type_id = $1")
            .bind(&et.id)
            .execute(&mut **tx.inner)
            .await?;

        for sv in &et.spec_versions {
            sqlx::query(
                "INSERT INTO msg_event_type_spec_versions (id, event_type_id, version, mime_type, schema_content, schema_type, status, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
            )
            .bind(&sv.id)
            .bind(&sv.event_type_id)
            .bind(&sv.version)
            .bind(&sv.mime_type)
            .bind(&sv.schema_content)
            .bind(sv.schema_type.as_str())
            .bind(sv.status.as_str())
            .bind(sv.created_at)
            .bind(sv.updated_at)
            .execute(&mut **tx.inner).await?;
        }

        Ok(())
    }

    async fn delete(&self, et: &EventType, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM msg_event_type_spec_versions WHERE event_type_id = $1")
            .bind(&et.id)
            .execute(&mut **tx.inner)
            .await?;
        sqlx::query("DELETE FROM msg_event_types WHERE id = $1")
            .bind(&et.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

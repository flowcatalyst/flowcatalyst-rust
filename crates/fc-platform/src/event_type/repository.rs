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
            client_id: None,  // not stored in DB; derived from context
            created_by: None, // not stored in msg_event_types
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
            "INSERT INTO msg_event_types (id, code, name, description, status, source, client_scoped, application, subdomain, aggregate, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
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
            "INSERT INTO msg_event_types (id, code, name, description, status, source, client_scoped, application, subdomain, aggregate, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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

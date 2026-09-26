//! Connection Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{Connection, ConnectionStatus};
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;

/// Row mapping for msg_connections table
#[derive(sqlx::FromRow)]
struct ConnectionRow {
    id: String,
    code: String,
    name: String,
    description: Option<String>,
    external_id: Option<String>,
    status: String,
    service_account_id: String,
    client_id: Option<String>,
    client_identifier: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    application_code: Option<String>,
    source: String,
}

impl TryFrom<ConnectionRow> for Connection {
    type Error = PlatformError;
    fn try_from(r: ConnectionRow) -> Result<Self> {
        let status = decode(&r.status, "msg_connections", "status", &r.id)?;
        // X-06: a source outside the known set is a loud read error (the
        // column's CHECK allows only these).
        if !matches!(r.source.as_str(), "CODE" | "API" | "UI") {
            return Err(PlatformError::internal(format!(
                "connection {} has an unrecognised source",
                r.id
            )));
        }
        Ok(Self {
            id: r.id,
            code: r.code,
            application_code: r.application_code,
            name: r.name,
            description: r.description,
            external_id: r.external_id,
            status,
            service_account_id: r.service_account_id,
            client_id: r.client_id,
            client_identifier: r.client_identifier,
            source: r.source,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

pub struct ConnectionRepository {
    pool: PgPool,
}

impl ConnectionRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, conn: &Connection) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_connections (id, code, name, description, external_id, status, service_account_id, client_id, client_identifier, created_at, updated_at, application_code, source)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"
        )
        .bind(&conn.id)
        .bind(&conn.code)
        .bind(&conn.name)
        .bind(&conn.description)
        .bind(&conn.external_id)
        .bind(conn.status.as_str())
        .bind(&conn.service_account_id)
        .bind(&conn.client_id)
        .bind(&conn.client_identifier)
        .bind(now)
        .bind(now)
        .bind(&conn.application_code)
        .bind(&conn.source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Connection>> {
        let row = sqlx::query_as::<_, ConnectionRow>("SELECT * FROM msg_connections WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(Connection::try_from).transpose()
    }

    /// The connections `ids` name (one query); an id naming none is absent.
    pub async fn find_by_ids(&self, ids: &[String]) -> Result<Vec<Connection>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows =
            sqlx::query_as::<_, ConnectionRow>("SELECT * FROM msg_connections WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter().map(Connection::try_from).collect()
    }

    pub async fn find_by_code_and_client(
        &self,
        code: &str,
        client_id: Option<&str>,
    ) -> Result<Option<Connection>> {
        let row = if let Some(cid) = client_id {
            sqlx::query_as::<_, ConnectionRow>(
                "SELECT * FROM msg_connections WHERE code = $1 AND client_id = $2",
            )
            .bind(code)
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, ConnectionRow>(
                "SELECT * FROM msg_connections WHERE code = $1 AND client_id IS NULL",
            )
            .bind(code)
            .fetch_optional(&self.pool)
            .await?
        };
        row.map(Connection::try_from).transpose()
    }

    pub async fn find_all(&self) -> Result<Vec<Connection>> {
        let rows =
            sqlx::query_as::<_, ConnectionRow>("SELECT * FROM msg_connections ORDER BY code ASC")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter().map(Connection::try_from).collect()
    }

    pub async fn find_with_filters(
        &self,
        client_id: Option<&str>,
        status: Option<ConnectionStatus>,
        service_account_id: Option<&str>,
    ) -> Result<Vec<Connection>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM msg_connections");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(v) = client_id {
            push_where(&mut qb, &mut has_where);
            qb.push("client_id = ").push_bind(v.to_string());
        }
        if let Some(v) = status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(v.as_str());
        }
        if let Some(v) = service_account_id {
            push_where(&mut qb, &mut has_where);
            qb.push("service_account_id = ").push_bind(v.to_string());
        }

        qb.push(" ORDER BY code ASC");
        let rows: Vec<ConnectionRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(Connection::try_from).collect()
    }

    pub async fn find_by_status(&self, status: &str) -> Result<Vec<Connection>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "SELECT * FROM msg_connections WHERE status = $1 ORDER BY code ASC",
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Connection::try_from).collect()
    }

    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Vec<Connection>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "SELECT * FROM msg_connections WHERE client_id = $1 ORDER BY code ASC",
        )
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Connection::try_from).collect()
    }

    pub async fn find_by_service_account(
        &self,
        service_account_id: &str,
    ) -> Result<Vec<Connection>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "SELECT * FROM msg_connections WHERE service_account_id = $1",
        )
        .bind(service_account_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Connection::try_from).collect()
    }

    pub async fn update(&self, conn: &Connection) -> Result<()> {
        sqlx::query(
            "UPDATE msg_connections SET
                code = $2,
                name = $3,
                description = $4,
                external_id = $5,
                status = $6,
                service_account_id = $7,
                client_id = $8,
                client_identifier = $9,
                updated_at = $10
             WHERE id = $1",
        )
        .bind(&conn.id)
        .bind(&conn.code)
        .bind(&conn.name)
        .bind(&conn.description)
        .bind(&conn.external_id)
        .bind(conn.status.as_str())
        .bind(&conn.service_account_id)
        .bind(&conn.client_id)
        .bind(&conn.client_identifier)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM msg_connections WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

impl HasId for Connection {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<Connection> for ConnectionRepository {
    async fn persist(&self, c: &Connection, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_connections (id, code, name, description, external_id, status, service_account_id, client_id, client_identifier, created_at, updated_at, application_code, source)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (id) DO UPDATE SET
                code = EXCLUDED.code,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                external_id = EXCLUDED.external_id,
                status = EXCLUDED.status,
                service_account_id = EXCLUDED.service_account_id,
                client_id = EXCLUDED.client_id,
                client_identifier = EXCLUDED.client_identifier,
                updated_at = EXCLUDED.updated_at,
                application_code = EXCLUDED.application_code,
                source = EXCLUDED.source"
        )
        .bind(&c.id)
        .bind(&c.code)
        .bind(&c.name)
        .bind(&c.description)
        .bind(&c.external_id)
        .bind(c.status.as_str())
        .bind(&c.service_account_id)
        .bind(&c.client_id)
        .bind(&c.client_identifier)
        .bind(now)
        .bind(now)
        .bind(&c.application_code)
        .bind(&c.source)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, c: &Connection, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM msg_connections WHERE id = $1")
            .bind(&c.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

// ── Application-scoped connections (Go 056 / SyncConnections) ────────────

impl ConnectionRepository {
    /// An application's connections in one client scope, with their source
    /// (Go `FindByApplicationAndClient`).
    pub async fn find_by_application_and_client(
        &self,
        application_code: &str,
        client_id: Option<&str>,
    ) -> Result<Vec<(Connection, String)>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "SELECT * FROM msg_connections \
             WHERE application_code = $1 AND client_id IS NOT DISTINCT FROM $2 ORDER BY code",
        )
        .bind(application_code)
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                let c = Connection::try_from(r)?;
                let source = c.source.clone();
                Ok((c, source))
            })
            .collect()
    }

    /// Every connection with one of `codes` that is `application_code`'s or
    /// shared (application-less), any client: what a subscription sync
    /// resolves its `connectionCode`s among, in one query.
    pub async fn find_by_codes_for_application(
        &self,
        codes: &[String],
        application_code: &str,
    ) -> Result<Vec<Connection>> {
        if codes.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "SELECT * FROM msg_connections WHERE code = ANY($1) \
             AND (application_code = $2 OR application_code IS NULL)",
        )
        .bind(codes)
        .bind(application_code)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Connection::try_from).collect()
    }

    /// The connection keyed by (code, application, client), where `None`
    /// is a real value of the key, never a wildcard (Go `FindByCode`: the
    /// `(application_code, client_id, code)` uniqueness of migration 050).
    pub async fn find_by_code_in_scope(
        &self,
        code: &str,
        application_code: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<Option<Connection>> {
        let row = sqlx::query_as::<_, ConnectionRow>(
            "SELECT * FROM msg_connections WHERE code = $1 \
             AND application_code IS NOT DISTINCT FROM $2 \
             AND client_id IS NOT DISTINCT FROM $3",
        )
        .bind(code)
        .bind(application_code)
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(Connection::try_from).transpose()
    }
}

#[async_trait]
impl crate::usecase::Persist<crate::connection::sync_plan::ConnectionSyncPlan>
    for ConnectionRepository
{
    /// One upsert for every saved connection (with its application and
    /// source) and one delete for the removed ones.
    async fn persist(
        &self,
        plan: &crate::connection::sync_plan::ConnectionSyncPlan,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        if !plan.saves.is_empty() {
            let now = Utc::now();
            let mut ids = Vec::new();
            let mut codes = Vec::new();
            let mut names = Vec::new();
            let mut descriptions: Vec<Option<String>> = Vec::new();
            let mut external_ids: Vec<Option<String>> = Vec::new();
            let mut statuses = Vec::new();
            let mut service_accounts = Vec::new();
            let mut client_ids: Vec<Option<String>> = Vec::new();
            let mut identifiers: Vec<Option<String>> = Vec::new();
            let mut created = Vec::new();
            let mut sources = Vec::new();
            for (c, source) in &plan.saves {
                ids.push(c.id.clone());
                codes.push(c.code.clone());
                names.push(c.name.clone());
                descriptions.push(c.description.clone());
                external_ids.push(c.external_id.clone());
                statuses.push(c.status.as_str().to_string());
                service_accounts.push(c.service_account_id.clone());
                client_ids.push(c.client_id.clone());
                identifiers.push(c.client_identifier.clone());
                created.push(c.created_at);
                sources.push(source.clone());
            }
            sqlx::query(
                "INSERT INTO msg_connections (id, code, name, description, external_id, status, \
                     service_account_id, client_id, client_identifier, created_at, updated_at, \
                     application_code, source) \
                 SELECT u.id, u.code, u.name, u.description, u.external_id, u.status, u.sa, \
                        u.client_id, u.identifier, u.created_at, $11, $12, u.source \
                 FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], \
                             $6::text[], $7::text[], $8::text[], $9::text[], $10::timestamptz[], \
                             $13::text[]) \
                   AS u(id, code, name, description, external_id, status, sa, client_id, \
                        identifier, created_at, source) \
                 ON CONFLICT (id) DO UPDATE SET \
                     code = EXCLUDED.code, name = EXCLUDED.name, \
                     description = EXCLUDED.description, external_id = EXCLUDED.external_id, \
                     status = EXCLUDED.status, service_account_id = EXCLUDED.service_account_id, \
                     client_id = EXCLUDED.client_id, client_identifier = EXCLUDED.client_identifier, \
                     updated_at = EXCLUDED.updated_at, application_code = EXCLUDED.application_code, \
                     source = EXCLUDED.source",
            )
            .bind(&ids)
            .bind(&codes)
            .bind(&names)
            .bind(&descriptions)
            .bind(&external_ids)
            .bind(&statuses)
            .bind(&service_accounts)
            .bind(&client_ids)
            .bind(&identifiers)
            .bind(&created)
            .bind(now)
            .bind(&plan.application_code)
            .bind(&sources)
            .execute(&mut **tx.inner)
            .await?;
        }
        if !plan.deletes.is_empty() {
            sqlx::query("DELETE FROM msg_connections WHERE id = ANY($1)")
                .bind(&plan.deletes)
                .execute(&mut **tx.inner)
                .await?;
        }
        Ok(())
    }

    async fn delete(
        &self,
        _plan: &crate::connection::sync_plan::ConnectionSyncPlan,
        _tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        Err(PlatformError::internal("a connection sync is not deleted"))
    }
}

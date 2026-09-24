//! Process Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{Process, ProcessSource, ProcessStatus};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::HasId;

#[derive(sqlx::FromRow)]
struct ProcessRow {
    id: String,
    code: String,
    name: String,
    description: Option<String>,
    status: String,
    source: String,
    application: String,
    subdomain: String,
    process_name: String,
    body: String,
    diagram_type: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ProcessRow> for Process {
    fn from(r: ProcessRow) -> Self {
        Self {
            id: r.id,
            code: r.code,
            name: r.name,
            description: r.description,
            status: ProcessStatus::from_str(&r.status),
            source: ProcessSource::from_str(&r.source),
            application: r.application,
            subdomain: r.subdomain,
            process_name: r.process_name,
            body: r.body,
            diagram_type: r.diagram_type,
            tags: r.tags,
            created_by: None,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub struct ProcessRepository {
    pool: PgPool,
}

impl ProcessRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, p: &Process) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_processes (id, code, name, description, status, source, application, subdomain, process_name, body, diagram_type, tags, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
        )
        .bind(&p.id)
        .bind(&p.code)
        .bind(&p.name)
        .bind(&p.description)
        .bind(p.status.as_str())
        .bind(p.source.as_str())
        .bind(&p.application)
        .bind(&p.subdomain)
        .bind(&p.process_name)
        .bind(&p.body)
        .bind(&p.diagram_type)
        .bind(&p.tags)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update(&self, p: &Process) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE msg_processes SET
                code = $2, name = $3, description = $4, status = $5, source = $6,
                application = $7, subdomain = $8, process_name = $9, body = $10,
                diagram_type = $11, tags = $12, updated_at = $13
             WHERE id = $1",
        )
        .bind(&p.id)
        .bind(&p.code)
        .bind(&p.name)
        .bind(&p.description)
        .bind(p.status.as_str())
        .bind(p.source.as_str())
        .bind(&p.application)
        .bind(&p.subdomain)
        .bind(&p.process_name)
        .bind(&p.body)
        .bind(&p.diagram_type)
        .bind(&p.tags)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM msg_processes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Process>> {
        let row = sqlx::query_as::<_, ProcessRow>("SELECT * FROM msg_processes WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Process::from))
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<Process>> {
        let row = sqlx::query_as::<_, ProcessRow>("SELECT * FROM msg_processes WHERE code = $1")
            .bind(code)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(Process::from))
    }

    pub async fn find_all(&self) -> Result<Vec<Process>> {
        let rows = sqlx::query_as::<_, ProcessRow>("SELECT * FROM msg_processes ORDER BY code ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(Process::from).collect())
    }

    pub async fn find_by_application(&self, application: &str) -> Result<Vec<Process>> {
        let rows = sqlx::query_as::<_, ProcessRow>(
            "SELECT * FROM msg_processes WHERE application = $1 ORDER BY code ASC",
        )
        .bind(application)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Process::from).collect())
    }

    pub async fn find_with_filters(
        &self,
        application: Option<&str>,
        subdomain: Option<&str>,
        status: Option<ProcessStatus>,
        search: Option<&str>,
    ) -> Result<Vec<Process>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM msg_processes");
        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(app) = application {
            push_where(&mut qb, &mut has_where);
            qb.push("application = ").push_bind(app.to_string());
        }
        if let Some(sub) = subdomain {
            push_where(&mut qb, &mut has_where);
            qb.push("subdomain = ").push_bind(sub.to_string());
        }
        if let Some(s) = status {
            push_where(&mut qb, &mut has_where);
            qb.push("status = ").push_bind(s.as_str());
        }
        if let Some(term) = search {
            push_where(&mut qb, &mut has_where);
            let pattern = format!("%{}%", term);
            qb.push("(code ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR name ILIKE ")
                .push_bind(pattern)
                .push(")");
        }

        qb.push(" ORDER BY code ASC");
        let rows: Vec<ProcessRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(Process::from).collect())
    }

    pub async fn exists_by_code(&self, code: &str) -> Result<bool> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM msg_processes WHERE code = $1")
            .bind(code)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 > 0)
    }
}

impl HasId for Process {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<Process> for ProcessRepository {
    async fn persist(&self, p: &Process, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_processes (id, code, name, description, status, source, application, subdomain, process_name, body, diagram_type, tags, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET
                code = EXCLUDED.code,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                status = EXCLUDED.status,
                source = EXCLUDED.source,
                application = EXCLUDED.application,
                subdomain = EXCLUDED.subdomain,
                process_name = EXCLUDED.process_name,
                body = EXCLUDED.body,
                diagram_type = EXCLUDED.diagram_type,
                tags = EXCLUDED.tags,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&p.id)
        .bind(&p.code)
        .bind(&p.name)
        .bind(&p.description)
        .bind(p.status.as_str())
        .bind(p.source.as_str())
        .bind(&p.application)
        .bind(&p.subdomain)
        .bind(&p.process_name)
        .bind(&p.body)
        .bind(&p.diagram_type)
        .bind(&p.tags)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, p: &Process, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM msg_processes WHERE id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

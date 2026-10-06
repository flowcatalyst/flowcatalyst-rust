//! `app_docs` repository (Go `appdocs/appdocs.go`).

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::ApplicationId;
use sqlx::PgPool;

use super::entity::{AppDoc, AppDocsReplacement};
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;

struct AppDocRow {
    id: String,
    application_id: ApplicationId,
    slug: String,
    title: String,
    content: String,
    position: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AppDocRow> for AppDoc {
    fn from(r: AppDocRow) -> Self {
        Self {
            id: r.id,
            application_id: r.application_id,
            slug: r.slug,
            title: r.title,
            content: r.content,
            position: r.position,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// A page's listing entry.
#[derive(Debug, Clone)]
pub struct AppDocSummary {
    pub application_id: ApplicationId,
    pub slug: String,
    pub title: String,
}

pub struct AppDocsRepository {
    pool: PgPool,
}

impl AppDocsRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// An application's page slugs.
    pub async fn slugs_for_application(
        &self,
        application_id: &ApplicationId,
    ) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query!(
            "SELECT slug FROM app_docs WHERE application_id = $1",
            application_id as &ApplicationId
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| (r.slug,))
        .collect();
        Ok(rows.into_iter().map(|(s,)| s).collect())
    }

    /// Every page's listing entry, by application, position and slug (Go
    /// `DistinctApplicationIDs` + `ListForApplication`, in one query).
    pub async fn summaries(&self) -> Result<Vec<AppDocSummary>> {
        Ok(sqlx::query_as!(
            AppDocSummary,
            "SELECT application_id AS \"application_id: ApplicationId\", slug, title \
                    FROM app_docs \
             ORDER BY application_id, position, slug"
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// One page (Go `Get`).
    pub async fn find(&self, application_id: &ApplicationId, slug: &str) -> Result<Option<AppDoc>> {
        let row = sqlx::query_as!(
            AppDocRow,
            "SELECT id, application_id AS \"application_id: ApplicationId\", slug, title, \
                    content, position, created_at, updated_at \
                    \
             FROM app_docs WHERE application_id = $1 AND slug = $2",
            application_id as &ApplicationId,
            slug
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }
}

impl Persist<AppDocsReplacement> for AppDocsRepository {
    /// Go `ReplaceForApplication`: every page upserted by (application,
    /// slug) with its position, then the unlisted ones deleted.
    async fn persist(&self, r: &AppDocsReplacement, tx: &mut DbTx<'_>) -> Result<()> {
        if !r.docs.is_empty() {
            let ids: Vec<&str> = r.docs.iter().map(|d| d.id.as_str()).collect();
            let slugs: Vec<&str> = r.docs.iter().map(|d| d.slug.as_str()).collect();
            let titles: Vec<&str> = r.docs.iter().map(|d| d.title.as_str()).collect();
            let contents: Vec<&str> = r.docs.iter().map(|d| d.content.as_str()).collect();
            let positions: Vec<i32> = r.docs.iter().map(|d| d.position).collect();
            let now = r.docs[0].updated_at;
            sqlx::query!(
            "INSERT INTO app_docs (id, application_id, slug, title, content, position, created_at, updated_at) \
                 SELECT u.id, $1, u.slug, u.title, u.content, u.position, $7, $7 \
                 FROM UNNEST($2::text[], $3::text[], $4::text[], $5::text[], $6::int[]) \
                   AS u(id, slug, title, content, position) \
                 ON CONFLICT (application_id, slug) DO UPDATE SET \
                     title = EXCLUDED.title, content = EXCLUDED.content, \
                     position = EXCLUDED.position, updated_at = EXCLUDED.updated_at",
            &r.application_id as &ApplicationId,
            &ids as &[&str],
            &slugs as &[&str],
            &titles as &[&str],
            &contents as &[&str],
            &positions,
            now
        )
            .execute(&mut **tx.inner)
            .await?;
        }
        if !r.removed_slugs.is_empty() {
            sqlx::query!(
                "DELETE FROM app_docs WHERE application_id = $1 AND slug = ANY($2)",
                &r.application_id as &ApplicationId,
                &r.removed_slugs
            )
            .execute(&mut **tx.inner)
            .await?;
        }
        Ok(())
    }

    async fn delete(&self, _r: &AppDocsReplacement, _tx: &mut DbTx<'_>) -> Result<()> {
        Err(PlatformError::internal(
            "a documentation replacement is not deleted",
        ))
    }
}

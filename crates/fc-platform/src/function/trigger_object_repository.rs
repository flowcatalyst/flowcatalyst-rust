//! `fn_trigger_objects` (Java `function/TriggerObjectRepository.java`): the
//! pool, subscriptions and scheduled jobs a function's live manifest
//! created at promote.
//!
//! A link is written only together with the object it names, in the same
//! transaction, by [`LinkedRepository`]: the object's own repository
//! persists (or deletes) the object, then this one links (or first
//! unlinks) it. The commit carries the object's own event and audit row,
//! so a function's subscription is a real subscription with a real audit
//! trail and the link is bookkeeping beside it, as in Java.

use std::collections::HashSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{TriggerObject, TriggerObjectKind, TriggerObjectLink};
use crate::shared::enum_str::decode;
use crate::shared::error::Result;
use crate::usecase::{DbTx, HasId, Persist};

pub struct TriggerObjectRepository {
    pool: PgPool,
}

impl TriggerObjectRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// A function's links, by kind then trigger key (Java `listByFunction`).
    pub async fn list(&self, function_id: &str) -> Result<Vec<TriggerObject>> {
        let rows: Vec<(String, String, String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT kind, object_id, trigger_key, created_at FROM fn_trigger_objects \
             WHERE function_id = $1 ORDER BY kind ASC, trigger_key ASC",
        )
        .bind(function_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(kind, object_id, trigger_key, created_at)| {
                let kind: TriggerObjectKind =
                    decode(&kind, "fn_trigger_objects", "kind", &object_id)?;
                Ok(TriggerObject {
                    function_id: function_id.to_string(),
                    kind,
                    object_id,
                    trigger_key,
                    created_at,
                })
            })
            .collect()
    }

    /// Every linked object id of one kind, across all functions (Java
    /// `objectIds`): what an SDK sync must leave alone.
    pub async fn object_ids(&self, kind: TriggerObjectKind) -> Result<HashSet<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT object_id FROM fn_trigger_objects WHERE kind = $1")
                .bind(kind.as_str())
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// A function's links, by kind then trigger key, each with whether the
    /// object it names still exists in its own table (Java's status route
    /// asks each object's repository one by one; this asks once). `false`
    /// means it was deleted by hand; the next promote recreates it.
    pub async fn list_by_function(&self, function_id: &str) -> Result<Vec<TriggerObjectLink>> {
        let rows: Vec<(String, String, String, bool)> = sqlx::query_as(
            "SELECT t.kind, t.trigger_key, t.object_id, \
                CASE t.kind \
                    WHEN 'POOL' THEN EXISTS \
                        (SELECT 1 FROM msg_dispatch_pools p WHERE p.id = t.object_id) \
                    WHEN 'SUBSCRIPTION' THEN EXISTS \
                        (SELECT 1 FROM msg_subscriptions s WHERE s.id = t.object_id) \
                    WHEN 'SCHEDULED_JOB' THEN EXISTS \
                        (SELECT 1 FROM msg_scheduled_jobs j WHERE j.id = t.object_id) \
                    ELSE FALSE \
                END \
             FROM fn_trigger_objects t WHERE t.function_id = $1 \
             ORDER BY t.kind ASC, t.trigger_key ASC",
        )
        .bind(function_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(kind, trigger_key, object_id, present)| {
                let kind: TriggerObjectKind =
                    decode(&kind, "fn_trigger_objects", "kind", &object_id)?;
                Ok(TriggerObjectLink {
                    kind,
                    trigger_key,
                    object_id,
                    present,
                })
            })
            .collect()
    }

    /// Upsert on `(function, kind, trigger key)`: a recreated object moves
    /// the link to its new id (Java `link`).
    async fn link(&self, link: &TriggerObject, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO fn_trigger_objects (function_id, kind, object_id, trigger_key, created_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (function_id, kind, trigger_key) DO UPDATE SET object_id = EXCLUDED.object_id",
        )
        .bind(&link.function_id)
        .bind(link.kind.as_str())
        .bind(&link.object_id)
        .bind(&link.trigger_key)
        .bind(link.created_at)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn unlink(&self, link: &TriggerObject, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query(
            "DELETE FROM fn_trigger_objects \
             WHERE function_id = $1 AND kind = $2 AND trigger_key = $3",
        )
        .bind(&link.function_id)
        .bind(link.kind.as_str())
        .bind(&link.trigger_key)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

/// An object a function owns, with its link.
pub struct Linked<A> {
    pub object: A,
    pub link: TriggerObject,
}

impl<A: HasId> HasId for Linked<A> {
    fn id(&self) -> &str {
        self.object.id()
    }
}

/// Persists a [`Linked`] object: the object through its own repository
/// (its one write path), then the link; deletes unlink first, then delete
/// the object through its own repository.
pub struct LinkedRepository<'r, R> {
    pub objects: &'r R,
    pub links: &'r TriggerObjectRepository,
}

#[async_trait]
impl<A, R> Persist<Linked<A>> for LinkedRepository<'_, R>
where
    A: HasId + Send + Sync,
    R: Persist<A>,
{
    async fn persist(&self, linked: &Linked<A>, tx: &mut DbTx<'_>) -> Result<()> {
        self.objects.persist(&linked.object, tx).await?;
        self.links.link(&linked.link, tx).await
    }

    async fn delete(&self, linked: &Linked<A>, tx: &mut DbTx<'_>) -> Result<()> {
        self.links.unlink(&linked.link, tx).await?;
        self.objects.delete(&linked.object, tx).await
    }
}

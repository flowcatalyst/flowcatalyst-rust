//! `fnr_trigger_objects` (Java `function/TriggerObjectRepository.java`): the
//! pool, subscriptions and scheduled jobs a function's live manifest
//! created at promote.
//!
//! A link is written only together with the object it names, in the same
//! transaction, by [`LinkedRepository`]: the object's own repository
//! persists (or deletes) the object, then this one links (or first
//! unlinks) it. The commit carries the object's own event and audit row,
//! so a function's subscription is a real subscription with a real audit
//! trail and the link is bookkeeping beside it, as in Java.

use fc_platform_core::shared::id::FunctionId;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{TriggerObject, TriggerObjectKind, TriggerObjectLink};
use fc_platform_core::shared::enum_str::Stored;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::{DbTx, HasId, Persist};

pub struct TriggerObjectRepository {
    pool: PgPool,
}

impl TriggerObjectRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// A function's links, by kind then trigger key (Java `listByFunction`).
    pub async fn list(&self, function_id: &FunctionId) -> Result<Vec<TriggerObject>> {
        let rows: Vec<(Stored<TriggerObjectKind>, String, String, DateTime<Utc>)> = sqlx::query!(
            "SELECT kind AS \"kind: Stored<TriggerObjectKind>\", object_id, trigger_key, created_at FROM fnr_trigger_objects \
             WHERE function_id = $1 ORDER BY kind ASC, trigger_key ASC",
            function_id as &FunctionId
        )
        .fetch_all(&self.pool)
        .await?.into_iter().map(|r| (r.kind, r.object_id, r.trigger_key, r.created_at)).collect();
        rows.into_iter()
            .map(|(kind, object_id, trigger_key, created_at)| {
                let kind = kind.decode("fnr_trigger_objects", "kind", &object_id)?;
                Ok(TriggerObject {
                    function_id: function_id.clone(),
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
        let rows: Vec<(String,)> = sqlx::query!(
            "SELECT object_id FROM fnr_trigger_objects WHERE kind = $1",
            kind as TriggerObjectKind
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| (r.object_id,))
        .collect();
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// A function's links, by kind then trigger key, each with whether the
    /// object it names still exists in its own table (Java's status route
    /// asks each object's repository one by one; this asks once). `false`
    /// means it was deleted by hand; the next promote recreates it.
    pub async fn list_by_function(
        &self,
        function_id: &FunctionId,
    ) -> Result<Vec<TriggerObjectLink>> {
        // `present!`: a CASE whose every branch is a boolean (EXISTS or FALSE).
        let rows: Vec<(Stored<TriggerObjectKind>, String, String, bool)> = sqlx::query!(
            "SELECT t.kind AS \"kind: Stored<TriggerObjectKind>\", t.trigger_key, t.object_id, \
                CASE t.kind \
                    WHEN $2 THEN EXISTS \
                        (SELECT 1 FROM msg_dispatch_pools p WHERE p.id = t.object_id) \
                    WHEN $3 THEN EXISTS \
                        (SELECT 1 FROM msg_subscriptions s WHERE s.id = t.object_id) \
                    WHEN $4 THEN EXISTS \
                        (SELECT 1 FROM msg_scheduled_jobs j WHERE j.id = t.object_id) \
                    ELSE FALSE \
                END AS \"present!\" \
             FROM fnr_trigger_objects t WHERE t.function_id = $1 \
             ORDER BY t.kind ASC, t.trigger_key ASC",
            function_id as &FunctionId,
            TriggerObjectKind::Pool as TriggerObjectKind,
            TriggerObjectKind::Subscription as TriggerObjectKind,
            TriggerObjectKind::ScheduledJob as TriggerObjectKind
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| (r.kind, r.trigger_key, r.object_id, r.present))
        .collect();
        rows.into_iter()
            .map(|(kind, trigger_key, object_id, present)| {
                let kind = kind.decode("fnr_trigger_objects", "kind", &object_id)?;
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
        sqlx::query!(
            "INSERT INTO fnr_trigger_objects (function_id, kind, object_id, trigger_key, created_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (function_id, kind, trigger_key) DO UPDATE SET object_id = EXCLUDED.object_id",
            &link.function_id as &FunctionId,
            link.kind as TriggerObjectKind,
            &link.object_id,
            &link.trigger_key,
            link.created_at
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn unlink(&self, link: &TriggerObject, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM fnr_trigger_objects \
             WHERE function_id = $1 AND kind = $2 AND trigger_key = $3",
            &link.function_id as &FunctionId,
            link.kind as TriggerObjectKind,
            &link.trigger_key
        )
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

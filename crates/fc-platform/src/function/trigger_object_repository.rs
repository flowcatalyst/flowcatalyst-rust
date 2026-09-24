//! `fn_trigger_objects`, read side (Java `function/TriggerObjectRepository.java`).
//! Promote writes these links in P5; the status route reads them now.

use sqlx::PgPool;

use super::entity::{TriggerObjectKind, TriggerObjectLink};
use crate::shared::enum_str::decode;
use crate::shared::error::Result;

pub struct TriggerObjectRepository {
    pool: PgPool,
}

impl TriggerObjectRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
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
}

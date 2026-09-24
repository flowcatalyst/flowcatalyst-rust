//! Unit of Work — PostgreSQL via SQLx
//!
//! Atomic commit of entity state changes, domain events, and audit logs
//! within a single PostgreSQL transaction.
//!
//! Two impls:
//!
//! - `PgUnitOfWork` — each `commit(…)` opens and closes its own tx. This is
//!   the common case: one HTTP request, one use case, one tx.
//!
//! - `TxScopedUnitOfWork` — shares an existing tx across multiple use cases
//!   so a handler can orchestrate two-aggregate operations atomically.
//!   Produced by `PgUnitOfWork::run(closure)`; the handler owns the tx
//!   boundary. Use cases see only the `UnitOfWork` trait either way.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use tokio::sync::Mutex;
use tracing::{debug, error};

use super::domain_event::DomainEvent;
use super::error::UseCaseError;
use super::result::UseCaseResult;

// ─── Traits ──────────────────────────────────────────────────────────────────

/// Trait for entities that have a unique string ID.
pub trait HasId {
    fn id(&self) -> &str;
}

// ─── Repository-owned persistence ────────────────────────────────────────────
//
// Aggregates don't persist themselves. A repository persists an aggregate.
// The transaction handle is wrapped in `DbTx` so repositories don't mention
// the concrete driver type — only this file does. A future backend swap
// would only touch `DbTx` plus `PgUnitOfWork` internals, not the ~15
// `impl Persist<X> for XRepository` blocks.
//
// See CLAUDE.md § "Layering Rules" for the full rule set.

/// Opaque write handle passed to `Persist` methods. Wraps the underlying
/// driver transaction; repositories access the inner handle via
/// `&mut *tx.inner` which keeps the leak contained to this crate.
pub struct DbTx<'t> {
    pub(crate) inner: &'t mut Transaction<'static, Postgres>,
}

/// A repository that can persist and delete aggregates of type `A` within
/// a transaction.
///
/// Implement this on the repository type (`impl Persist<Principal> for
/// PrincipalRepository`), **not** on the aggregate. The aggregate is the
/// thing being written; the repository is what writes it.
#[async_trait]
pub trait Persist<A: HasId + Send + Sync>: Send + Sync {
    /// Upsert the aggregate's rows within the given transaction.
    async fn persist(&self, aggregate: &A, tx: &mut DbTx<'_>) -> crate::shared::error::Result<()>;

    /// Delete the aggregate's rows within the given transaction.
    async fn delete(&self, aggregate: &A, tx: &mut DbTx<'_>) -> crate::shared::error::Result<()>;
}

// ─── UnitOfWork trait ────────────────────────────────────────────────────────

/// Unit of Work for atomic control plane operations.
///
/// Ensures entity state changes, domain events, and audit logs are committed
/// atomically in a single PostgreSQL transaction.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Commit an aggregate upsert via its repository, plus the domain event
    /// and audit log — all in a single transaction.
    async fn commit<A, R, E, C>(
        &self,
        aggregate: &A,
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync;

    /// Commit an aggregate delete via its repository, plus the domain event
    /// and audit log — all in a single transaction.
    async fn commit_delete<A, R, E, C>(
        &self,
        aggregate: &A,
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync;

    /// Emit a domain event and audit log without an entity change.
    ///
    /// Used for events that don't modify an entity directly (e.g., `UserLoggedIn`).
    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync;

    /// Commit a batch of aggregate upserts of the same type via one repository,
    /// plus a single domain event and audit log — all in one transaction.
    ///
    /// Use when one logical operation touches many rows of the same aggregate
    /// (e.g., toggling client→application enablement). Emits exactly one event
    /// summarising the change rather than one event per row.
    async fn commit_all<A, R, E, C>(
        &self,
        aggregates: &[A],
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync;
}

// ─── PgUnitOfWork ────────────────────────────────────────────────────────────

/// PostgreSQL implementation of `UnitOfWork` using SQLx transactions.
#[derive(Clone)]
pub struct PgUnitOfWork {
    pool: PgPool,
}

impl PgUnitOfWork {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn from_ref(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    // ── Subject parsing helpers ───────────────────────────────

    /// "platform.eventtype.123" -> "Eventtype"
    fn extract_aggregate_type(subject: &str) -> String {
        subject
            .split('.')
            .nth(1)
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// "platform.eventtype.123" -> Some("123")
    fn extract_entity_id(subject: &str) -> String {
        subject.split('.').nth(2).unwrap_or("").to_string()
    }

    // ── Persist helpers ──────────────────────────────────────

    async fn persist_event<E: DomainEvent>(
        txn: &mut Transaction<'_, Postgres>,
        event: &E,
    ) -> Result<(), UseCaseError> {
        let row = EventRow::from_event(event)?;
        let now = Utc::now();

        let result = sqlx::query(
            r#"INSERT INTO msg_events
                (id, spec_version, type, source, subject,
                 time, data, correlation_id, causation_id,
                 deduplication_id, message_group, client_id,
                 context_data, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"#,
        )
        .bind(row.id)
        .bind(row.spec_version)
        .bind(row.event_type)
        .bind(row.source)
        .bind(row.subject)
        .bind(row.time)
        .bind(&row.data)
        .bind(row.correlation_id)
        .bind(row.causation_id)
        .bind(&row.deduplication_id)
        .bind(row.message_group)
        .bind(None::<String>) // client_id
        .bind(&row.context_data)
        .bind(now)
        .execute(&mut **txn)
        .await;

        if let Err(e) = result {
            error!("Failed to insert domain event: {}", e);
            return Err(UseCaseError::commit(format!(
                "Failed to insert domain event: {}",
                e
            )));
        }

        Ok(())
    }

    async fn persist_audit_log<E: DomainEvent, C: Serialize>(
        txn: &mut Transaction<'_, Postgres>,
        event: &E,
        command: &C,
    ) -> Result<(), UseCaseError> {
        let row = AuditRow::from_event(event, command);

        let result = sqlx::query(
            r#"INSERT INTO aud_logs
                (id, entity_type, entity_id, operation,
                 operation_json, principal_id, application_id,
                 client_id, performed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(crate::shared::tsid::generate_untyped())
        .bind(&row.entity_type)
        .bind(&row.entity_id)
        .bind(&row.operation)
        .bind(&row.operation_json)
        .bind(row.principal_id)
        .bind(None::<String>) // application_id
        .bind(None::<String>) // client_id
        .bind(row.performed_at)
        .execute(&mut **txn)
        .await;

        if let Err(e) = result {
            error!("Failed to insert audit log: {}", e);
            return Err(UseCaseError::commit(format!(
                "Failed to insert audit log: {}",
                e
            )));
        }

        Ok(())
    }

    async fn persist_event_and_audit<E: DomainEvent, C: Serialize>(
        txn: &mut Transaction<'_, Postgres>,
        event: &E,
        command: &C,
    ) -> Result<(), UseCaseError> {
        Self::persist_event(&mut *txn, event).await?;
        Self::persist_audit_log(&mut *txn, event, command).await?;
        Ok(())
    }
}

// ─── Row projections ─────────────────────────────────────────────────────────
//
// The event- and command-derived column values a commit writes. Kept separate
// from the INSERTs so the exact persisted shape can be pinned by tests.

/// Column values of the `msg_events` row written for a domain event.
///
/// `client_id` (always NULL) and `created_at` (insert time) are not derived
/// from the event and are bound by the INSERT itself.
#[derive(Debug, Serialize)]
pub(crate) struct EventRow<'a> {
    pub id: &'a str,
    pub spec_version: &'a str,
    pub event_type: &'a str,
    pub source: &'a str,
    pub subject: &'a str,
    pub time: chrono::DateTime<Utc>,
    pub data: serde_json::Value,
    pub correlation_id: &'a str,
    pub causation_id: Option<&'a str>,
    pub deduplication_id: String,
    pub message_group: &'a str,
    pub context_data: serde_json::Value,
}

impl<'a> EventRow<'a> {
    /// The event's `Serialize` output is the `data` payload; a serialization
    /// failure fails the commit rather than persisting a placeholder.
    pub(crate) fn from_event<E: DomainEvent>(event: &'a E) -> Result<Self, UseCaseError> {
        let data = serde_json::to_value(event).map_err(|e| {
            error!("Failed to serialize domain event: {}", e);
            UseCaseError::commit(format!("Failed to serialize domain event: {}", e))
        })?;
        let meta = event.metadata();

        let context_data = serde_json::json!([
            {"key": "principalId", "value": meta.principal_id},
            {"key": "aggregateType", "value": PgUnitOfWork::extract_aggregate_type(&meta.subject)},
        ]);

        Ok(Self {
            id: &meta.event_id,
            spec_version: &meta.spec_version,
            event_type: &meta.event_type,
            source: &meta.source,
            subject: &meta.subject,
            time: meta.time,
            data,
            correlation_id: &meta.correlation_id,
            causation_id: meta.causation_id.as_deref(),
            deduplication_id: format!("{}-{}", meta.event_type, meta.event_id),
            message_group: &meta.message_group,
            context_data,
        })
    }
}

/// Column values of the `aud_logs` row written for a command.
///
/// `id` (fresh TSID), `application_id` and `client_id` (always NULL) are bound
/// by the INSERT itself.
#[derive(Debug, Serialize)]
pub(crate) struct AuditRow<'a> {
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub operation_json: Option<serde_json::Value>,
    pub principal_id: &'a str,
    pub performed_at: chrono::DateTime<Utc>,
}

impl<'a> AuditRow<'a> {
    pub(crate) fn from_event<E: DomainEvent, C: Serialize>(event: &'a E, command: &C) -> Self {
        let operation = std::any::type_name::<C>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown")
            .to_string();

        let meta = event.metadata();
        Self {
            entity_type: PgUnitOfWork::extract_aggregate_type(&meta.subject),
            entity_id: PgUnitOfWork::extract_entity_id(&meta.subject),
            operation,
            operation_json: serde_json::to_value(command).ok(),
            principal_id: &meta.principal_id,
            performed_at: meta.time,
        }
    }
}

#[async_trait]
impl UnitOfWork for PgUnitOfWork {
    async fn commit<A, R, E, C>(
        &self,
        aggregate: &A,
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        // Scope the DbTx so its &mut borrow of txn is released before we reuse txn.
        let persist_result = {
            let mut tx = DbTx { inner: &mut txn };
            repository.persist(aggregate, &mut tx).await
        };
        if let Err(e) = persist_result {
            let _ = txn.rollback().await;
            error!("Failed to persist aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to persist aggregate: {}",
                e
            )));
        }

        if let Err(e) = Self::persist_event_and_audit(&mut txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.metadata().event_id.as_str(),
            event_type = event.metadata().event_type.as_str(),
            "Successfully committed transaction"
        );

        UseCaseResult::success(event)
    }

    async fn commit_delete<A, R, E, C>(
        &self,
        aggregate: &A,
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        let delete_result = {
            let mut tx = DbTx { inner: &mut txn };
            repository.delete(aggregate, &mut tx).await
        };
        if let Err(e) = delete_result {
            let _ = txn.rollback().await;
            error!("Failed to delete aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to delete aggregate: {}",
                e
            )));
        }

        if let Err(e) = Self::persist_event_and_audit(&mut txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.metadata().event_id.as_str(),
            event_type = event.metadata().event_type.as_str(),
            "Successfully committed delete transaction"
        );

        UseCaseResult::success(event)
    }

    async fn commit_all<A, R, E, C>(
        &self,
        aggregates: &[A],
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        for aggregate in aggregates {
            let persist_result = {
                let mut tx = DbTx { inner: &mut txn };
                repository.persist(aggregate, &mut tx).await
            };
            if let Err(e) = persist_result {
                let _ = txn.rollback().await;
                error!("Failed to persist aggregate in batch: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to persist aggregate: {}",
                    e
                )));
            }
        }

        if let Err(e) = Self::persist_event_and_audit(&mut txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.metadata().event_id.as_str(),
            event_type = event.metadata().event_type.as_str(),
            count = aggregates.len(),
            "Successfully committed batch transaction"
        );

        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        if let Err(e) = Self::persist_event_and_audit(&mut txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.metadata().event_id.as_str(),
            event_type = event.metadata().event_type.as_str(),
            "Successfully emitted domain event"
        );

        UseCaseResult::success(event)
    }
}

// ─── Shared-tx orchestration ─────────────────────────────────────────────────
//
// For operations that mutate multiple aggregates atomically, the handler
// calls `pg_uow.run(|session| async move { ... })`. Inside the closure the
// `session` implements `UnitOfWork`, so existing use cases can be constructed
// against it and called one after another; they all write into the same tx.
// The closure's outer `UseCaseResult` decides commit vs rollback.
//
// The handler owns the tx boundary. Use cases remain tx-unaware — they see
// only the trait.

/// UnitOfWork implementation that writes into an already-open transaction
/// owned by `PgUnitOfWork::run`. `commit()` / `commit_delete()` / `emit_event()`
/// write their rows but do NOT close the tx — the outer `run` does.
pub struct TxScopedUnitOfWork {
    // tokio Mutex because the guard is held across `.await`. `Option` so
    // `run` can `.take()` the tx back out after the closure completes.
    tx: Mutex<Option<Transaction<'static, Postgres>>>,
}

impl TxScopedUnitOfWork {
    fn new(tx: Transaction<'static, Postgres>) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
        }
    }

    async fn take_tx(&self) -> Option<Transaction<'static, Postgres>> {
        self.tx.lock().await.take()
    }
}

#[async_trait]
impl UnitOfWork for TxScopedUnitOfWork {
    async fn commit<A, R, E, C>(
        &self,
        aggregate: &A,
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedUnitOfWork: transaction already finalized",
                ))
            }
        };

        let persist_result = {
            let mut tx = DbTx { inner: txn };
            repository.persist(aggregate, &mut tx).await
        };
        if let Err(e) = persist_result {
            error!("Failed to persist aggregate in scoped tx: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to persist aggregate: {}",
                e
            )));
        }

        if let Err(e) = PgUnitOfWork::persist_event_and_audit(txn, &event, command).await {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }

    async fn commit_delete<A, R, E, C>(
        &self,
        aggregate: &A,
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedUnitOfWork: transaction already finalized",
                ))
            }
        };

        let delete_result = {
            let mut tx = DbTx { inner: txn };
            repository.delete(aggregate, &mut tx).await
        };
        if let Err(e) = delete_result {
            error!("Failed to delete aggregate in scoped tx: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to delete aggregate: {}",
                e
            )));
        }

        if let Err(e) = PgUnitOfWork::persist_event_and_audit(txn, &event, command).await {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedUnitOfWork: transaction already finalized",
                ))
            }
        };

        if let Err(e) = PgUnitOfWork::persist_event_and_audit(txn, &event, command).await {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }

    async fn commit_all<A, R, E, C>(
        &self,
        aggregates: &[A],
        repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedUnitOfWork: transaction already finalized",
                ))
            }
        };

        for aggregate in aggregates {
            let persist_result = {
                let mut tx = DbTx { inner: txn };
                repository.persist(aggregate, &mut tx).await
            };
            if let Err(e) = persist_result {
                error!("Failed to persist aggregate in scoped batch: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to persist aggregate: {}",
                    e
                )));
            }
        }

        if let Err(e) = PgUnitOfWork::persist_event_and_audit(txn, &event, command).await {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }
}

impl PgUnitOfWork {
    /// Run a closure inside a single transaction.
    ///
    /// The closure receives an `Arc<TxScopedUnitOfWork>` that implements
    /// `UnitOfWork`. Use cases constructed against it all share the tx.
    /// The closure's outer `UseCaseResult` drives commit vs rollback.
    ///
    /// Handlers use this for cross-aggregate orchestration:
    ///
    /// ```ignore
    /// state.unit_of_work.run(|session| async move {
    ///     let sa_uc = CreateServiceAccountUseCase::new(sa_repo, client_repo, session.clone(), encryption);
    ///     let attach_uc = AttachServiceAccountToApplicationUseCase::new(app_repo, session.clone());
    ///
    ///     sa_uc.run(create_cmd, ctx.clone()).await.into_result()?;
    ///     attach_uc.run(attach_cmd, ctx).await.into_result()?;
    ///     UseCaseResult::success(())
    /// }).await
    /// ```
    ///
    /// The tx boundary lives in the handler; use cases stay tx-agnostic.
    pub async fn run<F, Fut, T>(&self, f: F) -> UseCaseResult<T>
    where
        F: FnOnce(Arc<TxScopedUnitOfWork>) -> Fut + Send,
        Fut: Future<Output = UseCaseResult<T>> + Send,
        T: Send + 'static,
    {
        let tx = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start orchestration transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        let scoped = Arc::new(TxScopedUnitOfWork::new(tx));
        let result = f(Arc::clone(&scoped)).await;

        // Reclaim the tx. If the scoped UoW has outstanding references
        // (e.g. a use case leaked it into a spawned task) we can't reclaim
        // — drop without explicit commit, which rolls back.
        let tx_opt = scoped.take_tx().await;

        if let Some(tx) = tx_opt {
            match result.as_result() {
                Ok(_) => {
                    if let Err(e) = tx.commit().await {
                        error!("Failed to commit orchestration tx: {}", e);
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to commit transaction: {}",
                            e
                        )));
                    }
                    debug!("Orchestration tx committed");
                }
                Err(err) => {
                    let _ = tx.rollback().await;
                    debug!(error = %err.code(), "Orchestration tx rolled back");
                }
            }
        }

        result
    }
}

// ─── InMemory (tests) ─────────────────────────────────────────────────────────

#[cfg(test)]
pub struct InMemoryUnitOfWork {
    pub committed_events: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl Default for InMemoryUnitOfWork {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl InMemoryUnitOfWork {
    pub fn new() -> Self {
        Self {
            committed_events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl UnitOfWork for InMemoryUnitOfWork {
    async fn commit<A, R, E, C>(
        &self,
        _aggregate: &A,
        _repository: &R,
        event: E,
        _command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.metadata().event_id.clone());
        UseCaseResult::success(event)
    }

    async fn commit_delete<A, R, E, C>(
        &self,
        _aggregate: &A,
        _repository: &R,
        event: E,
        _command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.metadata().event_id.clone());
        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(&self, event: E, _command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.metadata().event_id.clone());
        UseCaseResult::success(event)
    }

    async fn commit_all<A, R, E, C>(
        &self,
        _aggregates: &[A],
        _repository: &R,
        event: E,
        _command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.metadata().event_id.clone());
        UseCaseResult::success(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_aggregate_type() {
        assert_eq!(
            PgUnitOfWork::extract_aggregate_type("platform.eventtype.123"),
            "Eventtype"
        );
        assert_eq!(
            PgUnitOfWork::extract_aggregate_type("platform.user.abc"),
            "User"
        );
        assert_eq!(PgUnitOfWork::extract_aggregate_type(""), "Unknown");
    }

    #[test]
    fn test_extract_entity_id() {
        assert_eq!(PgUnitOfWork::extract_entity_id("platform.user.123"), "123");
        assert_eq!(PgUnitOfWork::extract_entity_id("platform.user"), "");
    }
}

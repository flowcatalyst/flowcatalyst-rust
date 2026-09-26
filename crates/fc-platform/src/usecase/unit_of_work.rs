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

use super::domain_event::{DomainEvent, RecordedEvent};
use super::error::UseCaseError;
use super::result::UseCaseResult;
use fc_common::audit_redaction::{redacted_command_json, AuditMasked};

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

/// A read a repository makes under a row lock (`SELECT … FOR UPDATE`),
/// inside the unit of work's transaction, so the lock holds until that
/// transaction commits (Java's `TxOperation` reads through
/// `TxScopedUnitOfWork.dbTx()`, e.g. `FunctionVersionRepository.nextVersion`).
/// `Q` names the read; a repository may offer several.
#[async_trait]
pub trait LockedRead<Q: Send + Sync>: Send + Sync {
    type Output: Send;

    async fn read_locked(
        &self,
        query: &Q,
        tx: &mut DbTx<'_>,
    ) -> crate::shared::error::Result<Self::Output>;
}

/// A locked read outside a transaction-scoped unit of work: the lock would
/// be released before anything relied on it.
fn transaction_required() -> UseCaseError {
    UseCaseError::internal(
        "TRANSACTION_REQUIRED",
        "a locked read needs a transaction-scoped unit of work (PgUnitOfWork::run)",
    )
}

/// SQLSTATE `23505`, unique_violation.
const UNIQUE_VIOLATION: &str = "23505";

/// The aggregate a write failure names: its type's short name and its id.
fn aggregate_subject<A: HasId>(aggregate: &A) -> String {
    let type_name = std::any::type_name::<A>();
    let short = type_name
        .split('<')
        .next()
        .unwrap_or(type_name)
        .rsplit("::")
        .next()
        .unwrap_or(type_name);
    format!("{} {}", short, aggregate.id())
}

/// Whether a repository write failed on a unique key: the database's
/// unique violation, or a repository's own `Duplicate`.
fn is_unique_violation(e: &crate::shared::error::PlatformError) -> bool {
    match e {
        crate::shared::error::PlatformError::Duplicate { .. } => true,
        crate::shared::error::PlatformError::Sqlx(sqlx::Error::Database(db)) => {
            db.code().as_deref() == Some(UNIQUE_VIOLATION)
        }
        _ => false,
    }
}

/// A repository write that failed.
///
/// - A write a repository refuses for a business reason of its own (a
///   unique constraint it maps to a code, e.g. `fn_routes`'
///   `PUBLIC_ROUTE_TAKEN`) keeps that code and its `409`.
/// - Any other unique violation is `409 DUPLICATE_KEY` naming the
///   aggregate (Java 9d71ffd4): the use case's validate step checked for the
///   duplicate, and a concurrent writer took the key between that check and
///   this persist, so the caller is told what the check would have said.
/// - Anything else is a failed commit, naming the aggregate.
fn write_failure<A: HasId>(
    what: &str,
    aggregate: &A,
    e: crate::shared::error::PlatformError,
) -> UseCaseError {
    let subject = aggregate_subject(aggregate);
    match e {
        e @ crate::shared::error::PlatformError::BusinessRule { .. } => UseCaseError::from(e),
        e if is_unique_violation(&e) => UseCaseError::business_rule(
            "DUPLICATE_KEY",
            format!("{subject} conflicts with an existing row on a unique key"),
        ),
        e => UseCaseError::commit(format!("Failed to {what} {subject}: {e}")),
    }
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
        C: Serialize + AuditMasked + Send + Sync;

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
        C: Serialize + AuditMasked + Send + Sync;

    /// Emit a domain event and audit log without an entity change.
    ///
    /// Used for events that don't modify an entity directly (e.g., `UserLoggedIn`).
    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync;

    /// Emit a sync's per-row events and its rollup event, each with its
    /// audit row, in one transaction, without an entity change: Go's
    /// `usecaseop.Sync`, which writes a created/updated/deleted event per
    /// synced row and then the rollup. For a sync whose rows its repository
    /// already wrote.
    async fn emit_events<E, C>(
        &self,
        rows: Vec<RecordedEvent>,
        rollup: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync;

    /// [`commit_all`](Self::commit_all) that also writes a sync's per-row
    /// events (each with its audit row) ahead of the rollup `event`, all in
    /// the one transaction.
    async fn commit_all_with_events<A, R, E, C>(
        &self,
        aggregates: &[A],
        repository: &R,
        rows: Vec<RecordedEvent>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync;

    /// A [`LockedRead`] on this unit of work's transaction. Only a
    /// transaction-scoped unit of work ([`PgUnitOfWork::run`]) has one that
    /// outlives the call, so only it holds the lock until its commit; any
    /// other refuses with `500 TRANSACTION_REQUIRED` rather than take a lock
    /// that would be released at once.
    async fn read_locked<Q, R>(&self, repository: &R, query: &Q) -> Result<R::Output, UseCaseError>
    where
        Q: Send + Sync,
        R: LockedRead<Q>;

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
        C: Serialize + AuditMasked + Send + Sync;
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

    async fn persist_audit_log<E: DomainEvent, C: Serialize + AuditMasked>(
        txn: &mut Transaction<'_, Postgres>,
        event: &E,
        command: &C,
        recorded_as: Option<&RecordedCommand>,
    ) -> Result<(), UseCaseError> {
        let mut row = AuditRow::from_event(event, command);
        if let Some(recorded) = recorded_as {
            row.operation = recorded.operation.clone();
            row.operation_json = recorded.operation_json.clone();
        }

        let result = sqlx::query(
            r#"INSERT INTO aud_logs
                (id, entity_type, entity_id, operation,
                 operation_json, principal_id, application_id,
                 client_id, performed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(crate::shared::tsid::generate(
            crate::shared::tsid::EntityType::AuditLog,
        ))
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

    /// Each row event with its audit row, then `event` with its own.
    async fn persist_events_and_audits<E: DomainEvent, C: Serialize + AuditMasked>(
        txn: &mut Transaction<'_, Postgres>,
        rows: &[RecordedEvent],
        event: &E,
        command: &C,
    ) -> Result<(), UseCaseError> {
        Self::persist_events_and_audits_as(txn, rows, event, command, None).await
    }

    async fn persist_events_and_audits_as<E: DomainEvent, C: Serialize + AuditMasked>(
        txn: &mut Transaction<'_, Postgres>,
        rows: &[RecordedEvent],
        event: &E,
        command: &C,
        recorded_as: Option<&RecordedCommand>,
    ) -> Result<(), UseCaseError> {
        for row in rows {
            Self::persist_event_and_audit_as(&mut *txn, row, command, recorded_as).await?;
        }
        Self::persist_event_and_audit_as(&mut *txn, event, command, recorded_as).await
    }

    async fn persist_event_and_audit<E: DomainEvent, C: Serialize + AuditMasked>(
        txn: &mut Transaction<'_, Postgres>,
        event: &E,
        command: &C,
    ) -> Result<(), UseCaseError> {
        Self::persist_event_and_audit_as(txn, event, command, None).await
    }

    /// The event, and its audit row recorded under `recorded_as` when given
    /// (an orchestration's own command), else under `command`.
    async fn persist_event_and_audit_as<E: DomainEvent, C: Serialize + AuditMasked>(
        txn: &mut Transaction<'_, Postgres>,
        event: &E,
        command: &C,
        recorded_as: Option<&RecordedCommand>,
    ) -> Result<(), UseCaseError> {
        Self::persist_event(&mut *txn, event).await?;
        Self::persist_audit_log(&mut *txn, event, command, recorded_as).await?;
        Ok(())
    }
}

/// The command an orchestration records every audit row under, in place of
/// each use case's own: Go's multi-aggregate operations hand their one
/// command to every scoped commit (`usecasepgx.CommitScoped`), so each row
/// names the operation the caller asked for. Built by
/// [`PgUnitOfWork::run_as`]; redacted as any command is.
#[derive(Debug, Clone)]
pub(crate) struct RecordedCommand {
    operation: String,
    operation_json: Option<serde_json::Value>,
}

impl RecordedCommand {
    fn of<C: Serialize + AuditMasked>(command: &C) -> Self {
        Self {
            operation: super::audit_operation::audit_operation_name::<C>().to_string(),
            operation_json: redacted_command_json(command).ok(),
        }
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
    pub correlation_id: Option<&'a str>,
    pub causation_id: Option<&'a str>,
    pub deduplication_id: String,
    pub message_group: Option<&'a str>,
    pub context_data: serde_json::Value,
}

impl<'a> EventRow<'a> {
    /// The event's `Serialize` output is the `data` payload (its own fields,
    /// no envelope: Go's `ToDataJSON`); a serialization failure fails the
    /// commit rather than persisting a placeholder. An event with no fields
    /// persists `{}`, as Go does for an empty payload.
    ///
    /// Correlation id, causation id and message group are NULL when empty,
    /// never `''` (Go's platform sink, `nullIfEmpty`).
    pub(crate) fn from_event<E: DomainEvent>(event: &'a E) -> Result<Self, UseCaseError> {
        let mut data = serde_json::to_value(event).map_err(|e| {
            error!("Failed to serialize domain event: {}", e);
            UseCaseError::commit(format!("Failed to serialize domain event: {}", e))
        })?;
        if data.is_null() {
            data = serde_json::json!({});
        }
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
            correlation_id: non_empty(&meta.correlation_id),
            causation_id: meta.causation_id.as_deref().and_then(non_empty),
            deduplication_id: format!("{}-{}", meta.event_type, meta.event_id),
            message_group: non_empty(&meta.message_group),
            context_data,
        })
    }
}

/// `None` for an empty string, so an optional column stores NULL.
fn non_empty(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Column values of the `aud_logs` row written for a command.
///
/// `id` (a fresh `aud_` TSID, as Go's `tsid.Generate(tsid.AuditLog)`),
/// `application_id` and `client_id` (always NULL) are bound by the INSERT
/// itself.
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
    /// `operation_json` is the command redacted by the audit-redaction rule
    /// (owner spec `docs/spec/audit-redaction.md`, Java repo): secret-named
    /// keys and the command's declared [`AuditMasked`] fields become `"***"`
    /// before the row exists, so no unit-of-work implementation can write a
    /// password or secret into `aud_logs`.
    pub(crate) fn from_event<E: DomainEvent, C: Serialize + AuditMasked>(
        event: &'a E,
        command: &C,
    ) -> Self {
        // Go's name for the command (see `usecase::audit_operation`).
        let operation = super::audit_operation::audit_operation_name::<C>().to_string();

        let meta = event.metadata();
        Self {
            entity_type: PgUnitOfWork::extract_aggregate_type(&meta.subject),
            entity_id: PgUnitOfWork::extract_entity_id(&meta.subject),
            operation,
            operation_json: redacted_command_json(command).ok(),
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
        C: Serialize + AuditMasked + Send + Sync,
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
            return UseCaseResult::failure(write_failure("persist", aggregate, e));
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
        C: Serialize + AuditMasked + Send + Sync,
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
            return UseCaseResult::failure(write_failure("delete", aggregate, e));
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
        C: Serialize + AuditMasked + Send + Sync,
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
                return UseCaseResult::failure(write_failure("persist", aggregate, e));
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

    async fn emit_events<E, C>(
        &self,
        rows: Vec<RecordedEvent>,
        rollup: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
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

        if let Err(e) = Self::persist_events_and_audits(&mut txn, &rows, &rollup, command).await {
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

        UseCaseResult::success(rollup)
    }

    async fn commit_all_with_events<A, R, E, C>(
        &self,
        aggregates: &[A],
        repository: &R,
        rows: Vec<RecordedEvent>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
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
                error!("Failed to persist aggregate in sync: {}", e);
                return UseCaseResult::failure(write_failure("persist", aggregate, e));
            }
        }

        if let Err(e) = Self::persist_events_and_audits(&mut txn, &rows, &event, command).await {
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

        UseCaseResult::success(event)
    }

    async fn read_locked<Q, R>(
        &self,
        _repository: &R,
        _query: &Q,
    ) -> Result<R::Output, UseCaseError>
    where
        Q: Send + Sync,
        R: LockedRead<Q>,
    {
        Err(transaction_required())
    }

    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
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
    /// Set by [`PgUnitOfWork::run_as`]: every audit row this session writes
    /// records this command instead of its use case's.
    recorded_as: Option<RecordedCommand>,
}

impl TxScopedUnitOfWork {
    fn new(tx: Transaction<'static, Postgres>, recorded_as: Option<RecordedCommand>) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
            recorded_as,
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
        C: Serialize + AuditMasked + Send + Sync,
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
            return UseCaseResult::failure(write_failure("persist", aggregate, e));
        }

        if let Err(e) = PgUnitOfWork::persist_event_and_audit_as(
            txn,
            &event,
            command,
            self.recorded_as.as_ref(),
        )
        .await
        {
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
        C: Serialize + AuditMasked + Send + Sync,
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
            return UseCaseResult::failure(write_failure("delete", aggregate, e));
        }

        if let Err(e) = PgUnitOfWork::persist_event_and_audit_as(
            txn,
            &event,
            command,
            self.recorded_as.as_ref(),
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }

    async fn emit_events<E, C>(
        &self,
        rows: Vec<RecordedEvent>,
        rollup: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
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

        if let Err(e) = PgUnitOfWork::persist_events_and_audits_as(
            txn,
            &rows,
            &rollup,
            command,
            self.recorded_as.as_ref(),
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(rollup)
    }

    async fn commit_all_with_events<A, R, E, C>(
        &self,
        aggregates: &[A],
        repository: &R,
        rows: Vec<RecordedEvent>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
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
                error!("Failed to persist aggregate in scoped sync: {}", e);
                return UseCaseResult::failure(write_failure("persist", aggregate, e));
            }
        }

        if let Err(e) = PgUnitOfWork::persist_events_and_audits_as(
            txn,
            &rows,
            &event,
            command,
            self.recorded_as.as_ref(),
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }

    async fn read_locked<Q, R>(&self, repository: &R, query: &Q) -> Result<R::Output, UseCaseError>
    where
        Q: Send + Sync,
        R: LockedRead<Q>,
    {
        let mut guard = self.tx.lock().await;
        let txn = guard.as_mut().ok_or_else(|| {
            UseCaseError::commit("TxScopedUnitOfWork: transaction already finalized")
        })?;
        let mut tx = DbTx { inner: txn };
        repository
            .read_locked(query, &mut tx)
            .await
            .map_err(UseCaseError::from)
    }

    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
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

        if let Err(e) = PgUnitOfWork::persist_event_and_audit_as(
            txn,
            &event,
            command,
            self.recorded_as.as_ref(),
        )
        .await
        {
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
        C: Serialize + AuditMasked + Send + Sync,
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
                return UseCaseResult::failure(write_failure("persist", aggregate, e));
            }
        }

        if let Err(e) = PgUnitOfWork::persist_event_and_audit_as(
            txn,
            &event,
            command,
            self.recorded_as.as_ref(),
        )
        .await
        {
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
        self.run_scoped(None, f).await
    }

    /// [`run`](Self::run), with every audit row the session writes recorded
    /// under `command` (its operation name and redacted JSON) instead of
    /// the command of the use case that wrote it. Each use case still
    /// writes its own event. This is Go's orchestration shape: one
    /// `ProvisionServiceAccountCommand` recorded on the service-account,
    /// application and OAuth-client rows alike.
    pub async fn run_as<C, F, Fut, T>(&self, command: &C, f: F) -> UseCaseResult<T>
    where
        C: Serialize + AuditMasked,
        F: FnOnce(Arc<TxScopedUnitOfWork>) -> Fut + Send,
        Fut: Future<Output = UseCaseResult<T>> + Send,
        T: Send + 'static,
    {
        self.run_scoped(Some(RecordedCommand::of(command)), f).await
    }

    async fn run_scoped<F, Fut, T>(
        &self,
        recorded_as: Option<RecordedCommand>,
        f: F,
    ) -> UseCaseResult<T>
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

        let scoped = Arc::new(TxScopedUnitOfWork::new(tx, recorded_as));
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
    /// The `operation_json` each commit would write to `aud_logs` —
    /// redacted exactly as the Postgres implementations redact it.
    pub committed_audits: std::sync::Mutex<Vec<Option<serde_json::Value>>>,
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
            committed_audits: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn record<E: DomainEvent, C: Serialize + AuditMasked>(&self, event: &E, command: &C) {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.metadata().event_id.clone());
        self.committed_audits
            .lock()
            .unwrap()
            .push(AuditRow::from_event(event, command).operation_json);
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
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
    {
        self.record(&event, command);
        UseCaseResult::success(event)
    }

    async fn commit_delete<A, R, E, C>(
        &self,
        _aggregate: &A,
        _repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
    {
        self.record(&event, command);
        UseCaseResult::success(event)
    }

    async fn emit_events<E, C>(
        &self,
        rows: Vec<RecordedEvent>,
        rollup: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
    {
        for row in &rows {
            self.record(row, command);
        }
        self.record(&rollup, command);
        UseCaseResult::success(rollup)
    }

    async fn commit_all_with_events<A, R, E, C>(
        &self,
        _aggregates: &[A],
        _repository: &R,
        rows: Vec<RecordedEvent>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
    {
        for row in &rows {
            self.record(row, command);
        }
        self.record(&event, command);
        UseCaseResult::success(event)
    }

    async fn read_locked<Q, R>(
        &self,
        _repository: &R,
        _query: &Q,
    ) -> Result<R::Output, UseCaseError>
    where
        Q: Send + Sync,
        R: LockedRead<Q>,
    {
        Err(transaction_required())
    }

    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
    {
        self.record(&event, command);
        UseCaseResult::success(event)
    }

    async fn commit_all<A, R, E, C>(
        &self,
        _aggregates: &[A],
        _repository: &R,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        A: HasId + Send + Sync,
        R: Persist<A>,
        E: DomainEvent + Send + 'static,
        C: Serialize + AuditMasked + Send + Sync,
    {
        self.record(&event, command);
        UseCaseResult::success(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counter;

    #[async_trait]
    impl LockedRead<&'static str> for Counter {
        type Output = i32;

        async fn read_locked(
            &self,
            _query: &&'static str,
            _tx: &mut DbTx<'_>,
        ) -> crate::shared::error::Result<i32> {
            Ok(1)
        }
    }

    /// A lock taken outside a transaction that reaches the commit would be
    /// released at once, so only a scoped unit of work performs one.
    #[tokio::test]
    async fn a_locked_read_needs_a_transaction_scoped_unit_of_work() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let err = PgUnitOfWork::new(pool)
            .read_locked(&Counter, &"fnc_1")
            .await
            .unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code()),
            (500, "TRANSACTION_REQUIRED")
        );
        let err = InMemoryUnitOfWork::new()
            .read_locked(&Counter, &"fnc_1")
            .await
            .unwrap_err();
        assert_eq!(err.code(), "TRANSACTION_REQUIRED");
    }

    struct Thing(&'static str);
    impl HasId for Thing {
        fn id(&self) -> &str {
            self.0
        }
    }

    /// Java 9d71ffd4: a unique key taken between validate and persist is a
    /// 409 naming the aggregate; a repository's own code is kept; anything
    /// else is a failed commit that names the aggregate.
    #[test]
    fn a_unique_violation_at_persist_is_a_conflict_naming_the_aggregate() {
        use crate::shared::error::PlatformError;

        let err = write_failure(
            "persist",
            &Thing("thg_1"),
            PlatformError::duplicate("Thing", "code", "x"),
        );
        assert_eq!((err.http_status_code(), err.code()), (409, "DUPLICATE_KEY"));
        assert!(err.message().contains("Thing thg_1"), "{}", err.message());
        let body = PlatformError::from(err);
        assert!(
            matches!(body, PlatformError::BusinessRule { ref code, .. } if code == "DUPLICATE_KEY")
        );

        let err = write_failure(
            "persist",
            &Thing("thg_2"),
            PlatformError::business_rule("PUBLIC_ROUTE_TAKEN", "taken"),
        );
        assert_eq!(
            (err.http_status_code(), err.code()),
            (409, "PUBLIC_ROUTE_TAKEN")
        );

        let err = write_failure(
            "delete",
            &Thing("thg_3"),
            PlatformError::internal("disk on fire"),
        );
        assert_eq!((err.http_status_code(), err.code()), (500, "COMMIT_FAILED"));
        assert!(
            err.message().contains("delete Thing thg_3"),
            "{}",
            err.message()
        );
    }

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

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct LeakyCommand {
        email: &'static str,
        new_password: &'static str,
        webhook: serde_json::Value,
        value: &'static str,
    }

    impl AuditMasked for LeakyCommand {
        fn audit_masked_fields(&self) -> &'static [&'static str] {
            &["value"]
        }
    }

    const LEAKY: LeakyCommand = LeakyCommand {
        email: "a@b.c",
        new_password: "hunter2",
        webhook: serde_json::Value::Null,
        value: "sk_live_123",
    };

    fn leaky() -> LeakyCommand {
        LeakyCommand {
            webhook: serde_json::json!({"authType": "HMAC_SIGNATURE", "signingSecret": "s3"}),
            ..LEAKY
        }
    }

    fn user_created() -> crate::principal::operations::events::UserCreated {
        crate::principal::operations::events::UserCreated::new(
            &super::super::ExecutionContext::create("prn_actor"),
            "prn_1",
            "a@b.c",
        )
    }

    /// The row every Postgres implementation (PgUnitOfWork and
    /// TxScopedUnitOfWork both go through `persist_audit_log`) writes.
    #[test]
    fn the_audit_row_is_redacted() {
        let event = user_created();
        let json = AuditRow::from_event(&event, &leaky())
            .operation_json
            .expect("operation_json");
        assert_eq!(
            json,
            serde_json::json!({
                "email": "a@b.c",
                "newPassword": "***",
                "webhook": {"authType": "HMAC_SIGNATURE", "signingSecret": "***"},
                "value": "***",
            })
        );
    }

    #[tokio::test]
    async fn the_in_memory_unit_of_work_records_the_redacted_audit() {
        let uow = InMemoryUnitOfWork::new();
        let _ = uow.emit_event(user_created(), &leaky()).await;
        let audits = uow.committed_audits.lock().unwrap();
        let json = audits[0].as_ref().expect("operation_json").to_string();
        for secret in ["hunter2", "s3", "sk_live_123"] {
            assert!(!json.contains(secret), "{secret} reached the audit: {json}");
        }
        assert!(json.contains("a@b.c"));
    }
}

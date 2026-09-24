//! The seam where a function's manifest becomes platform-managed wiring
//! (Java `function/operations/TriggerSync.java`): its dispatch pool
//! `fn-<fid>`, its event-type subscriptions (source `FUNCTION`) and its
//! scheduled jobs, one `fn_trigger_objects` row each.
//!
//! That wiring is created at promote, which is workstream P5. Until then no
//! Rust code path creates any of it, so the two hooks this workstream's use
//! cases call are no-ops.
//!
//! TODO(P5): port `FunctionTriggerSync`:
//! - `on_delete`: delete every linked subscription, scheduled job and the
//!   pool through their own delete use cases (own events and audit), before
//!   the function row, in the same transaction. Links a Java platform wrote
//!   into the shared database would otherwise be orphaned when Rust deletes
//!   the function (`fn_trigger_objects` cascades; the objects do not).
//! - `on_status_change`: `DISABLED` pauses every linked subscription and
//!   scheduled job (never the pool); `ACTIVE` resumes them, in the same
//!   transaction as the status flip.
//!
//! Both need the use cases to run on one transaction
//! (`PgUnitOfWork::run` / `TxScopedUnitOfWork`), as Java's `TxOperation`s do.

use crate::function::entity::Function;
use crate::usecase::{ExecutionContext, UseCaseError};

/// No wiring exists yet, so there is nothing to change.
#[derive(Debug, Clone, Copy, Default)]
pub struct TriggerSync;

impl TriggerSync {
    /// Remove every object `function` owns. No-op until P5.
    pub async fn on_delete(
        &self,
        _function: &Function,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Pause or resume every object `function` owns after a real status
    /// transition. No-op until P5.
    pub async fn on_status_change(
        &self,
        _function: &Function,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }
}

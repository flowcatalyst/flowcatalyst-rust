# Use case template

Every write in fc-platform is a use case in `src/<aggregate>/operations/<verb>.rs`
(see CLAUDE.md, "Use Case / Operations Pattern"). They are written by hand,
not generated: the owner ruled against a macro (2026-09-28), because a
`macro_rules!` body loses rustfmt, rust-analyzer and readable compiler errors,
and hiding `authorize` would hide the part reviewers most need to see.
Uniformity comes from this template plus the convention tests.

Copy this file's shape exactly. The parts marked *fixed* are identical in every
use case; only the command fields and the three bodies change.

```rust
//! Create Widget use case.                                   // one line: what it does

use async_trait::async_trait;                                 // fixed imports
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::WidgetCreated;
use crate::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};
use crate::{Widget, WidgetRepository};

// ── 1. Command ──────────────────────────────────────────────────────────────
/// What the caller asked for. Serialized into the audit row, so field names
/// are the API's (camelCase) and secrets are masked (`AuditMasked`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWidgetCommand {
    /// Formatted fields are parsed value types (e.g. `EventTypeCode`), built
    /// by the handler *after* its permission check, so a 403 wins over a 400.
    pub code: WidgetCode,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl crate::usecase::AuditMasked for CreateWidgetCommand {}  // list secret fields if any

// ── 2. Use case struct + constructor (fixed shape) ─────────────────────────
pub struct CreateWidgetUseCase<U: UnitOfWork> {
    widget_repo: Arc<WidgetRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateWidgetUseCase<U> {
    pub fn new(widget_repo: Arc<WidgetRepository>, unit_of_work: Arc<U>) -> Self {
        Self { widget_repo, unit_of_work }
    }
}

// ── 3. validate → authorize → execute (fixed order) ───────────────────────
#[async_trait]
impl<U: UnitOfWork> UseCase for CreateWidgetUseCase<U> {
    type Command = CreateWidgetCommand;
    type Event = WidgetCreated;

    /// Input shape only: presence, length, format. No I/O.
    async fn validate(&self, command: &CreateWidgetCommand) -> Result<(), UseCaseError> {
        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation("NAME_REQUIRED", "Widget name is required"));
        }
        Ok(())
    }

    /// Resource-level authorization: may *this caller* act on *this target*?
    /// Client reach, application scope, anchor-only, ownership, ceilings.
    /// Use the `checks::*` helpers so the rule text lives in one place.
    /// The coarse permission gate (`can_create_widgets`) stays in the handler,
    /// where Go checks it before decoding the body.
    /// An empty `Ok(())` needs an allowlist entry with a reason in the
    /// authorize convention test.
    async fn authorize(
        &self,
        command: &CreateWidgetCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        crate::checks::require_scope_access(&ctx.caller, command.client_id.as_deref())?; // (phase 2)
        Ok(())
    }

    /// Load, apply business rules, build the event, commit. The happy path
    /// ends in `unit_of_work.commit(...)` (or `commit_delete`, `commit_all`,
    /// `emit_event`, `commit_sync`): `Committed` can't be built any other way.
    async fn execute(
        &self,
        command: CreateWidgetCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<WidgetCreated>, UseCaseError> {
        if self.widget_repo.find_by_code(command.code.as_str()).await?.is_some() {
            return Err(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Widget with code '{}' already exists", command.code),
            ));
        }
        let widget = Widget::create(&command, &ctx);             // aggregate factory, not field-by-field mutation
        let event = WidgetCreated::new(&ctx, &widget);           // event constructor on the event type
        self.unit_of_work
            .commit(&widget, &*self.widget_repo, event, &command)
            .await
    }
}

// ── 4. Tests ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests { /* validation order, error codes; Docker tests live in tests/ */ }
```

## Rules the tests enforce

| Rule | Test |
|---|---|
| Every `execute` happy path reaches a `unit_of_work.*` call | `tests/uow_convention_test.rs` (and `Committed` is sealed, so skipping it doesn't compile) |
| `authorize` is not an empty `Ok(())` without an allowlisted reason | authorize convention test (platform-uniformity phase 2) |
| File shape: command, struct + `new`, `impl UseCase` with `validate`, `authorize`, `execute` in that order | shape convention test (platform-uniformity phase 2) |
| Every `/api` write handler calls a permission check | `tests/route_auth_convention_test.rs` |
| Sync use cases write only through `commit_sync` | `tests/uow_convention_test.rs` |

## What not to do

- Don't write to a repository from `execute` (`repo.insert/update/delete`): the
  unit of work persists the aggregate.
- Don't `return` a hand-built success: there is none to build.
- Don't mutate an aggregate field by field after `new`; add a factory or
  behaviour method on the entity.
- Don't parse formatted strings in `validate` when a value type exists; parse
  in the handler into the command's typed field.
- Don't reach for a generic helper or macro to save the fixed lines. The
  exception is a *family* of use cases that are identical apart from their
  types (e.g. archive-by-id); one generic use case may replace a whole family,
  when a survey shows the bodies really are the same.

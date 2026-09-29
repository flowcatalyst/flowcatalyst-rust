# Domain modelling and code size — plan

Prompted by an outside review (2026-09-29): the Rust platform is ~1.75x the
Go platform's size for the same behaviour, and its domain modelling is
"Go written in Rust" (string ids, flat public-field entities with many
`Option`s, some string statuses, data-less enums). This plan separates the
two complaints, checks them against the code, and orders the work.

Extends `branded-types.md` (its priorities 1-3 become phases 2-3 here) and
runs after the crate split (done) and platform-uniformity phase 2.

## Owner rulings (2026-09-29)

- Line counts are irrelevant: **phase 0 and phase 4 are dropped.**
- Entities stay separate from DTOs and from row structs; the mapping is
  wanted, not overhead.
- Phase 1 done: `ResetApprovalRequest` status is an enum; the portal
  set-status use case no longer defaults a bad status to ACTIVE. Every other
  `status: String` left is a row struct (decoded via `decode`), a wire DTO or a
  command whose `STATUS_INVALID` error code must stay. Unused
  `SyncOpenApiSpecResult` still has a string status.
- Phase 3 pilot 1 done: `ResetApprovalRequest` (private fields,
  `ResetApprovalState` carrying the decider and time).

## What the review gets right and wrong (measured 2026-09-29)

| Claim | Finding |
|---|---|
| ~690 `id: String` fields, ~no id newtypes | Right in substance. No id newtype exists (only `EventTypeCode`, `ProcessCode`, `SecretValue`, and `VersionById`, a query marker). A grep over `crates/*/src` for `*id: String` / `Option<String>` finds ~1,260 including DTOs, row structs and fc-sdk, so the 690 is the entity/command subset. |
| `DispatchJob` is flat, all `Option`, all `pub` | Right. `dispatch_job/entity.rs` is 1,464 lines of public fields. Nothing ties `status = Completed` to `completed_at`. |
| Some statuses are still strings | Right. `mfa/reset_approval.rs:20` `status: String`, plus `portal`, `application_openapi_spec` sync, `function` repository/API and others. Many are response DTOs (fine to stay strings, they mirror Go's wire shape); the domain/row ones are not. |
| 134k vs 76k lines, "same behaviour" | **Not like for like, and not yet trustworthy.** The Rust workspace has whole subsystems Go does not: the function registry and host (`fc-platform-functions` 16k, `fc-fnhost-core` 17.6k, `fc-function-model` 6k, `fc-function-pdk` 3.9k), `fc-web` (15k, trial), `fc-sdk` (20k), MCP, processes. Inline `#[cfg(test)]` code is also mixed into `src`. The real ratio for the shared surface is unknown. |
| "Rust advantage is modest" | Fair for the current code, but that is an argument for doing phases 1-3, not against Rust. |

Where platform lines go (7 platform crates, ~152k lines in `src` including
inline tests): `api.rs` 20.5k, `repository.rs` 13.9k, `entity.rs` 9.9k,
`operations/` 32.5k (179 files, ~180 lines each), `events.rs` 4.9k,
`routes.rs` 3.8k, `bff.rs` 2.6k.

## Framing (owner call needed on the first one)

Better types do **not** shrink the code; newtypes and state types add
lines. The one place they subtract is the ~286 hand-written blank/format
checks in use cases. So "fix the size" and "fix the modelling" are separate
goals with separate finishing lines. This plan does not target line parity
with Go. It targets measured duplication (phase 4) after we know the real
gap (phase 0).

## Phase 0 — measure like for like (small, gates phase 4)

- Count with `tokei` (or `cloc`): production code only, tests and
  `#[cfg(test)]` modules excluded, generated code excluded.
- Rust side: the seven platform crates minus function registry, plus
  `fc-router`, `fc-outbox`, `fc-queue`, `fc-common`, `fc-config`. Leave out
  `fc-sdk`, `fc-web`, `fc-fnhost*`, `fc-function-*`.
- Go side: the equivalent packages in `../flowcatalyst-go`. **Reading Go's
  tree needs your confirmation** (hands-off rule); counting only, no edits.
- Output: a table by area (repository, entity, handlers, use cases, router)
  in `docs/parity/size-comparison.md`. If the ratio for the shared surface is
  well under 1.75x, phase 4 shrinks or disappears.

## Phase 1 — string statuses and stringly fields to enums (small, independent)

- Inventory `status: String` (and similar closed sets: `type`, `kind`) in
  domain structs, commands and row structs; skip response DTOs that mirror
  Go's wire shape.
- Convert each to an enum with `#[serde]`/`#[sqlx]` shapes that leave the
  wire and DB values byte-identical. First one: `ResetApprovalRequest`
  (Go has the four-constant `Status`; Rust should not be weaker).
- Repository rows parse to the enum and return an error on an unknown
  value, not a silent default.
- Gate: the API parity harness and event persistence snapshots unchanged.

## Phase 2 — typed ids (branded-types priority 1)

- One declarative `macro_rules!` (no proc-macro, per the supply-chain rule)
  in `fc-platform-core::ids`: `define_id!(ClientId, "clt")` and so on for
  the 30 `EntityType` variants. Private field, `parse()` checks the TSID
  prefix, `#[serde(transparent)]`, `#[sqlx(transparent)]`, `as_str()`.
- Order, one aggregate group per commit: (1) `PrincipalId` / `ServiceAccountId`
  (the real `prn_`/`sac_` mix-up precedent), (2) `ClientId` and
  `ApplicationId` with `Caller` reach checks (`caller.can_reach(ClientId)`),
  (3) messaging aggregates, (4) the rest.
- Rules from `branded-types.md` hold: HTTP DTOs stay `String`; parse in the
  handler after the permission check; keep Go's exact error codes and order.
- Expect a net line increase here. That is the price of the compile error.

## Phase 3 — validated values, then entities with invariants

- Formatted codes and non-blank names (branded-types priorities 2-3). This
  is where the ~286 blank checks go, so it is the phase that removes code.
- **State-typed entities, pilot first.** Give one aggregate private fields,
  a validating constructor, and per-state data, so "completed with no
  `completed_at`" cannot be built:

  ```rust
  enum DispatchState {
      Pending,
      Queued { queued_at: DateTime<Utc> },
      Completed { completed_at: DateTime<Utc> },
      Failed { last_error: String, last_attempt_at: DateTime<Utc> },
  }
  ```

  The row stays flat in the database; the repository's row-to-entity
  conversion enforces the invariant and errors on a corrupt row.
- Pilot choice: **not** `DispatchJob` first. Its delivery lifecycle is
  infrastructure (no UoW), write-hot and has a separate lightweight
  `SchedulerJobRow`, so a bad design costs throughput and the invariants are
  enforced least there. Pilot on an aggregate that goes through UoW with a
  real state machine (Subscription or ScheduledJob), then apply to
  `DispatchJob` behind a benchmark (`scripts/build-bench` style, ingest and
  dispatch rates before/after).
- Stop condition: if the pilot's diff is bigger than the bugs it would have
  caught justify, say so and stop before rolling out.

## Phase 4 — reduce duplication (only what phase 0 justifies)

Candidates ranked by measured lines, none started without phase 0 numbers:

1. `repository.rs` row structs mirroring `entity.rs` (13.9k + 9.9k):
   derive `FromRow` onto the entity where no invariants are needed, keep a
   row type only where the row differs.
2. `operations/` boilerplate (32.5k, 179 files): shared helpers for the
   validate/authorize/commit skeleton and event construction (`events.rs`
   4.9k). Declarative macros only; visible SQL and use-case bodies stay.
3. `api.rs` request/response DTO plumbing (20.5k): only if repetition, not
   the Go-contract shape, is what's big.

## Order and gates

0 -> 1 in parallel with 2 -> 3 (pilot) -> 4 if justified. Every commit is one
aggregate group, gated by the route-table snapshot, API parity harness, event
persistence snapshots, and `cargo test` per crate. Update the SDKs' parity
plan only if a wire shape changes (none should).

## Open questions

1. Confirm counting `../flowcatalyst-go` for phase 0 (read-only).
2. Is a state-typed entity pilot acceptable if it makes the entity file
   longer? (Recommendation: yes; the goal is unrepresentable bad states.)
3. Should phase 4 start at all if phase 0 shows the shared-surface ratio is
   near 1.2x or less? (Recommendation: no.)

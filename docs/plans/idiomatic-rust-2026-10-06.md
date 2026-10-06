# Plan: make the compiler carry the correctness (2026-10-06)

Status: proposed, nothing started. Owner decisions needed are listed in section 5.

## 1. Purpose

The reason to own a Rust implementation is that the compiler rejects whole classes of mistake.
Today this codebase collects only part of that. The aim of this plan is to move rules that are
currently held by convention, tests or review into types, so that a wrong change fails to build.
It is not a style pass: a change belongs here only if it makes the compiler catch something it
does not catch now, or removes a hazard.

Everything is behaviour-preserving. The wire contract, the database schema and the measured
performance must not change.

## 2. Where the code stands (survey of 2026-10-06)

Non-test Rust: 877 files, about 223k lines, 22 workspace crates. Counts are grep counts over
that corpus and are upper bounds where residual test code remains.

The code is **not uniformly "Go written in Rust"**. The right mechanisms exist and are good;
they are applied to a small part of the code.

| Area | What exists | Gap |
|---|---|---|
| SQL | `FromRow` row structs, SQL confined to repositories | 893 runtime-checked queries, **0** compile-time checked; no `.sqlx/` cache. 202 `SELECT *`. |
| Identifiers | `Id<K>` with validated parse, sqlx/serde/utoipa support; 22 kinds declared | 14 typed fields against **1,079** bare `String` id fields and 466 `&str` id parameters. 13 of the 22 kinds are unused. No `DispatchJobId` or `EventId`. |
| Enums | 222 enums; entities use them (189 enum-typed status fields) | 253 `String` status-like fields in DTOs and row structs; 48 SQL statements repeat enum spellings as literals; about 15 string-literal comparisons in logic; no enum derives `sqlx::Type`. |
| Entity state | Sealed `Committed<T>` so a use case cannot succeed without the unit of work | 601 `pub` fields against 1 private in entity files. `DispatchJob` and others are a status plus `Option` fields valid only in some statuses; transitions do not check the current state. |
| Errors | Typed `PlatformError` / `UseCaseError` in the platform crates | `anyhow` in library crates (`fc-outbox` 29, `fc-stream` 13, `fc-mcp` 11, `fc-platform-core` 10); 80 `map_err(to_string)`, 88 `map_err(\|_\|` dropping the source; 142 `let _ =`. |
| Shared state | Supervised tasks, `JoinSet` lanes, cancellation tokens | Router: 25 mutexes, 17 rwlocks, 15 concurrent maps, 89 atomics, three lock families mixed; a 500-line `route_batch_inner`; per-message `String` clones on the hot path. |
| Traits | Generic use-case layer; real polymorphism where there are several backends | About 20 `dyn` traits with exactly one production implementation; 291 `#[async_trait]`. |
| Lints | Clippy and `-Dwarnings` in CI; cargo-deny and cargo-vet | No `[workspace.lints]` (23 copied blocks); `wildcard_enum_match_arm` in only 7 crates; no `unsafe_code` lint (25 `unsafe` sites). |
| Tests | About 3,400 tests | The 360 database-backed tests are `#[ignore]` and **never run in CI**. |

Models to copy, already in the tree: `fc-platform-core/src/usecase/result.rs` and
`unit_of_work.rs` (sealed success), `shared/id.rs` and `ids.rs` (typed ids),
`shared/enum_str.rs` (strict enums), `fc-common/src/diagnostics/supervise.rs` (supervised
loops), `fc-stream/src/lib.rs` and the scheduler's lane `JoinSet` (structured tasks).

## 3. Rules for every phase

1. **One phase, one concern, its own commits.** No drive-by changes.
2. **Behaviour-preserving.** The OpenAPI document is diffed before and after and must be
   identical unless the phase says otherwise. No migration is added.
3. **The gate is the full suite including the database tests**, run in the foreground, with
   failures checked explicitly before any commit.
4. **Router and scheduler phases re-run the benchmark rig** (`../flowcatalyst-javalin/bench/router`,
   `run.sh` and `sched.sh`) before and after; a regression beyond run-to-run noise blocks the
   phase.
5. **The compiler leads.** Change the type, then fix what stops compiling. Do not add
   conversions back to `String` to make an error go away; if a boundary really needs a string,
   convert once at that boundary.
6. **No new `#[allow]`** without a comment giving the reason.

## 4. Phases

Ordered so that each phase makes the next one cheaper. Sizes are from the survey.

### Phase 0 — Safety net (small)

Do this first; every later phase depends on it.

- Run the database-backed tests in CI: a Postgres service (or testcontainers on the runner) and
  `--include-ignored` for the Docker-gated set. 360 tests, one workflow.
- One `[workspace.lints]` table inherited by every crate, replacing the 23 copies:
  `unsafe_code = "forbid"` (explicit, commented allows in `fc-fnhost-js`, `fc-fnhost-core`,
  `fc-dev`, `fc-common`), `wildcard_enum_match_arm` everywhere, and in library crates
  `unwrap_used`, `expect_used`, `panic`, `let_underscore_must_use` as warnings. Triage what
  fires (about 300 `_ =>` arms, 142 `let _ =`).
- Add `rust-version` and a `rust-toolchain.toml` so builds are reproducible.

Compiler gain: new enum variants must be handled in every crate; new `unsafe`, discarded
`Result`s and stray panics need a visible exemption.

### Phase 1 — Typed identifiers everywhere (large, mechanical)

- Add the missing kinds: `DispatchJobId`, `EventId`, `MessageId`, and any other id that has a
  TSID prefix.
- Convert every `*_id` field and parameter to `Id<K>`: about 1,079 fields and 466 parameters
  in about 150 files. Work crate by crate, innermost first (`fc-platform-core`, then
  `messaging`, `iam`, `auth`, `functions`, `scheduled-jobs`, then `fc-platform`).
- Replace the roughly 22 signatures with three or more consecutive string parameters by a
  parameter struct or typed arguments.

Compiler gain: passing a subscription id where a client id is expected stops compiling.

### Phase 2 — Enums across the boundary (medium)

- `#[derive(sqlx::Type)]` (text-backed) on the string enums, so the 34 row structs hold the
  enum and the separate `decode` step goes.
- Bind the enum in SQL in place of the 48 hard-coded literals (`status = 'ACTIVE'`).
- DTOs and commands carry the domain enum, with the schema derived from it: about 60 fields,
  removing the roughly 15 string comparisons in logic (`dispatch_job_actions`,
  `set_client_association`, `principal/api`, `monitoring_api`, SDK claims).
- `DispatchMode` stays lenient at the wire edge, as already ruled (X-01).

Compiler gain: a renamed or added variant is checked in SQL parameters, row mapping and
request handling, not only in the domain.

The OpenAPI document may gain `enum` constraints where it had plain strings. That is a contract
tightening and needs the owner's agreement per field (section 5).

### Phase 3 — Compile-time checked SQL (largest)

Needs phases 1 and 2 first, so the macros can map straight to `Id<K>` and the enums.

- Commit a `.sqlx/` offline cache; CI runs `cargo sqlx prepare --check`. Builds do not need a
  database.
- Convert the static queries (about 750, including the 51 that only interpolate a constant
  column list) to `query_as!` / `query!`, one repository at a time. Replace the 202 `SELECT *`
  with explicit column lists as part of each conversion.
- Leave as runtime queries, by design: the 83 `QueryBuilder` filter queries, the roughly 11
  variable-SQL sites, and the multi-database crates (`fc-outbox`, `fc-queue`), where one query
  text serves Postgres, MySQL and SQLite.
- Audit the 79 `fetch_one` calls against the existing rule while each repository is open.

Compiler gain: a renamed column, a wrong type or a wrong nullability fails the build.

This reverses the recorded rule in `CLAUDE.md` ("Queries: `sqlx::query_as::<_, FooRow>`") and
the 2026-09-28 note that deferred checked queries. `CLAUDE.md` is updated in this phase.

### Phase 4 — State as types (medium, highest design content)

- `DispatchJob`: replace `status` plus optional fields with a state enum carrying each state's
  data (`Pending { scheduled_for }`, `Queued`, `Processing { since }`,
  `Completed { at, duration }`, `Failed { at, last_error }` and the rest of the lifecycle), with
  transitions that consume the old state. The transition table already written in
  `fc-common/src/dispatch_lifecycle.rs` is the source of truth; the in-memory type must not
  allow a transition that table forbids.
- The same treatment for scheduled-job instances and the other status-plus-options entities
  found in the survey (`Principal`, `Subscription` to be confirmed by reading).
- Private fields and constructors on the aggregates that have state machines, with accessors
  and transition methods. Start with the three or four that matter; do not privatise all 601
  fields for its own sake.
- Replace the 344 build-then-patch assignments in `operations/` for those aggregates with
  methods on the entity.

Compiler gain: a completed job without a completion time, or a job marked in progress after it
completed, cannot be constructed.

### Phase 5 — Typed errors in the libraries (medium)

- `thiserror` enums in `fc-outbox`, `fc-stream`, `fc-queue`, `fc-mcp` and the remaining
  `fc-platform-core` sites; `anyhow` stays only in the binaries.
- Replace `map_err(|e| e.to_string())` (80) and `map_err(|_| ...)` (88) with variants that
  keep the source error.
- Resolve the discarded results that matter (section 6).

Compiler gain: callers must handle each failure kind; error causes are no longer erased.

### Phase 6 — Ownership and structure in the router (medium, performance-sensitive)

- Split `route_batch_inner` (about 500 lines) and the other functions over 150 lines in the
  router along the ownership of the data they touch.
- Move each message into the task that processes it; remove the per-message `String` clones in
  `manager/routing.rs` (receipt handle, broker id, message id) with moves or shared immutable
  strings.
- One lock family per crate, chosen deliberately; document the choice. Review each shared
  structure: can it be owned by one task and reached by a channel, instead of locked?
- Detached `tokio::spawn` calls (about 33) go into a `JoinSet`, a `TaskTracker` or the existing
  supervisor, so no task outlives shutdown unnoticed.
- Remove the roughly 20 `dyn` traits with a single production implementation (generic parameter
  or concrete type), and `#[async_trait]` wherever the trait is not used as `dyn`.
- Pass `&mut Transaction` through the unit-of-work closure in place of
  `Arc<Mutex<Option<Transaction>>>`.

Compiler gain: data handed to a task cannot also be mutated elsewhere; fewer places where
correctness rests on remembering to take a lock.

This phase is gated on the benchmark rig, before and after.

### Phase 7 — Wiring (optional)

The two `main` functions (985 and 502 lines) and the 93 per-route-group state structs are hard
to read but are not a correctness risk. Finish the `PlatformContext` consolidation already
planned in `platform-uniformity-2026-09-28.md` only if the earlier phases leave appetite.

## 5. Decisions needed from the owner

1. **Go ahead at all?** This is a large investment in an implementation that is not the one in
   production. Phases 0 to 3 are mostly mechanical and agent-sized; phases 4 and 6 need design
   review.
2. **Reverse the runtime-SQL rule** in `CLAUDE.md` (phase 3).
3. **OpenAPI tightening** in phase 2: may string fields become enumerations in the published
   contract, or must the document stay byte-identical (enums internal only)?
4. **Multi-database crates** stay on runtime queries (proposed), or get per-database checked
   queries (three caches, more work).
5. **Scope of private fields** in phase 4: state-machine aggregates only (proposed), or all
   entities.

## 6. Hazards found during the survey

To be fixed in the phase noted, or sooner.

Confirmed by reading:

- The 360 database-backed tests do not gate merges (phase 0).
- `fc-platform-auth/src/auth/oauth_api.rs:2592`: a failed refresh-token revoke is discarded
  silently on one branch; the other branch logs it (phase 5).
- `fc-platform-auth/src/mfa/service.rs:388,396`: a failed PIN delete is discarded; the attempt
  counter still blocks the PIN (phase 5).
- `fc-platform-messaging/src/dispatch_job_actions/operations.rs:205,214`: status guards are
  string comparisons (phase 2).

Suspected, to verify:

- `fc-router/src/api/mod.rs:301-304` lists the `/api/test/*` mock endpoints, including a
  60-second sleep, in the production route list. Confirm they are mounted only in development.
- `bin/fc-outbox-processor/src/main.rs:166-169` discards the join result, so a panicked
  processor task may exit with status 0.
- `fc-queue/src/activemq.rs`: a read guard is held across broker calls and could delay a
  reconnect.
- `bin/fc-server/src/main.rs:851,870,1078`: a poisoned standard-library lock on the CORS cache
  would make every later request panic.
- `DispatchJob::complete_success` and `record_failure` do not check the current status. The
  production scheduler goes through SQL in `dispatch_lifecycle.rs`; whether anything else calls
  these in production was not checked (phase 4 removes the question).
- The 25 `unsafe` sites have not been audited.

## 7. Measuring success

At the end, each of these must be true, and each is checkable mechanically:

- A column rename in a migration fails `cargo build` for every static query that uses it.
- No `*_id` field or parameter is a bare `String` outside wire DTO decoding.
- No status-like field is a `String` outside the lenient wire edge.
- An illegal dispatch-job state cannot be constructed in safe code.
- No library crate exposes `anyhow`.
- The database tests run on every pull request.
- Router and scheduler throughput on the rig are within noise of the figures recorded before
  phase 6.

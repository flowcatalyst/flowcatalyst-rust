# Platform uniformity: route wiring and authorization placement

Owner request, 2026-09-28. Two structural fixes to fc-platform, plus SQLx
housekeeping. **Behaviour must not change**: every HTTP status, body, header,
event and audit row stays byte-identical (the Go parity contract). The API
parity harness (`harness/parity`, baseline 1227 OK / 124 ACCEPTED / 16 DIFF /
0 ERROR on `main`), the Docker suite and the convention tests gate every step.

## Library check (done)

SQLx stays. It is the one driver that serves Postgres, MySQL and SQLite
(the outbox processor and SDK outboxes read consumer apps' tables in all
three); the codebase is hand-written Postgres SQL (UNNEST, `ANY`, `RETURNING`,
JSONB, partitions) that ORMs fight; the function DB pools rely on
`after_release` hooks and in-place connect-option updates for secret
rotation. Its measured per-query overhead vs tokio-postgres (~20 µs CPU) is
immaterial against RDS round trips (see memory `project_sqlx_vs_tokio_postgres`).
Compile-time checked queries (`query!` + offline `.sqlx/`) are the one
upgrade worth considering later; not in scope.

The SeaORM → SQLx migration finished in April 2026. Remaining housekeeping
(phase 0): a stale SeaORM comment in `role/entity.rs`, CLAUDE.md's "SQLx
Migration (In Progress)" section, and `platform_config/repository.rs`, the
one repository still building its WHERE with `format!` instead of
`sqlx::QueryBuilder`.

## Phase 1: route wiring (one module per aggregate)

**Today.** `router.rs` (1,011 lines) imports ~60 router functions through a
re-export module (`crate::api`) and nests each at a `PATH_*` constant;
`PlatformRoutes` holds ~55 hand-built per-aggregate `*State` structs, built one
by one in `shared/server_setup/platform_routes.rs` (1,393 lines) and again in
tests. Aggregates split their handlers across `api.rs`, `go_api.rs`,
`*_api.rs`, `shared/bff_*_api.rs`, with no rule for which goes where.

**Target** (Java's `XApi.register(routes, state)`, in axum terms):

- One shared dependency bundle, `PlatformContext` (repositories, unit of work,
  authorization/auth services, config, rate-limit store, …), built once.
- Every aggregate module exposes **one** entry point,
  `pub fn routes(ctx: &PlatformContext) -> AggregateRoutes`, returning its
  documented routes (`OpenApiRouter`) and plain routes (`Router`) **already at
  their full paths**. It builds its own `*State` from the context. The paths
  live next to the handlers, not in `router.rs`.
- `router.rs` becomes a list of modules plus the cross-cutting layers (auth
  layer, rate limits, OpenAPI/Swagger, SPA, health), in the same order as today
  so middleware semantics don't move.
- File rule per aggregate: `api.rs` holds `/api/*` handlers, `bff.rs` holds
  `/bff/*` handlers, `routes.rs` (or `mod.rs`) holds `routes()`. The historical
  `go_api.rs` / `api.rs` pairs merge into that shape. Shared plumbing
  (`shared/*_api.rs` that are not an aggregate's) stays in `shared/`.
- `PlatformRoutes`' ~55 state fields go away; binaries (fc-server, fc-dev) and
  tests build a `PlatformContext` and call `build`.
- **Guardrails:** the route-auth convention test keeps passing unchanged; a new
  convention test fails if `router.rs` imports a handler or state type, or an
  aggregate registers routes outside its `routes()`. A route-table snapshot
  test (method + path + auth requirement for every route, before vs after)
  proves the refactor moved nothing.

## Phase 2: authorization placement (resource checks in use cases)

**Today.** 132 of 139 use cases have an empty `authorize`. Permission *and*
resource checks (client reach, application scope, anchor-only, ownership)
live in HTTP handlers, so every caller of a use case (API, BFF, fc-web, sync
paths) must repeat them, and a missing handler check is a privilege
escalation. `ExecutionContext` carries only ids, so a use case *cannot*
authorize today.

**Target** (Java's `.authorize(cmd -> Checks.checkScopeAccess(...))`):

- `ExecutionContext` carries the caller's authority: a `Caller` (principal id
  and type, scope/tier, accessible clients, permissions, application scope,
  credential kind) taken from `AuthContext` in `ExecutionContext::from_auth`.
  System-initiated contexts (startup sync, scheduler, bootstrap) carry an
  explicit `Caller::system()` — never an anonymous default.
- **Resource-level checks move into `authorize`**: whatever depends on the
  command or the loaded target (client reach for `cmd.client_id`, application
  scope, anchor-only operations, ownership, role/permission ceilings). Helpers
  come from `shared::authorization_service::checks`, so the rule text stays in
  one place.
- **The coarse permission gate stays at the handler entry** (`can_create_*`
  etc.). Go checks permissions before decoding the body, so an unauthorised
  caller gets 403 before any 400; `UseCase::run` validates before it
  authorizes, so moving the permission check into `authorize` would turn some
  403s into 400s. Keeping it in the handler preserves Go's order; it also
  stays enforced by the route-auth guardrail. (It may *also* be asserted in
  `authorize` as defence in depth where that can't change the answer.)
- Where a check needs the loaded aggregate (e.g. the target's client), the use
  case loads it in `authorize` or keeps the check at the top of `execute`,
  before any write, and says so. The error must stay exactly what the handler
  returned (status, code, message).
- **Guardrail:** a convention test fails any use case whose `authorize` is
  `Ok(())` unless it is on an allowlist with a reason (e.g. "self-service: the
  caller acts on themselves", "platform-internal: system caller only").
- **Use-case shape (owner, 2026-09-28: no macro).** Use cases stay hand-written
  to `docs/architecture/use-case-template.md`. Phase 2 adds a shape convention
  test (command, struct + `new`, `impl UseCase` with validate/authorize/execute
  in order), updates the template once `Caller` exists, and surveys for
  *families* of use cases identical apart from their types (e.g.
  archive-by-id), replacing a family with one generic use case only where the
  bodies really are the same.
- Reads get the same treatment later through shared read loaders
  (`<aggregate>::read::load_for(caller, id)`), which fc-web will use
  (`docs/topcoat-trial.md`, "Authorization rules for server calls"). Out of
  scope for this pass unless cheap.

## Order and gates

1. Phase 0 + phase 1 on one branch (route wiring touches every `api.rs`, so it
   goes first and alone).
2. Phase 2 after phase 1 merges, by domain group (IAM; messaging; platform
   admin; functions), each group a branch, merged in sequence.

Every step: `cargo check --workspace --all-targets`; clippy at the baseline
(4); workspace tests; convention tests; the Docker suite (287 on `main`);
`FC_SKIP_FRONTEND_BUILD=1 cargo check -p fc-dev --features web` and the fc-web
tests (fc-web calls use cases and builds routes); the API parity harness with
no new DIFF and no stale allow-list entry; the event persistence snapshot
tests byte-identical.

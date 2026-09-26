# FlowCatalyst Rust - Development Guidelines

## HTTP Tier Convention

The platform exposes exactly two programmable tiers and an internal one:

- **`/bff/*`** — frontend-only. Cookie/session auth. Response shapes are tuned
  to screens; callers outside the frontend should not depend on them.
- **`/api/*`** — the single programmable surface for SDKs and external
  consumers. Bearer token auth. Authorization is enforced by **permissions**
  (role/permission checks inside handlers), not by URL tier.
- **`/auth/*`, `/oauth/*`, `/.well-known/*`, `/api/dispatch/*`, `/api/monitoring/*`,
  `/api/me/*`, `/api/public/*`** — platform-owned, do not move.

**There is no `/api/admin/*` or `/api/sdk/*` anymore.** Any write handler under
`/api/*` MUST call an explicit authorization check (`require_anchor`,
`require_permission`, or one of the `can_*` helpers) — because the URL prefix
no longer provides a second line of defense. Missing a permission call on a
write handler is a privilege-escalation bug.

## UoW Invariant (Sealed)

`UseCaseResult::success` is sealed (`pub(in crate::usecase)`). The only code
that can construct a success is `UnitOfWork::commit` / `commit_delete` /
`emit_event` / `commit_all`, plus the `.map()` combinator inside the usecase
module. A use case that tries to `return UseCaseResult::success(event)` without
routing through UoW fails to compile. This is **stronger than the TS runtime
token** — compile-time guaranteed, zero cost.

What this means for every `*UseCase::execute`:
1. The happy path must end in `unit_of_work.commit(...)`, `commit_delete(...)`,
   `emit_event(...)`, or `commit_all(...)` — or in `.map(|_| ...)` chained onto
   one of those.
2. The only other legal tail is `UseCaseResult::failure(...)`.
3. You cannot skip UoW and return a hand-built success. It's a type error.

Aggregates can't persist themselves — `impl Persist<X> for XRepository`
lives on the repository, not on the aggregate. Use cases write via
`unit_of_work.commit(&agg, &*self.repo, event, &command)` (or
`commit_delete`). Direct `repo.insert/update/delete` from a use case body
is forbidden by convention; `tests/uow_convention_test.rs` asserts that
every use case's `execute` body reaches a `unit_of_work.*` call on the
happy path, catching any regressions.

**Consequence:** if you see a write action with no corresponding row in
`msg_events` / `iam_audit_logs`, the bug is almost certainly in the handler
bypassing the use case, not in the use case itself.


## Database Access Rules

### N+1 Query Prevention
Never call a query inside a loop. This is the #1 performance issue in this codebase.

**Banned pattern:**
```rust
for item in items {
    item.children = self.load_children(&item.id).await?; // N queries!
}
```

**Required pattern — batch load with IN clause:**
```rust
let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
let all_children = sqlx::query_as::<_, ChildRow>(
    "SELECT * FROM children WHERE parent_id = ANY($1)"
)
.bind(&ids)
.fetch_all(&self.pool).await?;

// Group by parent_id in memory
let mut map: HashMap<String, Vec<Child>> = HashMap::new();
for c in all_children {
    map.entry(c.parent_id.clone()).or_default().push(c.into());
}
```

**For inserts — use UNNEST, not loops:**
```rust
// Bad: N inserts
for item in items { sqlx::query("INSERT...").bind(&item).execute(&pool).await?; }

// Good: 1 insert
sqlx::query("INSERT INTO t (a, b) SELECT * FROM UNNEST($1::text[], $2::text[])")
    .bind(&a_values).bind(&b_values).execute(&pool).await?;
```

### Concurrent Independent Queries
When a handler needs data from multiple tables, use `tokio::try_join!` instead of sequential awaits:
```rust
let (clients, events, pools) = tokio::try_join!(
    repo.find_clients(),
    repo.find_events(),
    repo.find_pools(),
)?;
```

### Prefer `fetch_optional` Over `fetch_one`
`fetch_one` is a runtime panic waiting to happen — treat it like `.unwrap()`. Always use `fetch_optional` and handle `None` unless the query is **mathematically guaranteed** to return a row (e.g., `SELECT COUNT(*)`).

```rust
// Bad: panics at runtime if no rows
let row: (i64,) = sqlx::query_as("SELECT id FROM foo WHERE bar = $1")
    .bind(bar).fetch_one(&pool).await?;

// Good: compile-time safety
let row = sqlx::query_as::<_, (i64,)>("SELECT id FROM foo WHERE bar = $1")
    .bind(bar).fetch_optional(&pool).await?;
match row {
    Some((id,)) => { /* use id */ }
    None => { /* handle missing */ }
}
```

The **only** acceptable use of `fetch_one` is on aggregate queries that always return exactly one row: `SELECT COUNT(*)`, `SELECT MAX(...)`, `SELECT EXISTS(...)`.

### Shallow Queries for Filter/List Endpoints
If a handler only needs a few fields (e.g., id + name for a dropdown), don't load junction tables or child entities. Add a `find_*_shallow()` method that skips hydration.

## SQLx Migration (In Progress)
We are migrating from SeaORM to raw SQLx. New repositories should use `sqlx::PgPool` with handwritten SQL. Pattern:
- Row structs: `#[derive(sqlx::FromRow)]` in the repository file
- Queries: `sqlx::query_as::<_, FooRow>("SELECT ...")` — visible SQL, no ORM magic
- Domain entities stay in `*/entity.rs`, row mapping stays in `*/repository.rs`
- Connection: use `shared::database::create_pool()` for SQLx repos

## Caching
- **Token validation**: `AuthService` caches validated JWT claims (DashMap, 30s TTL)
- **Permission resolution**: `AuthorizationService` caches role→permissions (DashMap, 60s TTL)
- Both caches exist to avoid repeated RSA verification and DB queries on every authenticated request

## Static Asset Serving
Vite hashed assets (`/assets/*`) are served with `Cache-Control: public, max-age=31536000, immutable`. The SPA shell (`/`, `/index.html`, and the SPA fallback for any path no route claims) is never cacheable (`no-cache, no-store, must-revalidate`, `router::SPA_SHELL_CACHE_CONTROL`), so a deploy is picked up on the next load; other static files keep default caching. `fc-platform::router::serve_spa` does this for `FC_STATIC_DIR`; `fc-dev` does the same for the `frontend/dist` it embeds.

## Use Case / Operations Pattern

### UseCase Trait Contract
Every write operation MUST implement the `UseCase` trait, which enforces three steps:
1. **`validate`** — Input validation (field presence, format, length). Return `Ok(())` if none needed.
2. **`authorize`** — Resource-level authorization (ownership, access checks). Return `Ok(())` if none needed.
3. **`execute`** — Business logic: load aggregate, check business rules, build domain event, call `unit_of_work.commit()`.

Handlers call `use_case.run(command, ctx)` which executes validate → authorize → execute in order.

### No Direct DB Writes Outside Operations
All write operations (create, update, delete, state transitions) MUST go through a use case in `*/operations/`.
Handlers (BFF, SDK, admin API) are thin adapters that:
1. Check permissions (role/permission-level authorization)
2. Build a Command from the request DTO
3. Create an `ExecutionContext::from_auth(&auth.0)`
4. Call `use_case.run(command, ctx).await.into_result()?`
5. Convert the result to an HTTP response

**Never call `repo.insert()`, `repo.update()`, or `repo.delete()` directly from a handler.**
The use case layer ensures: validation, authorization, domain events, audit logs, and atomic commits via UnitOfWork.

### Exceptions: Platform Infrastructure Processing
The **only** operations that bypass UseCase/UnitOfWork are the platform's own internal
infrastructure — the machinery that moves messages through the pipeline. These cannot
generate events/audit logs (that would be recursive — a UoW commit emits a domain event,
so creating an event via UoW would mean emitting an event about the event):

- **Event ingest**: `POST /api/events/batch` — stores events received from consumer apps
- **Dispatch job ingest**: `POST /api/dispatch-jobs/batch` — stores dispatch jobs from consumer apps
- **Stream processing**: `events_raw` CQRS projection into `msg_events`
- **Dispatch job delivery lifecycle**: status transitions during webhook delivery (pending → in_progress → success/failed), attempt recording
- **Outbox processing**: polling `outbox_messages` and forwarding to platform API
- **Auth/OIDC token storage**: refresh token, authorization code, OIDC pending-auth
  state, and OIDC login state inserts (`auth/oauth_api.rs`, `auth/auth_api.rs`,
  `auth/oidc_login_api.rs`). These are short-lived session records — wrapping
  them in UoW would emit a domain event per token, swamping the event log on
  every login or token refresh. Login/logout *outcomes* (e.g. `UserLoggedIn`,
  `UserLoggedOut`) ARE emitted via UoW; only the token-row plumbing bypasses.
- **Built-in role seeding**: startup-time hydration of code-defined roles via
  `shared/database.rs::seed_built_in_roles` and `shared/role_sync_service.rs`.
  Bootstrap-only, runs before HTTP serving begins, no executing principal.
- **Scheduled-job firings**: every cron tick (and the dispatcher's status
  transitions during webhook delivery) writes to
  `msg_scheduled_job_instances` directly. The SDK callback paths
  (`POST /api/scheduled-jobs/instances/:id/log`,
  `POST /api/scheduled-jobs/instances/:id/complete`) write to
  `msg_scheduled_job_instance_logs` / update the instance row directly.
  Wrapping any of these in UoW would emit one domain event per firing /
  log line, swamping the event log. The *definitions* (`ScheduledJob`
  CRUD: create / update / pause / resume / archive / delete / sync) DO go
  through UoW with full event + audit. `ScheduledJobFiredManually` is the
  exception that proves the rule: it is the audit record for the human
  action; the instance row inserted alongside is still the infrastructure
  path.
- **Function-host heartbeat**: `POST /control/functions/heartbeat`
  (`function/control_api.rs::heartbeat`) upserts the host's `fn_hosts` row
  and purges hosts silent for over a day, in one transaction, through
  `function/host_repository.rs::FunctionHostRepository::heartbeat`, with no
  event and no audit row. A heartbeat is telemetry, every 15 s per host;
  wrapping it in UoW would emit a domain event per beat and swamp the event
  log. What a heartbeat *causes* is still a use case: a version the host
  reports loaded becomes `READY` through `MarkVersionReadyUseCase`, with its
  event and audit row.
- **Function artifact uploads**: `PUT /api/functions/{address}/artifacts/{digest}`
  (`function/version_api.rs::upload_artifact`, `function/artifact/upload.rs`)
  streams the blob into the configured artifact store (file or S3), with no
  event and no audit row. Storing a blob changes nothing a caller can observe
  until a version is published against it, and that publish is a use case
  with its event and audit row; an orphan blob is garbage, not state, which a
  function delete collects. The route is still permission-gated
  (`platform:function:version:publish`) like any other `/api/*` write.
- **Lazy OAuth client-secret rehash**: after a client authenticates
  successfully at `/oauth/token` against a secret stored in an older format,
  `auth/oauth_api.rs::accept_client_secret` rewrites it to the current
  `hashed:v1:` form through
  `auth/oauth_client_repository.rs::rewrite_secret_ref` (and
  `rewrite_previous_secret_ref` for the rotation-overlap secret; the
  once-a-minute `touch_previous_secret_used` stamp is the same kind of
  write). It upgrades the at-rest format of a secret the caller just proved
  it holds, not the secret itself, and runs on the token endpoint's hot path,
  so it emits no event and no audit row. It is best-effort (a failure is
  logged; the authentication already succeeded) and conditional on the row
  still holding the verified ref, so a concurrent rotation, which *is* a use
  case, is never overwritten.
- **Second-factor (2FA) rows and self-service password change** (owner,
  2026-09-26): enrolled methods, recovery codes, email PINs and trusted
  devices (`mfa/repository.rs`, written from `mfa/login_api.rs`,
  `mfa/self_service_api.rs`, `mfa/admin_api.rs`), and the signed-in user's
  own `POST /auth/change-password` (`mfa/account_api.rs`), are written
  directly, as Go does. They are per-login plumbing: an event per PIN, code
  use or trusted-device stamp would swamp the event log. Go's audit rows are
  still written for the security-relevant steps (`mfa/audit.rs`, e.g. a
  method enrolled or removed, recovery codes regenerated, an admin 2FA reset).
  Codes are spent by one guarded UPDATE/DELETE so each signs in once.
  Administrative user changes (create, update, activate, admin password
  reset) stay use cases.

These go directly to the repository. They are the platform's internal plumbing.

Any `createEvent` / `createDispatchJob` code path — SDK, admin UI, internal
caller, reprocessing tool — falls in this category and **must not** be wrapped
in a UseCase. Wrapping them would emit a domain event for every ingested
event/job, which is recursive and swamps the event log.

**Everything else goes through UseCase with domain events + audit logs:**
- All control plane CRUD: Event Types, Subscriptions, Connections, Dispatch Pools, Clients, Principals, Roles, Applications, Service Accounts, Identity Providers, Email Domain Mappings, CORS Origins, Auth Configs
- Human-initiated dispatch job actions: resend, ignore, cancel
- Sync operations (emit a summary event, e.g., `EventTypesSynced`)
- Consumer app operations via SDK (e.g., `ShipOrder`, `CancelOrder`)

### Events vs Audit Logs
Both are generated from the same `UnitOfWork.commit()` call. They are two views of the same fact:
- **Domain Events** — "what happened", consumed by other systems (subscriptions, webhooks). Can be purged after delivery/TTL.
- **Audit Logs** — "who did what, when", consumed by humans (admin UI, compliance). Retained long-term.

All UseCase operations emit both. The UnitOfWork handles this automatically.

### Reads Are Fine in Handlers
Read operations (list, get, filter) can call repositories directly from handlers.
Only writes need the use case layer.

## Layering Rules

The platform has four layers. Code in each layer may only depend on layers
below it. Crossing layers is a bug, even when it compiles.

| Layer | Lives in | Knows about | Does NOT know about |
|---|---|---|---|
| **Handler** (HTTP/route) | `*/api.rs`, `shared/*_api.rs` | HTTP types, DTOs, permission checks | SQL, transactions, database types |
| **Use Case** | `*/operations/*.rs` | Domain entities, repositories (as traits/readers), `UnitOfWork`, domain events | HTTP, SQL strings, transaction types |
| **Domain** | `*/entity.rs`, `*/operations/events.rs` | Plain data, domain invariants, factory/behavior methods | `sqlx`, `Postgres`, `Transaction<'_, _>`, any DB driver |
| **Repository** | `*/repository.rs` | SQL, sqlx types, row structs, transaction handles | HTTP, permissions, domain events |

### Aggregates Don't Persist Themselves

Domain entities (`Principal`, `Client`, `EventType`, …) are pure data + domain
behavior. They **must not**:
- Import `sqlx`, `Postgres`, `Transaction<'_, _>`, or any driver-specific type.
- Contain SQL strings in method bodies.
- Implement a persistence trait that takes a transaction handle.

If you catch yourself writing `impl Persist for Principal`, stop. The correct
shape is `impl Persist<Principal> for PrincipalRepository` — the **repository**
persists the **aggregate**. The aggregate is the thing being written, not the
writer.

This is why the TS version reads cleaner than Rust on the same operation:
TS puts `insert/update/delete` on `PrincipalRepository` and nowhere else;
earlier Rust ports collapsed this into `impl PgPersist for Principal` because
it reduced generic bounds — at the cost of leaking the transaction type into
the domain layer and creating two competing write paths.

### One Write Path Per Aggregate

Every aggregate has exactly one place its rows are written: its repository's
`persist` and `delete` methods. No handler, use case, or service writes to
that aggregate's tables directly. If you need to write to `iam_principals`,
you go through `PrincipalRepository`. Full stop.

The one exception is **platform infrastructure processing** (stream
projections, dispatch lifecycle, outbox polling, `createEvent` /
`createDispatchJob` ingest) — these write directly to `msg_events` /
`msg_dispatch_jobs` / `outbox_messages` without aggregates or use cases.
Those tables don't have aggregates in the DDD sense; they're message
queues and audit streams.

### Transactions Stay in the Persistence Layer

Use cases call `unit_of_work.commit(&aggregate, &*self.repo, event, &command)`
— passing the repository by reference. They never see a `Transaction<'_, _>`
type. The `UnitOfWork` opens the transaction, calls the repository's persist
method, writes the domain event + audit log, commits. If a use case signature
or body mentions a transaction type, something has leaked upward.

The transaction handle is wrapped in a `DbTx<'_>` newtype so that swapping
the underlying driver (or adding a second dev-only backend) only touches the
newtype and its consumers — not every repository method signature.

### Where New Code Goes

Adding a new aggregate? You create, in order:
1. `src/<domain>/entity.rs` — pure Rust structs, no sqlx.
2. `src/<domain>/repository.rs` — `struct <Aggregate>Repository`, row types, all SQL, and `impl Persist<Aggregate> for <Aggregate>Repository`.
3. `src/<domain>/operations/*.rs` — one file per use case. Call `unit_of_work.commit(...)` at the tail.
4. `src/<domain>/api.rs` — HTTP handlers. Permission checks, build Command, call `use_case.run(...)`.

If you find yourself adding SQL anywhere other than `repository.rs` (or one
of the three infrastructure-processing files), you are in the wrong file.

## Permission Check Naming Convention

Authorization checks live in `shared::authorization_service::checks`. The following naming convention applies:

### Existing Functions (do not rename)

| Function | Purpose | HTTP Methods |
|---|---|---|
| `require_anchor(ctx)` | Anchor-only endpoints | Any |
| `is_admin(ctx)` | Requires anchor scope or `ADMIN_ALL` permission | Any |
| `can_read_events(ctx)` | Read events | GET |
| `can_read_events_raw(ctx)` | Read event payloads | GET |
| `can_read_event_types(ctx)` | Read event types | GET |
| `can_create_event_types(ctx)` | Create event types | POST |
| `can_update_event_types(ctx)` | Update event types | PUT/PATCH |
| `can_delete_event_types(ctx)` | Delete event types | DELETE |
| `can_write_event_types(ctx)` | Any write on event types (create/update/delete) | POST/PUT/DELETE |
| `can_read_subscriptions(ctx)` | Read subscriptions | GET |
| `can_create_subscriptions(ctx)` | Create subscriptions | POST |
| `can_update_subscriptions(ctx)` | Update subscriptions | PUT/PATCH |
| `can_delete_subscriptions(ctx)` | Delete subscriptions | DELETE |
| `can_write_subscriptions(ctx)` | Any write on subscriptions | POST/PUT/DELETE |
| `can_read_dispatch_jobs(ctx)` | Read dispatch jobs | GET |
| `can_read_dispatch_jobs_raw(ctx)` | Read dispatch job payloads | GET |
| `can_create_dispatch_jobs(ctx)` | Create dispatch jobs | POST |
| `can_retry_dispatch_jobs(ctx)` | Retry dispatch jobs | POST |
| `can_write_dispatch_jobs(ctx)` | Batch write dispatch jobs | POST |
| `can_write_events(ctx)` | Create/batch events | POST |

### Convention for New Check Functions

- **`can_read_<resource>(ctx)`** — for GET endpoints (list, get by id, filters)
- **`can_read_<resource>_raw(ctx)`** — for GET endpoints that expose sensitive payloads
- **`can_create_<resource>(ctx)`** — for POST endpoints that create a single entity
- **`can_update_<resource>(ctx)`** — for PUT/PATCH endpoints
- **`can_delete_<resource>(ctx)`** — for DELETE endpoints
- **`can_write_<resource>(ctx)`** — for endpoints that accept any write (create, update, or delete); checks if the caller has *any* of the three granular permissions
- **`require_anchor(ctx)`** — for anchor-only endpoints (platform settings, identity providers, etc.)
- **`is_admin(ctx)`** — for endpoints requiring full admin access

### Service-Level Methods on `AuthorizationService`

The `AuthorizationService` struct also provides general-purpose methods:
- `authorize(ctx, permission, client_id)` — check a single permission + optional client access
- `require_anchor(ctx)` — require anchor scope
- `require_permission(ctx, permission)` — require a specific permission string
- `require_client_access(ctx, client_id)` — require access to a specific client

## Frontend UI Conventions

The SPA in `frontend/` **is the Go platform's production SPA**, taken
verbatim from `flowcatalyst-go` (see `frontend/PROVENANCE.md` for the
source commit and every change made on top of it). Rust is a drop-in
replacement for Go, so the SPA calls this platform exactly as it calls Go.
Match Go's idiom when adding UI; keep Rust-only additions (functions,
anchor-tier gating, …) listed in `PROVENANCE.md`.

**No Tailwind in the Vue SPA.** It isn't installed there: utility classes
(`grid grid-cols-N`, `flex justify-between`, `mb-4`, …) silently no-op.
Use PrimeVue components, the global classes in
`frontend/src/styles/main.css`, and scoped CSS. (This is about the Vue app
only. In the server-rendered `fc-web` crate at `/ui/*`, Tailwind and
Topcoat UI's components are the intended tools; see its section below.)

**List page + drawer.** Mirror Go's list pages (`ConnectionListPage.vue`,
`UserListPage.vue`, `SubscriptionListPage.vue`):

- **Layout primitives** (global, `styles/main.css`): `page-container` as
  the root, a `page-header` with `page-title` / `page-subtitle` and the
  primary action button, `fc-card` around the table.
- **Table toolbar**: `FcTableToolbar` in the `DataTable`'s `#header` slot —
  quick search (`v-model:search`), a **Filters** popover (`#filters` slot;
  each control wrapped in `FcFormField`, dropdowns `appendTo="self"`),
  Clear All, optional refresh; a `#start` slot for an always-visible
  selector. Filter state is `useListState` (URL-synced); `useTableFilters`
  derives the popover badge / Clear All and, for client-side tables, the
  `:filters` meta.
- **Rows open a drawer**: `:rowClass="() => 'clickable-row'"` and
  `@row-click` push the child route with `query: route.query`. Detail and
  create views are **child routes of the list** (`/connections/new`,
  `/connections/:id`) rendered in the list's
  `<RouterView v-slot="{ Component }"><component :is="Component" @changed="load" /></RouterView>`
  outlet, so the list stays visible and clickable underneath.
- **Drawers**: `components/drawer/EntityDrawer.vue` (non-modal right panel;
  `size` default / `wide` / `two-thirds`; `loading`, `error`, `dirty`;
  `#header-extra` for status tags, `#footer` for actions; `close(force)`)
  plus `useDrawerRoute({ listPath, paramKey, dirty })` (`id`, `goToList`,
  `replaceToDetail` for the create → detail hand-off, and the
  discard-changes leave guard). Edit forms track changes with
  `useDirtyForm`. Detail drawers `watch` the route param — the instance is
  reused when another row is clicked — and emit `changed` after writes.
- **Forms**: `FcFormSection` (`flat` inside drawers; `#actions` slot),
  `FcFormField` (label / `required` / `help` / `error` / `span`; its default
  slot passes the input `id`), `.fc-form-grid` for two-column forms,
  `FcDetailField` in a `.fc-detail-grid` for read-only values,
  `FcFormActions` (`:bordered="false"` in a drawer footer).
- **Full pages** only where a drawer is too small: editors and multi-tab
  views (`RoleEditPage`, `ClientLoginThemePage`, `ProcessCreatePage`,
  `FunctionDetailPage`, the manifest editor).
- **Components — PrimeVue v4**, auto-imported by `unplugin-vue-components`
  (`Select`, not `Dropdown`); `src/components/**` is auto-registered too.
- **Pagination**: client-side `paginator` for small lists; `lazy
  paginator` for offset-paginated server lists; cursor lists use
  `useCursorPagination` (audit log, login attempts). **High-volume
  firehose tables** (events, dispatch jobs, debug grids) take a result
  `size` only, no paging.

**Navigation and access** (owner decision #8): `config/navigation.ts`
holds the sidebar groups (`scope: "anchor" | "client"` splits audiences);
`stores/permissions.ts` maps routes to permission codes
(`ROUTE_PERMISSIONS`, any-of lists allowed; detail routes inherit their
list's entry) and anchor-only pages (`ANCHOR_ROUTES`). `canAccessPath` is
the one rule for the route guard, the sidebar and the post-login landing
page; a user with no role reaches only `/profile`. Use the catalogue's
permission codes (`role/entity.rs`), and gate in-page actions with
`userHasPermission(authStore.user, code)`. The server enforces every call
regardless.

**Rule of thumb for a new page**: copy the closest Go list page and its
drawers, keep the `<template>` + `<style scoped>` skeleton, and fill in
the resource-specific bits.

## fc-web (Topcoat UI trial)

`crates/fc-web` is a server-rendered admin UI built with Topcoat, mounted in
`fc-dev --features web` at `/ui/*` in front of the Vue SPA. Read
`docs/topcoat-trial.md` first. It is **excluded from the Cargo workspace**
(only the `web` feature builds it), so `--workspace` commands skip it; test
it with `CARGO_TARGET_DIR=target cargo test --manifest-path
crates/fc-web/Cargo.toml`.

- **Every page, route, shard and procedure has an explicit path under
  `/ui/(app)`.** That puts it inside the authentication `#[layer]`. A shard
  or procedure without a path gets `/_topcoat/runtime/<hash>`, outside the
  layer, and a shard endpoint runs without its page's checks.
- **Every handler calls `auth(cx)?` and `permit(checks::…)?`**, the same
  `checks` functions the axum handlers use. `tests/auth_convention_test.rs`
  enforces both. Public entry points go on its allowlist with a reason.
- **Writes are form POSTs to `#[route]`s that run the use case** (the same
  one the BFF handler runs), then `see_other` back with `set_flash`. Never
  write through a repository from fc-web; the UoW rules above apply
  unchanged.
- **Look = the SPA in `frontend/`** (Go's production UI): list pages with
  rows that open a right-hand drawer. Before adding a page, open the
  matching `.vue` list page and its drawer and copy their values (columns,
  labels, tags, empty states).
- **Build with Topcoat UI's components and Tailwind.** The no-Tailwind rule
  is the Vue SPA's only. Components live in `src/components/` (installed
  with `topcoat ui add <name>` from `crates/fc-web`, tracked in
  `components.toml`) and are ours to edit; `styles.css` themes them toward
  the SPA (Nora tokens on `:root`, the radius and type scale in the
  `.tc-theme` scope a Topcoat-built page's root carries).
  `docs/topcoat-components.md` has the catalogue, what each SPA pattern
  maps to, the local edits, and the gaps that are still hand-built.
  `app/users.rs` is the worked example. The older sections still use the
  hand-built `.fc-*` kit (`src/ui.rs`, `styles.css`) until they migrate.
  - Gate pages and nav entries with the permission the API handler checks
    (and `frontend/src/stores/permissions.ts` uses), never less.
- **Run the API's handler bodies, not a copy.** When an API handler holds
  the rules (reach, ceilings, orchestration), extract its body into
  fc-platform (e.g. `principal::admin`) and call that from both.
- **Pages work as plain HTML first.** Filters are GET forms and actions are
  POST forms.
  - Modals are native `<dialog>` (Topcoat's `dialog` / `alert_dialog`
    with `open: false`) opened with `commandfor`/`command="show-modal"`,
    so the browser gives the focus trap and Escape.
  - Menus are `popover` or Topcoat's `dropdown_menu`; collapsibles are
    `<details>`.
  - Signals and shards are only for in-place updates, such as the detail
    drawer, that HTML can't do.
- **Assets bundle themselves** on the first start after a build
  (`src/assets.rs`); after changing Tailwind classes, rebuild and restart.

## Frontend API Response Handling

API modules live in `frontend/src/api/`, one per resource, over the
hand-rolled transport in `api/client.ts`: `apiFetch` (`/api`), `bffFetch`
(`/bff`), `authFetch` (`/auth`, errors stay inline: no global toast, no
401/403 modal). `client.ts` decodes the platform error envelope once
(`{ error: CODE, message, details? }`): it toasts non-401 failures
(opt out with `suppressGlobalErrorToast`), raises the session-expired /
permission-denied modal on 401/403, and throws `ApiError` (`status`,
`code`, `details`, a `message` with per-field validation errors appended).

Request/response types **alias the generated contract** in
`src/api/generated/types.gen.ts`, generated (types only) from
`frontend/openapi/openapi.json` — a copy of Go's OpenAPI lockfile, the
contract this platform converges to — so `vue-tsc` fails when a wrapper
drifts. Regenerate with `cd frontend && pnpm api:generate`; don't overwrite
that file with this platform's own `/q/openapi`. The function API has its
own document (`api/generated-functions/`).

Most of our PUT/PATCH update handlers return **`204 No Content`** — no body.
`apiFetch` resolves to `undefined` for 204 responses. That means:

```ts
// ❌ wrong — `thing.value` becomes undefined, every `v-if="thing"` flips
// false, and the drawer shows "not found".
thing.value = await thingsApi.update(id, ...);
```

```ts
// ✅ right — call the void method, then refetch from the source of truth.
await thingsApi.update(id, ...);
await loadThing(id);
emit("changed"); // drawers: let the list behind reload
```

**Convention checklist when adding/modifying an FE API wrapper:**

- If the backend handler returns `NO_CONTENT`, the FE wrapper MUST be typed
  `Promise<void>`. Don't declare it `Promise<Entity>` and let the type lie —
  the bug is invisible until users see "not found" after a successful save.
- After calling a void API method, **refetch** with `await loadX(id)`
  (or whichever loader the drawer/page already has). Don't assign the
  call's result to a reactive ref.
- If the backend should return the updated entity, it must do so for Go
  too (the SPA's contract is Go's); mismatched declarations are the bug.

The convention is enforced by
`frontend/tests/conventions/no-void-api-assignment.test.ts`. It scans
`src/pages` and `src/components` for `ref.value = await xxxApi.method(...)`
where `method` is declared `Promise<void>` and fails with the exact
file:line. Run the frontend tests with `pnpm test` from `frontend/`
(vitest; component tests mount with `@vue/test-utils` under a per-file
`// @vitest-environment jsdom`), the type check with `pnpm build`, lint
with `pnpm lint`.

For genuinely intentional uses (rare), add a trailing
`// fc-api-void: ok` comment on the line to opt out.

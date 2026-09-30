# Platform Control Plane

The platform is the brain. It owns every aggregate (clients, principals, roles, event types, subscriptions, connections, dispatch pools, scheduled jobs, applications, identity providers, …), exposes them through two HTTP tiers (`/bff/*` for the UI, `/api/*` for SDKs), and gates every mutation through the same UseCase / UnitOfWork pipeline that emits domain events and audit logs atomically with each write. Source: `crates/fc-platform/`, binary `bin/fc-server/` (`FC_PLATFORM_ENABLED`, on by default).

This document is for engineers working in `fc-platform`. For ops-level concerns (deploy, secrets, IDP setup) see [operations/](../operations/). For developer-facing usage (publishing events, configuring subscriptions) see [developers/](../developers/).

---

## Layering

Four layers, strict downward dependency only. CLAUDE.md states this rule and `tests/it/uow_convention_test.rs` enforces a subset of it at test time.

| Layer | Lives in | Knows about | Does not import |
|---|---|---|---|
| Handler | `*/api.rs`, `shared/*_api.rs` | HTTP, DTOs, permission checks | SQL, transactions, sqlx |
| UseCase | `*/operations/*.rs` | Domain entities, repositories, `UnitOfWork`, domain events | HTTP, SQL strings |
| Domain | `*/entity.rs`, `*/operations/events.rs` | Plain data, invariants, factory/behaviour methods | `sqlx`, `Postgres`, `Transaction<'_, _>` |
| Repository | `*/repository.rs` | SQL, sqlx, row structs, transaction handles | HTTP, permissions, domain events |

The most-violated boundary in early Rust ports was "aggregates persist themselves" (`impl Persist for Principal`). That collapsed the layering for the sake of generic-bound brevity at the cost of leaking transaction types into the domain. Current shape: `impl Persist<Principal> for PrincipalRepository` — the **repository persists the aggregate**. The aggregate is the thing being written, not the writer.

---

## The platform crates

The platform is seven crates (docs/plans/build-speed-2026-09-28.md, section
12). Each keeps its modules at their historical paths, and `fc-platform`
re-exports every one at `fc_platform::<module>`, so the binaries, fc-web and
the tests name them as before.

| Crate | Holds | Depends on |
|---|---|---|
| `fc-platform-core` | `usecase` (the UoW seal), the kernel half of `shared` (errors, ids, the authorization context and `checks`, the request extractors and `AuthLayer`, the pool and migration runner, encryption, email, rate limiting), the permission catalogue (`permissions`), the principal kinds, the IAM lookup traits (`directory`) | – |
| `fc-platform-iam` | tenancy, identity and access: `client`, `application`, `principal`, `role`, `service_account`, `identity_provider`, `email_domain_mapping`, `platform_config`, `cors`, `audit`, …; the IAM half of `auth` (OAuth clients, auth configs, `auth_service`, password hashing, the reset emailer), `mfa` (2FA state) and `portal` (identities, apps) | core |
| `fc-platform-auth` | the sign-in flows: `auth` (token endpoint, OIDC login, sessions, refresh tokens), `mfa` (login, self-service), `webauthn`, `portal` (login plane), client selection, `/api/me` | core, iam |
| `fc-platform-messaging` | `event_type`, `event`, `subscription`, `connection`, `dispatch_pool`, `dispatch_job`, the scheduler, `process`, the ingest endpoints | core |
| `fc-platform-scheduled-jobs` | `scheduled_job` | core |
| `fc-platform-functions` | `function` | core, messaging, scheduled-jobs |
| `fc-platform` | the assembly: every aggregate's `routes.rs`, `router.rs`, the `PlatformContext` and server setup, the OpenAPI documents, the startup seeding, the cross-aggregate endpoints in `shared` | all of them |

Messaging, the scheduled jobs and the functions read IAM facts (a client's
name, an application's code, signing accounts, outbound credentials, the
application scope) through the traits in `fc_platform_core::directory`, which
iam implements and the assembly injects; so an IAM edit rebuilds only auth and
the assembly.

## Aggregate-per-directory

Every aggregate lives in its own directory under its crate's `src/` (the table above). The layout is intentionally repetitive — a new aggregate is mechanical to add:

```
crates/fc-platform-<domain>/src/<aggregate>/
├── entity.rs         # the domain struct + factory/mutator methods
├── repository.rs     # sqlx row struct, persist/find/delete, impl Persist<Entity>
├── api.rs            # axum handlers; thin adapters
└── operations/
    ├── mod.rs
    ├── events.rs     # DomainEvent impls
    ├── create_*.rs   # one file per UseCase
    ├── update_*.rs
    └── delete_*.rs
crates/fc-platform/src/<aggregate>/
├── mod.rs            # `pub use fc_platform_<domain>::<aggregate>::*;` + `pub mod routes;`
└── routes.rs         # routes(ctx): builds the states from the PlatformContext
```

Current aggregates (mid-2026):

- **Tenancy & IAM**: `client`, `principal`, `role`, `application`, `service_account`, `identity_provider`, `email_domain_mapping`, `cors`, `anchor_domain` (under `client/`), `webauthn`, `password_reset`, `login_attempt`.
- **Messaging**: `event_type`, `subscription`, `connection`, `dispatch_pool`, `dispatch_job`, `event`, `scheduled_job`.
- **Platform**: `platform_config`, `application_openapi_spec`, `audit`.
- **Auth machinery**: `auth/` (oauth, oidc, password, JWKS).

Outside the aggregate dirs:

- `shared/` — services and helpers that span aggregates (database, authorization, encryption, email, projection, role sync). Also where the **shared API routers** live (BFF roles/dashboard/etc., SDK sync/batch APIs, monitoring, public/well-known, dispatch-process callback).
- `usecase/` — the UoW seal, command/error/result/context types.
- `scheduler/` — dispatch job scheduler (separate document: [scheduler.md](scheduler.md)).
- `service/` — historically things lived here; mostly migrated into aggregate dirs.
- `seed/` — dev/test data seeders.
- `router.rs` — top-level Axum router composition.

---

## UseCase + UnitOfWork

### The contract

Every write goes through a `UseCase` impl. Three steps:

```rust
#[async_trait]
pub trait UseCase<Cmd, Event: DomainEvent>: Send + Sync {
    async fn validate(&self, cmd: &Cmd, ctx: &ExecutionContext) -> Result<(), UseCaseError>;
    async fn authorize(&self, cmd: &Cmd, ctx: &ExecutionContext) -> Result<(), UseCaseError>;
    async fn execute(&self, cmd: Cmd, ctx: ExecutionContext) -> UseCaseResult<Event>;
}
```

A handler does this and only this:

```rust
async fn handler(State(state): State<...>, auth: AuthContext, Json(req): Json<CreateClientRequest>) -> ... {
    can_write_clients(&auth)?;                    // 1. permission gate
    let cmd = CreateClientCommand::from(req);     // 2. DTO → command
    let ctx = ExecutionContext::from_auth(&auth.0);
    let outcome = state.use_case.run(cmd, ctx).await.into_result()?;
    Json(ClientResponse::from(outcome.event))
}
```

`run()` calls `validate → authorize → execute` in order. The handler is a thin adapter; the business logic lives in the use case. CLAUDE.md is explicit about this — "Handlers (BFF, SDK, admin API) are thin adapters".

### The seal

`execute` returns `Result<Committed<Event>, UseCaseError>`, and `Committed::new` is `pub(in crate::usecase)`. The only constructors are inside `usecase/result.rs` and `usecase/unit_of_work.rs`. Concretely this means a use case **cannot** build a success result by hand:

```rust
// won't compile — Committed::new is module-private
return Ok(Committed::new(MyEvent::new(...)));
```

Everything before the commit is ordinary `Result` code — `?` on repository calls, `return Err(UseCaseError::...)` for rule violations. The only paths to `Ok(_)` for a write use case are:

- `unit_of_work.commit(aggregate, repo, event, command)` — write + emit event + audit.
- `unit_of_work.commit_delete(aggregate, repo, event, command)` — delete + emit event + audit.
- `unit_of_work.commit_all(aggregates, repo, event, command)` — batch write.
- `unit_of_work.emit_event(event, command)` — emit event only (login, sync summary).
- Any `.map(|c| c.map(|_| success_value))` chained onto one of the above.

This is **stronger than the TypeScript runtime token check** — it's compile-time-guaranteed. A use case that "forgets" to call UoW fails to compile, not at test time. The convention test in `tests/it/uow_convention_test.rs` adds a second guard: it parses every `execute` body and asserts it reaches a `unit_of_work.*` call.

### What UoW.commit does

```rust
async fn commit<A, E, C>(
    &self,
    aggregate: &A,
    repo: &dyn Persist<A>,
    event: E,
    cmd: &C,
) -> Result<Committed<E>, UseCaseError>
where A: HasId, E: DomainEvent, C: Serialize {
    let mut tx = self.pool.begin().await?;
    repo.persist(aggregate, &mut tx).await?;            // INSERT/UPDATE entity row(s)
    self.persist_event(&mut tx, &event).await?;         // INSERT into msg_events
    self.persist_audit(&mut tx, &event, cmd).await?;    // INSERT into aud_logs
    tx.commit().await?;
    Ok(Committed::new(event))
}
```

Three rows in one transaction. Either all land or none do. `msg_events` is the domain-event stream consumed by `fc-stream::event_fan_out` (which then writes `msg_dispatch_jobs`). `aud_logs` is the long-retention audit trail consumed by the admin UI.

### Infrastructure exceptions (CLAUDE.md)

A small set of write paths intentionally bypass UoW. They're the platform's own internal plumbing — wrapping them in UoW would emit recursive domain events. The exceptions list is fixed and enforced by the convention test:

- `POST /api/events/batch` — application-emitted events. The whole point is to ingest events; wrapping it would emit a meta-event per event.
- `POST /api/dispatch-jobs/batch` — direct dispatch-job ingest.
- Dispatch lifecycle (PENDING → QUEUED → PROCESSING → COMPLETED / FAILED) — infrastructure traffic.
- Outbox processing (`outbox_messages` table).
- OAuth/OIDC short-lived state (refresh tokens, auth codes, OIDC login state).
- Built-in role seeding at startup.
- Scheduled-job firings and instance log lines.

Login *outcomes* (`UserLoggedIn`, `UserLoggedOut`, `ScheduledJobFiredManually`) **do** go through UoW — those are audit-worthy actions. The bypass only covers the token-row plumbing or instance-row plumbing underneath.

### Aggregates don't persist themselves

```rust
// Wrong (what early Rust ports did):
impl Persist for Principal {                 // domain depends on Postgres
    async fn persist(&self, tx: &mut Transaction<'_, Postgres>) -> ... { ... }
}

// Right (current code):
impl Persist<Principal> for PrincipalRepository {
    async fn persist(&self, agg: &Principal, tx: &mut DbTx<'_>) -> ... { ... }
}
```

`DbTx<'_>` is a newtype around `Transaction<'_, Postgres>`. Wrapping it means swapping the driver (or adding a second dev backend) touches the newtype and its consumers, not every domain method. Use cases never see the `Transaction` type directly.

---

## HTTP tiers

Three URL prefixes, three contracts. CLAUDE.md states this and every handler honours it:

### `/bff/*` — frontend only

Cookie-session auth, response shapes tuned to specific screens. Callers outside the FlowCatalyst frontend should not depend on these endpoints — fields are added and removed without API-version discipline.

Composition: `BffRouter::build()` in `crates/fc-platform/src/router.rs` plus the per-screen routers in `shared/bff_*_api.rs`. Notable routes:

- `/bff/dashboard`, `/bff/events`, `/bff/dispatch-jobs`, `/bff/filter-options`
- `/bff/roles`, `/bff/event-types`, `/bff/scheduled-jobs`, `/bff/developer`
- `/bff/debug/events`, `/bff/debug/dispatch-jobs` — high-volume firehose lists, no pagination (see [feedback_high_volume_no_paging](https://...) — `?size=` only).

### `/api/*` — programmable

Bearer-token auth. Stable contract. Authorisation is enforced by **permissions** (`can_read_events`, `can_write_subscriptions`, etc.), not by URL tier — there is no `/api/admin/*` anymore. A missing `can_*` call on a write handler is a privilege-escalation bug.

Naming convention for permission checks (`shared/authorization_service.rs::checks`):

| Function | When | HTTP |
|---|---|---|
| `require_anchor(ctx)` | Anchor-only endpoints | any |
| `is_admin(ctx)` | Anchor or `ADMIN_ALL` permission | any |
| `can_read_<resource>(ctx)` | Read | GET |
| `can_read_<resource>_raw(ctx)` | Read sensitive payload | GET |
| `can_create_<resource>(ctx)` | Create one | POST |
| `can_update_<resource>(ctx)` | Update | PUT/PATCH |
| `can_delete_<resource>(ctx)` | Delete | DELETE |
| `can_write_<resource>(ctx)` | Any write (batch endpoints) | POST/PUT/DELETE |

The `can_write_*` form exists for endpoints that accept create/update/delete in one batch. It checks "any of the granular permissions".

### Platform-owned, never moved

`/auth/*`, `/oauth/*`, `/.well-known/*`, `/api/dispatch/*`, `/api/monitoring/*`, `/api/me/*`, `/api/public/*` are platform infrastructure. They don't follow the BFF/API split — they exist outside both tiers.

### Router assembly

`crates/fc-platform/src/router.rs::build(&PlatformContext)` composes the final Axum router. Each route module (`client`, `principal`, …, `shared`) exposes `routes(ctx) -> AggregateRoutes` in its `routes.rs`: it builds its own state from the `PlatformContext` (built once per process) and returns its documented and plain routes at their full paths, with any per-group layers (rate limits, error mapping) applied. `router.rs` lists the modules, then adds the cross-cutting pieces:

1. **Documented routes** — every module's `utoipa-axum` routes, merged in module order so they appear in the generated spec.
2. **Plain routes** — the developer portal (which needs the finished spec), then every module's plain routes.
3. **Auth middleware layer** — `AuthLayer` extracts bearer tokens or session cookies, populates `AuthContext` for downstream handlers.
4. **CORS layer** — driven by `cors_origins_cache` populated from `tnt_cors_allowed_origins` (refreshed every 60 s).
5. **SPA fallback** — `/assets/*` with `Cache-Control: public, max-age=31536000, immutable`; unmatched GET routes fall through to embedded `index.html`. Toggled per binary: `fc-server` serves the SPA from `FC_STATIC_DIR` (default `/app/frontend/dist`), `fc-dev` embeds it or uses `FC_STATIC_DIR`.

---

## AuthorizationService

`shared/authorization_service.rs`. Two responsibilities:

1. **Build context.** Given the JWT claims off the token, produce an `AuthContext` with the principal, their effective permissions, accessible clients, and scope (Anchor / Partner / Client).
2. **Check permissions.** Service-level helpers (`authorize`, `require_anchor`, `require_permission`, `require_client_access`) and per-resource convenience wrappers in the `checks` submodule.

### Permission resolution + caching

A principal's effective permission set is the union of permissions granted to each of their roles (`iam_role_permissions` junction). Computing it on every request would mean two DB hits per request. Instead:

- **Token validation cache** (DashMap, 30 s TTL): caches `(token_hash) → AccessTokenClaims`. Saves repeated RSA verification.
- **Permission resolution cache** (DashMap, 60 s TTL): caches `(principal_id, role_set_hash) → Vec<Permission>`. Saves repeated role-to-permission lookups.

Both caches invalidate naturally on TTL expiry. There's no explicit invalidation on role mutation — a 60 s lag for a role change to propagate to live tokens is acceptable (and any operator who needs immediate effect can ask the principal to log out and back in).

### Permission pattern

Permissions are colon-separated 4-tuples: `application:resource:entity:action`. Wildcards (`*`) at any position match anything in that position.

Examples (granted to the built-in `viewer` role for the `platform` application):

```
platform:event:*:read
platform:subscription:*:read
platform:dispatch_job:*:read
```

A check `can_read_events(ctx)` resolves to:

```rust
ctx.has_permission_with_pattern("platform:event:*:read")
```

which scans the principal's permission set for any permission whose pattern matches.

### Multi-tenancy: scopes and clients

Three principal scopes:

| Scope | Meaning | `clients` JWT claim |
|---|---|---|
| Anchor | Platform admin | `["*"]` |
| Partner | Integration partner serving multiple clients | `["clt_a:acme", "clt_b:globex"]` |
| Client | End user of one tenant | `["clt_a:acme"]` |

`require_client_access(ctx, "clt_a")` does the scope check: anchors pass, partners pass if `clt_a` is in their assignment list, clients pass only for their own client ID. Most write handlers do this check after the permission check, to enforce "you have permission to update subscriptions in principle, but not subscriptions belonging to a client you can't see".

---

## Authentication flows

Three independent paths into the platform, three different token sources, one common JWT output. Details in [auth-and-oidc.md](auth-and-oidc.md); brief inventory here:

1. **Local password.** For clients without an IDP. `argon2id` hash, `iam_login_attempts` table for backoff. `auth/auth_api.rs`.
2. **OIDC bridge.** For clients with an IDP (Entra, Keycloak, Google Workspace). Validates the IDP's ID token against its JWKS, resolves or creates the principal, issues a FlowCatalyst session JWT. `auth/oidc_login_api.rs`, `auth/jwks_cache.rs`.
3. **Service account.** OAuth `client_credentials` grant. `auth/oauth_api.rs`.

All three converge on `AuthService::generate_access_token`, which signs an RS256 JWT with FlowCatalyst's RSA private key. The same key validates incoming tokens at the middleware layer; key rotation is supported via `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS`.

OIDC client secrets (stored in `oauth_clients`) are encrypted at rest with `FLOWCATALYST_APP_KEY` (AES-256-GCM via the `EncryptionService`). The plaintext is returned exactly once at creation and on explicit regeneration; subsequent reads return only the cipher.

---

## Database access rules

CLAUDE.md states the rules; enforcing them matters because the alternative path is silent N+1 query proliferation.

### Banned: query in a loop

```rust
// Never:
for item in items {
    item.children = load_children(&item.id).await?;
}
```

### Required: batch with `IN` / `ANY($1)` / `UNNEST($1, $2, ...)`

```rust
let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
let all_children = sqlx::query_as::<_, ChildRow>(
    "SELECT * FROM children WHERE parent_id = ANY($1)"
).bind(&ids).fetch_all(&pool).await?;
// group in memory
```

### Concurrent independent queries: `tokio::try_join!`

```rust
let (clients, events, pools) = tokio::try_join!(
    repo.find_clients(),
    repo.find_events(),
    repo.find_pools(),
)?;
```

### `fetch_optional` over `fetch_one`

`fetch_one` panics at runtime if no rows return. The only safe uses are aggregate queries guaranteed to return one row: `SELECT COUNT(*)`, `SELECT MAX(...)`, `SELECT EXISTS(...)`. Everything else uses `fetch_optional` and matches `Some(_)` / `None`.

### Shallow queries for list endpoints

A dropdown that only needs `(id, name)` does not load the whole junction tree. The `*_shallow()` repository methods exist for exactly this.

---

## SQLx migration status

The platform was originally on SeaORM. Migration to handwritten SQLx is ongoing:

- **Phase 1 (done):** 15 repos migrated. All read paths use `sqlx::query_as` with explicit row structs.
- **Phase 2-5 (in progress):** remaining repos, then the auth/session layer, then the scheduler module.

New repositories MUST use SQLx. Pattern:

```rust
#[derive(sqlx::FromRow)]
struct PrincipalRow {
    id: String,
    email: String,
    // ...
}

pub struct PrincipalRepository { pool: PgPool }

impl PrincipalRepository {
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Principal>> {
        let row = sqlx::query_as::<_, PrincipalRow>("SELECT ... WHERE id = $1")
            .bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(Principal::from))
    }
}
```

Row mapping stays in `repository.rs`. Domain entities stay in `entity.rs`. The two never import each other directly — `From` impls in `repository.rs` bridge them.

---

## Caching, beyond auth

The platform keeps a few in-memory caches for hot reads:

| Cache | TTL | Owned by | Purpose |
|---|---|---|---|
| Validated JWT claims | 30 s | `AuthService` | Skip RSA verify on repeat requests |
| Principal→permissions | 60 s | `AuthorizationService` | Skip role join on repeat requests |
| Paused connections | 60 s | `scheduler::PausedConnectionCache` | Filter `PENDING` jobs cheaply |
| CORS origins | 60 s | `fc-server` top-level | CORS predicate function |
| Subscriptions | 5 s | `fc-stream::EventFanOutService` | Fan-out matching |

All TTL-based; no explicit invalidation. The trade-off accepted: changes take up to 60 s to propagate (5 s for subscriptions, which see the most frequent admin changes).

---

## Migrations

`migrations/` directory at the repo root. Numbered sequentially, applied at startup by `shared/database.rs::run_migrations`. Three rules:

1. **Never edit a shipped migration.** The runner stores a sha256 of each migration's content in `_schema_migrations.checksum`. Editing flags a warning at next deploy (the new SQL is **not** executed — drift, not refusal). For schema changes, write a new migration: `NNN_alter_<table>_<change>.sql`.
2. **Partitioned-table UNIQUE constraints must include `created_at`.** Postgres rejects partition-key-less UNIQUEs on partitioned tables. See [partitioning.md](partitioning.md).
3. **Migrations are idempotent.** Use `CREATE TABLE IF NOT EXISTS`, `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`, and the guarded-by-`pg_partitioned_table` early-return pattern for partition setups.

Current migrations: `001_tenant_tables.sql` through `025_application_openapi_specs.sql`. Migration 023 was retired (numbering doesn't reuse).

---

## Built-in role seeding

`shared/database.rs::seed_builtin_roles` runs at every startup. The catalogue is defined **once** in `role/entity.rs::roles()` — there are no parallel constants in other modules. The seeder upserts roles and permission grants per the catalogue, never touches roles whose source is `Database` (operator-created) or `Sdk` (synced from an application).

The same seeder runs `seed_platform_application` immediately after, ensuring an `app_applications` row with code `platform` exists. The platform's own permissions hang off this row.

This is one of the few infrastructure-write paths exempt from UoW — it runs at boot before any user is present, so there's no executing principal to attribute the events to.

---

## Code references

- Top-level router: `crates/fc-platform/src/router.rs::build`; the modules' `*/routes.rs`; `shared/platform_context.rs`.
- Use case framework: `crates/fc-platform-core/src/usecase/{mod,result,context,error}.rs`.
- UoW: `crates/fc-platform-core/src/usecase/unit_of_work.rs::PgUnitOfWork`.
- UoW seal: search for `pub(in crate::usecase) fn success` in `usecase/result.rs`.
- UoW convention test: `crates/fc-platform/tests/it/uow_convention_test.rs`.
- Authz service: `crates/fc-platform-iam/src/shared/authorization_service.rs` (the context and the checks: `crates/fc-platform-core/src/shared/authorization_service.rs`).
- Auth middleware: `crates/fc-platform-core/src/shared/middleware.rs` (`AuthLayer`; the platform's `AppState` is fc-platform-iam's).
- DB pool + migrations + secret refresh + role seeding: `crates/fc-platform/src/shared/database.rs`.
- TSID generation: `crates/fc-common-types/src/tsid.rs` (`tsid::generate`/`generate_with_prefix`/`generate_untyped`), re-exported as `fc_common::tsid` and via `crates/fc-platform-core/src/shared/tsid.rs`.

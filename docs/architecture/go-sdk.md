# Go SDK — Design

Design document for the fourth FlowCatalyst SDK, alongside `crates/fc-sdk`
(Rust), `clients/typescript-sdk`, and `clients/laravel-sdk`. Lives at
`clients/go-sdk/`. Module path:
`github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk`.

Status: Phases 1 + 2 + 3 + client-parity rewrite landed. Phase 1 —
`usecase`, `usecasepgx`, `usecasesql`, `outboxpgx`, `outboxsql`,
`internal/sealed`. Phase 2 — `tsid` (35 EntityType prefixes,
byte-identical to Rust), `webhook` (HMAC-SHA256 validator), `client`
(platform HTTP client + 7 resource families: event types,
subscriptions, dispatch pools, applications, processes, principals,
roles), `sync` (declarative DefinitionSet + Synchronizer for
one-call-per-category reconciliation). Phase 3 — `auth`
(AccessTokenClaims/AuthContext, RS256 TokenValidator via
`lestrrat-go/jwx/v2` JWKS auto-discovery + cache, HS256
HmacTokenValidator, OAuth2 OAuthClient with PKCE auth code / refresh /
revoke / introspect / userinfo / RP-initiated logout,
ClientCredentialsProvider satisfying `client.TokenProvider`).
Client-parity rewrite — applications/principals/roles DTOs aligned
byte-for-byte to Rust SDK; deferred methods added
(ProvisionServiceAccount / ListClients / UpdateClientConfig /
EnableForClient / DisableForClient / ListRoles on Applications;
Roles / AddRole / RemoveRole / SetRoles / ClientAccessGrants /
GrantClientAccess / RevokeClientAccess / ResetPassword on Principals;
GrantPermission / RevokePermission / ListForApplication on Roles);
8 new resource families ported — Permissions, AuditLogs, Clients
(tenants), Connections, Me, Router (router-base-URL monitoring),
ScheduledJobs (full CRUD + sync; consumer-side Runner still TODO),
OpenAPI. Seal verified by a sandbox compile test.

D / E / F — `cache` (pluggable byte-oriented cache with required TTL;
generic `Get[T]`/`Set[T]`/`GetOrSet[T]` JSON helpers; `MemoryCache` in
the root package; `cache/postgrescache` (pgx) and `cache/rediscache`
(go-redis/v9) as opt-in sub-packages so the SDK module itself doesn't
pull driver deps for callers that don't need them). `lock`
(distributed-lock contract; `NoOp` + `Memory` in the root package;
`lock/postgreslock` (table-based with WHERE-on-upsert, UUID holder
tokens) and `lock/redislock` (SET NX PX + Lua check-and-delete) as
opt-in sub-packages). `scheduledjobs` (consumer-side runner — register
`HandlerFunc`s by job code, runner serialises via a `lock.Provider`,
streams log lines via `POST /api/scheduled-jobs/instances/:id/log`,
reports completion via `POST /complete`; handles panic recovery and
oversize-result truncation; `Wait()` for graceful shutdown in tests).

All Go SDK phases (1+2+3+client-parity+D/E/F) now shipped. Future work:
the `flowcatalyst-go` server migration to consume `usecasepgx` (see
memory `project_go_sdk_location.md` "Migration story").

## Goals

1. Same domain-driven patterns as the other three SDKs:
   `DomainEvent` → `UseCase` (validate → authorize → execute) →
   `UnitOfWork.Commit(...)` → outbox row → fc-outbox-processor → platform.
2. Same wire format: TSID prefixes, event payload JSON shape, audit log
   shape, outbox column layout. A Go service and a TS service must produce
   byte-identical outbox rows for the same logical event.
3. Idiomatic Go: stdlib-first, `context.Context` first arg, errors as
   values, generics where they add clarity, no framework lock-in.
4. UnitOfWork seal stronger than the TS/Laravel symbol-token approach —
   compile-time enforced via `internal/`, matching the Rust SDK's
   `pub(in crate::usecase)` guarantee.

## Out of scope (v1)

- Framework middleware (gin/echo/chi/fiber). Core stays `http.Handler`-shaped.
  Add per-framework subpackages in v2 if there's demand.
- MongoDB-backed UnitOfWork. Mongo gets the simple outbox pattern only
  (`OutboxManager.CreateEvent(...)`); aggregate-level transactional
  semantics are a SQL-only feature.
- Effect-style (TS) or Eloquent-coupled (Laravel) abstractions. Go gets
  plain interfaces and generics.

## Module layout

```
clients/go-sdk/
  go.mod                          // module github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk
  README.md
  usecase/                        // DB-agnostic primitives
    domain_event.go               //   DomainEvent, EventMetadata, BaseDomainEvent
    execution_context.go          //   ExecutionContext + factories
    error.go                      //   UseCaseError (validation/not_found/business_rule/...)
    result.go                     //   UseCaseResult[E] — sealed via internal/sealed
    use_case.go                   //   UseCase[C,E] interface + Run[C,E] generic
  outbox/                         // Shared DTOs + driver interface
    manager.go                    //   OutboxManager — simple pattern (CreateEvent/AuditLog/DispatchJob)
    dto.go                        //   CreateEventDto, CreateAuditLogDto, CreateDispatchJobDto (builders)
    driver.go                     //   OutboxDriver interface
    schema.go                     //   CREATE TABLE SQL for PG + MySQL
    types.go                      //   OutboxMessage, MessageType, OutboxStatus
  outboxpgx/                      // pgx-backed driver + UnitOfWork
    driver.go                     //   implements outbox.OutboxDriver
    unit_of_work.go               //   UnitOfWork[pgx.Tx]
    tx_scoped.go                  //   TxScopedUnitOfWork[pgx.Tx] for Run()
  outboxsql/                      // database/sql-backed (PG + MySQL via stdlib)
    driver.go
    unit_of_work.go               //   UnitOfWork[*sql.Tx]
    tx_scoped.go
  outboxmongo/                    // MongoDB driver — outbox writes only, no UoW
    driver.go
  outboxmem/                      // In-memory UoW for tests
    unit_of_work.go               //   InMemoryUnitOfWork[Tx any]
  tsid/
    tsid.go                       //   13-char Crockford Base32, matches Rust/TS
    entity_type.go                //   30 typed prefixes — port of Rust EntityType
  client/                         // Platform HTTP client
    client.go                     //   FlowCatalystClient
    options.go                    //   WithToken, WithTimeout, WithRetry, WithHTTPClient
    eventtypes.go                 //   one file per aggregate, mirrors crates/fc-sdk/src/client/
    subscriptions.go
    dispatchpools.go
    principals.go
    roles.go
    processes.go
    scheduledjobs.go
    applications.go
    clients.go
    auditlogs.go
    me.go
    permissions.go
    connections.go
    router.go
    internal/openapi/             // oapi-codegen output — types only
  sync/                           // DefinitionSet + DefinitionSynchronizer
    synchronizer.go
    definition_set.go
    definitions.go
    options.go
    result.go
  webhook/
    validator.go                  //   HMAC-SHA256, constant-time compare
  auth/
    jwks.go                       //   JWKS cache (via jwx/v2)
    jwt.go                        //   token verification
    oauth.go                      //   client_credentials grant
  cache/
    cache.go                      //   Cache interface (TTL required on every write)
    memory.go
    postgres.go
    redis.go
  lock/
    lock.go                       //   LockProvider interface
    memory.go
    postgres.go
    redis.go
  scheduledjobs/
    runner.go                     //   ScheduledJobRunner (consumer-side)
  internal/
    sealed/
      token.go                    //   sealed.Token — only constructable from within internal/
  examples/
    list-event-types/main.go
    fc-sync/main.go
    scheduled-jobs-runner/main.go
```

### Why so many sibling packages instead of subpackages of `outbox/`?

Go's `internal/` rule is per-directory. We need three concrete UoW
implementations (`pgx`, `sql`, `mem`) to import `internal/sealed` to
construct `Success`. The cleanest way to scope `internal/sealed` is to
keep it under the module root (`clients/go-sdk/internal/sealed`) so all
sibling packages can use it, while everything outside the SDK module
cannot. Putting the UoWs under `outbox/pgx`, `outbox/sql`, `outbox/mem`
would also work — the choice is taste. The flat layout reads better in
import lines:

```go
import (
    "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/outbox"
    "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/outboxpgx"
    "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
)
```

vs.

```go
import (
    "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/outbox"
    "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/outbox/pgx"
    "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
)
```

The flat form makes the driver choice visible at the import site. Go
modules in the wild (e.g. `pgx/v5` with `pgx/v5/pgxpool` and
`pgx/v5/stdlib`) use the same pattern.

## Database coverage

| Concern                          | Postgres | MySQL | MongoDB |
|----------------------------------|----------|-------|---------|
| Outbox writer (`OutboxManager`)  | ✓ pgx    | ✓ sql | ✓ mongo |
| UnitOfWork (entity + event tx)   | ✓ pgx    | ✓ sql | —       |
| `UnitOfWork.Run` orchestration   | ✓ pgx    | ✓ sql | —       |

Rationale: the UoW seal guarantees domain events are emitted atomically
with state changes. That requires a real two-phase commit on the same
connection, which MongoDB does not provide across collections in a way
the SDK can rely on. Mongo users get the simple outbox pattern — same
shape as Laravel's `MongoDriver`.

## The UnitOfWork seal

The TS and Laravel SDKs use a runtime symbol/token to restrict
`Result.success(...)` construction. The Rust SDK upgrades that to a
compile-time guarantee via `pub(in crate::usecase)`. Go can match the
Rust strength via the standard `internal/` rule.

```go
// clients/go-sdk/internal/sealed/token.go
package sealed

type Token struct{} // unexported field; cannot be constructed outside this package

func Internal() Token { return Token{} }
```

`internal/sealed` can only be imported by packages under
`clients/go-sdk/...`. External code — including user applications —
cannot call `sealed.Internal()` because they cannot import the package.

```go
// usecase/result.go
package usecase

import "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/internal/sealed"

type UseCaseResult[E any] interface {
    isResult()                 // unexported method — seals the interface
    IsSuccess() bool
    Unwrap() (E, *UseCaseError)
}

type successImpl[E any] struct{ value E }
func (successImpl[E]) isResult()           {}
func (s successImpl[E]) IsSuccess() bool    { return true }
func (s successImpl[E]) Unwrap() (E, *UseCaseError) { return s.value, nil }

type failureImpl[E any] struct{ err *UseCaseError }
func (failureImpl[E]) isResult()           {}
func (failureImpl[E]) IsSuccess() bool      { return false }
func (f failureImpl[E]) Unwrap() (E, *UseCaseError) { var zero E; return zero, f.err }

// Success requires a token. Only packages that can import internal/sealed
// can produce one — that's outboxpgx, outboxsql, outboxmem, and the
// internal helpers inside usecase itself.
func Success[E any](_ sealed.Token, value E) UseCaseResult[E] {
    return successImpl[E]{value: value}
}

func Failure[E any](err *UseCaseError) UseCaseResult[E] {
    return failureImpl[E]{err: err}
}
```

Result: a use case that tries to construct a `Success` directly fails to
compile (cannot import `internal/sealed`). The only way to return success
is to route through `unit_of_work.Commit(...)`. Same property as Rust.

## UseCase contract

```go
// usecase/use_case.go
package usecase

import "context"

type UseCase[C any, E DomainEvent] interface {
    Validate(ctx context.Context, cmd C) *UseCaseError
    Authorize(ctx context.Context, cmd C, ec ExecutionContext) *UseCaseError
    Execute(ctx context.Context, cmd C, ec ExecutionContext) UseCaseResult[E]
}

// Run executes validate → authorize → execute and short-circuits on first
// error. Free function (not a method on the interface) so users cannot
// override the pipeline shape — same as the Rust SDK's provided run().
func Run[C any, E DomainEvent](
    ctx context.Context,
    uc UseCase[C, E],
    cmd C,
    ec ExecutionContext,
) UseCaseResult[E] {
    if err := uc.Validate(ctx, cmd); err != nil {
        return Failure[E](err)
    }
    if err := uc.Authorize(ctx, cmd, ec); err != nil {
        return Failure[E](err)
    }
    return uc.Execute(ctx, cmd, ec)
}
```

Returning `*UseCaseError` (pointer) lets `nil` mean success without
allocating a wrapper. Stylistic; could just as well be `error` plus a
type assertion.

## UnitOfWork — generic over the transaction handle

```go
// usecase/persistable.go
package usecase

import "context"

type Persistable[Tx any] interface {
    ID() string
    Upsert(ctx context.Context, tx Tx) error
    Delete(ctx context.Context, tx Tx) error
}
```

```go
// usecase/unit_of_work.go
package usecase

import "context"

type UnitOfWork[Tx any] interface {
    Commit(
        ctx context.Context,
        agg Persistable[Tx],
        ev DomainEvent,
        cmd any,
    ) UseCaseResult[DomainEvent]

    CommitDelete(
        ctx context.Context,
        agg Persistable[Tx],
        ev DomainEvent,
        cmd any,
    ) UseCaseResult[DomainEvent]

    EmitEvent(
        ctx context.Context,
        ev DomainEvent,
        cmd any,
    ) UseCaseResult[DomainEvent]

    CommitAll(
        ctx context.Context,
        aggs []Persistable[Tx],
        ev DomainEvent,
        cmd any,
    ) UseCaseResult[DomainEvent]
}
```

Go's lack of method-level type parameters means `Commit` cannot return
`UseCaseResult[E]` where `E` is per-call. The compromise: methods return
`UseCaseResult[DomainEvent]`; callers do a type assertion if they need
the concrete event back. Most application code doesn't — it returns the
UseCaseResult upstream and the handler maps to HTTP.

If this lossiness bothers us later, the alternative is to make the UoW
itself generic over the event type too (`UnitOfWork[Tx, E]`), or to use
a small adapter pattern. The recommended starting point is the lossy
shape because it matches what the TS/Laravel SDKs do at runtime.

### Two concrete implementations

```go
// outboxpgx/unit_of_work.go
type UnitOfWork struct {
    pool   *pgxpool.Pool
    config Config
}
// implements usecase.UnitOfWork[pgx.Tx]

// outboxsql/unit_of_work.go
type UnitOfWork struct {
    db     *sql.DB
    config Config
}
// implements usecase.UnitOfWork[*sql.Tx]
```

An app picks one. Aggregates implement the matching `Persistable[Tx]`.
A test uses `outboxmem.UnitOfWork[Tx]` parameterized over whichever Tx
the production code targets — so the same use-case body runs in tests
without a database.

### Persistable per backend (chosen design)

An aggregate that needs both Postgres and MySQL support implements
**two** `Persistable` interfaces — one per Tx type — using the
appropriate SQL dialect in each. Mirrors what Laravel apps do via
Eloquent (one model class, multiple driver bindings).

We do **not** ship a `SqlTx` wrapper interface that abstracts over
`pgx.Tx` and `*sql.Tx`. The reasons:

1. Apps lose access to pgx-specific features (`CopyFrom`, `Conn()`,
   typed scanning) inside `Upsert`.
2. The "one method, multiple drivers" path is rare — most production
   services pick one DB and stick with it.
3. A wrapper would make the seal harder to reason about: it would have
   to live in `usecase/` and would couple `usecase` to both driver
   packages.

We can add an optional `SqlTx` adapter later if users ask for it.

### `Run` for orchestrated transactions

Mirrors `OutboxUnitOfWork::run` (Rust) and `OutboxUnitOfWork.run` (TS).

```go
// outboxpgx/unit_of_work.go
func (u *UnitOfWork) Run(
    ctx context.Context,
    fn func(ts *TxScopedUnitOfWork) usecase.UseCaseResult[any],
) usecase.UseCaseResult[any]
```

Opens one `pgx.Tx`, hands a `TxScopedUnitOfWork` to the callback,
commits on `Success`, rolls back on `Failure`. The scoped UoW exposes
`WithTx(func(pgx.Tx) error) error` for ad-hoc writes that need to be
atomic with outbox rows.

Use cases stay tx-agnostic: they're parameterized over a
`UnitOfWork[Tx]` and don't know whether they're in a top-level commit
or inside a `Run` block.

## Library picks

### Required (in `go.mod` of the SDK module itself)

| Module                                     | Purpose                              | Notes                                          |
|--------------------------------------------|--------------------------------------|------------------------------------------------|
| `github.com/jackc/pgx/v5`                  | Postgres driver (primary)            | `pgx.Tx`, `pgxpool`. Also via `pgx/v5/stdlib`. |
| `github.com/go-sql-driver/mysql`           | MySQL driver for `database/sql`      | Only loaded if user wires `outboxsql` with MySQL. |
| `go.mongodb.org/mongo-driver/v2`           | MongoDB outbox writer                | Only loaded if user imports `outboxmongo`.     |
| `github.com/lestrrat-go/jwx/v2`            | JWT + JWKS                           | `jwk.Cache` for rotation. Far better than `golang-jwt/jwt` for our use. |
| `github.com/redis/go-redis/v9`             | Redis cache + lock                   | Lua release script for atomic lock release.   |
| `github.com/oapi-codegen/oapi-codegen/v2`  | Codegen tool (build-time)            | Generate types from `openapi/openapi.json`. Hand-write resource wrappers. |
| `github.com/google/uuid`                   | Lock-holder tokens                   | Single dep, only in `lock/`.                   |
| `github.com/stretchr/testify`              | Test assertions                      | dev-only.                                      |
| `github.com/testcontainers/testcontainers-go` | Integration tests                 | dev-only. PG + MySQL + Mongo containers.       |

### Avoided

| Library                                | Why not                                                                                               |
|----------------------------------------|-------------------------------------------------------------------------------------------------------|
| `github.com/golang-jwt/jwt/v5`         | Popular but no built-in JWKS cache. We'd reinvent the rotation logic that jwx already ships.          |
| `github.com/hashicorp/go-retryablehttp`| Retry policy in the SDK client is small enough to hand-roll (~60 LoC). Matches Rust's reqwest approach. |
| Web framework (chi/gin/echo)           | Core SDK stays framework-agnostic. Add subpackages per framework in v2 if asked.                      |
| TSID library                           | The wire format is non-negotiable (must match Rust/TS exactly). Hand-roll ~40 LoC.                    |
| `github.com/jmoiron/sqlx`              | We use either pgx or stdlib `database/sql`. sqlx adds a third tx type to bridge — not worth it.      |

## TSID compatibility

The 13-char Crockford Base32 encoding plus the `prefix_` typed prefix
format are wire contracts. The Go implementation must produce IDs
identical to those from Rust and TS for the same `(timestamp, random)`
input. Test approach: round-trip 1000 IDs in the integration suite
against a known seed; assert byte-for-byte equality with a reference
fixture file committed to the repo.

The `EntityType` enum must enumerate the same 30 variants as
`crates/fc-sdk/src/tsid.rs`. A doc comment in `entity_type.go` should
point at the Rust file as the source of truth; whoever adds a 31st
variant adds it to both.

## HTTP client shape

Mirror the Rust `FlowCatalystClient`:

```go
type FlowCatalystClient struct {
    baseURL    string
    http       *http.Client
    token      func() (string, error)
    routerBase string
}

func New(baseURL string, opts ...Option) *FlowCatalystClient { ... }

func (c *FlowCatalystClient) EventTypes() *EventTypesResource    { ... }
func (c *FlowCatalystClient) Subscriptions() *SubscriptionsResource { ... }
// ... one accessor per aggregate
```

Each resource gets its own file under `client/`, e.g. `eventtypes.go`,
with methods named per the Rust SDK (`Create`, `Get`, `GetByCode`, `List`,
`Update`, `Archive`, `Delete`, `Sync`). Methods take `context.Context`
first, return `(T, error)`. Errors carry an HTTP-status-aware type so
callers can `errors.As(err, &client.APIError{})`.

`OidcTokenManager` (for `client_credentials` grant) lives in `auth/` and
plugs in via `WithTokenManager(...)`.

## Webhook validator

Single struct in `webhook/validator.go`. Constants `SignatureHeader`,
`TimestampHeader`. Default tolerance 300s, 60s future grace. Same field
names and behaviour as Rust. The `Validate` method takes `(signature,
timestamp, body)` — no `http.Request` coupling, so the same validator
works behind any framework.

## Scheduled jobs runner

Mirrors `crates/fc-sdk/src/scheduled_jobs/runner.rs`:

- Register handlers by job code.
- Poll the platform for due instances (or be invoked via inbound webhook).
- Acquire a distributed lock if the job is `concurrent: false`.
- Run the handler with `context.Context` + an envelope of metadata.
- Stream log lines back via `POST /api/scheduled-jobs/instances/:id/log`.
- Call `POST /api/scheduled-jobs/instances/:id/complete` on exit.

Memory-backed lock provider in `lock/memory.go` ships by default; PG
and Redis backends are opt-in via the `lock/` subpackages.

## Versioning & release

- Independent semver via the `clients/go-sdk/vX.Y.Z` tag pattern (Go's
  monorepo convention).
- Initial release `clients/go-sdk/v0.1.0` once `usecase/`, `outbox/`,
  `outboxpgx/`, `outboxsql/`, `outboxmem/`, `tsid/`, and `webhook/` are
  in place with passing tests.
- `client/` and `sync/` ship in `v0.2.0`. `auth/`, `cache/`, `lock/`,
  `scheduledjobs/` in `v0.3.0`. This matches the order the Rust SDK
  layered features.

## What this design does NOT do (deliberately)

- **No automatic outbox table creation in production.** `schema.go`
  exposes the SQL string and a `Init(ctx, db)` helper for dev. Apps run
  it through their own migrations.
- **No struct tags driving event payload shape.** `DomainEvent.ToDataJSON()`
  returns a string built by the implementor — mirrors Rust/TS/Laravel.
- **No reflection-based command-name extraction.** The Rust SDK uses
  `std::any::type_name::<C>()` for audit logs. Go's equivalent is
  `reflect.TypeOf(cmd).Name()` — we'll use it, but as a hint only; apps
  that want a stable name pass it explicitly via a builder method on
  the audit DTO.
- **No effect-system port.** TS ships an `effect/` subpath. Go gets
  plain interfaces.

## References

- `crates/fc-sdk/src/lib.rs` — Rust SDK entry point.
- `crates/fc-sdk/src/outbox/unit_of_work.rs` — canonical UoW + `Run`.
- `clients/typescript-sdk/src/usecase/outbox-unit-of-work.ts` — TS port.
- `clients/laravel-sdk/src/UseCase/OutboxUnitOfWork.php` — Laravel port.
- `CLAUDE.md` — workspace conventions; the UoW invariant section applies
  here too (compile-time seal, single write path per aggregate,
  layering rules).

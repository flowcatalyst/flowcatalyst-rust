# FlowCatalyst Go SDK

Go SDK for the FlowCatalyst platform. Sibling of `crates/fc-sdk` (Rust),
`clients/typescript-sdk`, and `clients/laravel-sdk`. Module path:

```
github.com/flowcatalyst/flowcatalyst/clients/go-sdk
```

Wire-compatible with the other three SDKs — a token, event payload,
or TSID minted in any of them is byte-identical to the same value
minted here.

## Mental model

A consumer app emits domain events through a `UnitOfWork`. The UoW
writes the aggregate row, the event, and (if enabled) the audit log
inside one transaction; on commit, the event lands in
`outbox_messages`, and `fc-outbox-processor` forwards it to the
platform's `/api/events/batch`. Use cases never call the platform
directly during a transaction.

```
HTTP request → command DTO
             → usecase.ExecutionContext  (principal + correlation)
             → usecase.Run(useCase, cmd, ec)
                  └ Validate → Authorize → Execute
                                             ↓
                                        usecasepgx.Commit (one tx)
                                          ├ Repo.Persist
                                          ├ Sink.WriteEvent  → outbox_messages
                                          └ Sink.WriteAudit  → outbox_messages
             → usecase.Into(result) → (T, error)
             → HTTP 201 / 4xx / 500
```

The `Sink` slot is what makes the SDK reusable. Consumer apps wire
`outboxpgx.Sink`; the platform itself wires its own sink that writes
directly to `msg_events` / `iam_audit_logs`.

## Getting started — `examples/order-service`

Walk through `examples/order-service/main.go` — it's the single best
place to start. The file is ~250 lines and shows every assembly point:

1. **Aggregate + repository** (`Order` + `OrderRepository`) — implements
   `usecase.HasID` and `usecasepgx.Persist[Order]`. The aggregate
   doesn't write itself; the repository does. The CLAUDE.md
   "aggregates don't persist themselves" rule applies in Go too.
2. **Domain event** (`OrderPlaced`) — embeds `usecase.EventMetadata`,
   implements `usecase.DomainEvent`. `ToDataJSON` returns the
   event-specific payload.
3. **Use case** (`PlaceOrder`) — implements
   `usecase.UseCase[PlaceOrderCommand, OrderPlaced]` with
   `Validate` / `Authorize` / `Execute`. `Execute`'s last line is
   `return usecasepgx.Commit(...)` — that's the only legal happy-path
   shape because the `Result[E]` seal won't let you mint a Success
   anywhere else.
4. **HTTP handler** — parses the body into a command, builds an
   `ExecutionContext` from the auth header, calls `usecase.Run`, and
   maps the result back to HTTP via `usecase.Into` + `usecase.AsError`.
5. **`main`** — wires `pgxpool` → `outboxpgx.Sink` → `usecasepgx.New`
   → the use case → `http.ServeMux`.

```
FC_DATABASE_URL=postgres://localhost:5432/orders go run ./examples/order-service
curl -XPOST http://localhost:8080/orders \
    -H 'X-Principal-ID: prn_demo' \
    -H 'Content-Type: application/json' \
    -d '{"customerId":"cus_42","totalCents":1500}'
```

A row appears in `orders` AND a row appears in `outbox_messages`,
atomically. `fc-outbox-processor` picks it up and forwards to the
platform on its next poll.

### Other runnable examples

- `examples/list-event-types` — minimal `client` + `auth` wiring, two
  flavors (static bearer + `ClientCredentialsProvider`).
- `examples/fc-sync` — declarative reconciliation across roles, event
  types, dispatch pools, subscriptions, and processes.
- `examples/scheduled-jobs-runner` — webhook-driven `scheduledjobs.Runner`
  with two handlers and a memory lock provider.
- `examples/webhook-receiver` — HMAC-validated stdlib HTTP handler
  with the full sentinel-based error switch.

All five compile under `go build ./examples/...`.

## What's in the box

| Package | Purpose |
|---|---|
| `usecase` | `Result[E]` (sealed sum), `UseCase[C,E]`, `Run[C,E]`, `DomainEvent`, `EventMetadata`, `ExecutionContext`, `Error` + helpers. Driver-agnostic, no I/O. |
| `usecasepgx` | pgx-backed `*UnitOfWork`, `Persist[A]`, `Sink`, generic `Commit` / `CommitDelete` / `CommitAll` / `EmitEvent` free functions, `Run(...)` for orchestrated multi-aggregate tx. |
| `usecasesql` | Same shape as `usecasepgx`, backed by `database/sql` (Postgres + MySQL). |
| `outboxpgx` | Consumer `Sink` that writes to `outbox_messages` via pgx. `fc-outbox-processor` forwards. |
| `outboxsql` | Same as `outboxpgx`, for `database/sql` consumers. |
| `tsid` | TSID generator (13-char Crockford Base32) + 35 `EntityType` prefixes matching the other SDKs byte-for-byte. |
| `webhook` | HMAC-SHA256 inbound webhook validator. Stdlib only; framework-agnostic. |
| `client` | Platform HTTP API client. `*FlowCatalystClient` + per-aggregate resources: `EventTypes`, `Subscriptions`, `DispatchPools`, `Applications`, `Processes`, `Principals`, `Roles`, `Permissions`, `AuditLogs`, `Clients` (tenants), `Connections`, `Me`, `Router`, `ScheduledJobs`, `OpenAPI`. Retry on transient 5xx, typed `*APIError`, bearer token or `TokenProvider` auth. |
| `sync` | Declarative reconciliation. Build a `DefinitionSet` with the per-category fluent builders, hand it to a `Synchronizer`; one HTTP call per category, errors captured per-category. |
| `scheduledjobs` | Consumer-side `Runner` for platform-fired scheduled-job webhooks. Register `HandlerFunc`s by job code; runner serialises via a `lock.Provider`, streams log lines back, reports completion. |
| `auth` | OIDC: `AccessTokenClaims` + `AuthContext`, `TokenValidator` (RS256 via JWKS auto-discovery, `lestrrat-go/jwx/v2`), `HmacTokenValidator` (HS256), `OAuthClient` (PKCE / refresh / revoke / introspect / userinfo / RP-initiated logout), `ClientCredentialsProvider` (service-to-service `TokenProvider`). |
| `cache` | Pluggable byte-oriented cache with required TTL on every write. Generic `Get[T]` / `Set[T]` / `GetOrSet[T]` JSON helpers. `MemoryCache` ships here; `cache/postgrescache` (pgx) and `cache/rediscache` (go-redis/v9) are opt-in sub-packages. |
| `lock` | Distributed-lock contract. `NoOp` + `Memory` ship here; `lock/postgreslock` (pgx, table-based with WHERE-on-upsert + UUID holder tokens) and `lock/redislock` (SET NX PX + Lua check-and-delete) are opt-in sub-packages. |
| `internal/sealed` | Token type. Constructable only by packages under `clients/go-sdk/`; gates `usecase.Success`. Compile-time enforcement of the seal. |

## The seal

A `usecase.Result[E]` whose inner value is a success can only be
produced by calling `usecase.Success[E](sealed.Token, value)`. The
`sealed.Token` type lives in `internal/sealed/` and is only
constructable by packages **under the SDK module** — Go's `internal/`
rule prevents external code from importing it. Therefore the only
path to a Success-valued Result outside the SDK is through one of
the `Commit*` / `EmitEvent` free functions in `usecasepgx` /
`usecasesql`.

This is the Go analogue of the Rust SDK's
`pub(in crate::usecase) fn success(...)` — compile-time enforced.

## Picking a backend

Apps that talk to Postgres via pgx (most performance-sensitive choice):

```go
import (
    "github.com/flowcatalyst/flowcatalyst/clients/go-sdk/usecasepgx"
    "github.com/flowcatalyst/flowcatalyst/clients/go-sdk/outboxpgx"
)

sink := outboxpgx.NewSink(outboxpgx.Config{
    ClientID:     "clt_0HZXEQ5Y8JY5Z",
    AuditEnabled: false,
})
uow := usecasepgx.New(pool, sink)

// In a use case:
result := usecasepgx.Commit(ctx, uow, &order, orderRepo, event, command)
```

Apps that need MySQL (or want one code path across drivers via
`database/sql`):

```go
import (
    "github.com/flowcatalyst/flowcatalyst/clients/go-sdk/usecasesql"
    "github.com/flowcatalyst/flowcatalyst/clients/go-sdk/outboxsql"
)

sink := outboxsql.NewSink(outboxsql.Config{ /* ... */ })
uow := usecasesql.New(db, sink)
result := usecasesql.Commit(ctx, uow, &order, orderRepo, event, command)
```

The platform itself implements its own `Sink` that writes directly
to `msg_events` / `iam_audit_logs` — that sink does **not** live in
this SDK.

## Service-to-service auth

For server-side apps calling the platform with a confidential client:

```go
import (
    "github.com/flowcatalyst/flowcatalyst/clients/go-sdk/auth"
    "github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client"
)

cc := auth.NewClientCredentialsProvider(auth.ClientCredentialsConfig{
    IssuerURL:    "https://auth.flowcatalyst.io",
    ClientID:     "svc-app",
    ClientSecret: "...",
})
c := client.New("https://api.flowcatalyst.io", client.WithTokenProvider(cc.Token))
```

`cc.Token` is a `client.TokenProvider`. It fetches via the OAuth2
`client_credentials` grant on first call, caches until 60s before
expiry, and refreshes on demand. Concurrent-safe.

For resource servers that need to validate incoming bearer tokens:

```go
v := auth.NewTokenValidator(auth.TokenValidatorConfig{
    IssuerURL: "https://auth.flowcatalyst.io",
    Audience:  "my-app",
})

func handler(w http.ResponseWriter, r *http.Request) {
    ctx, err := v.ValidateBearer(r.Context(), r.Header.Get("Authorization"))
    if err != nil { /* 401 */ return }
    if !ctx.HasRole("admin") { /* 403 */ return }
    // ...
}
```

OIDC discovery happens on the first request. JWKS rotation is handled
automatically via `lestrrat-go/jwx/v2`'s `jwk.Cache` (default refresh
interval: 1h, configurable via `JWKSRefreshInterval`).

## Errors

See [ERRORS.md](ERRORS.md) for the full table of which sentinels use
`errors.Is` vs which typed errors use `errors.As`, per package.

## Design doc

`docs/architecture/go-sdk.md` (in the parent repo) has the full design:
rationale for sibling vs sub-packages, the seal mechanism, why
opt-in driver packages, the multi-backend story.

## License

Copyright (c) 2026 Belac (FlowCatalyst).

[MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/) — see [LICENSE](LICENSE).
File-level copyleft: changes to the SDK's own files must be shared; your
application that depends on it is not affected.

// Package gosdk is the entry-point overview for the FlowCatalyst Go
// SDK. There is no Go code in the root — this file exists so that
// `go doc github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk`
// returns a useful map of what's where.
//
// The SDK has byte-for-byte wire parity with the Rust, TypeScript,
// and Laravel SDKs: a token, event payload, or TSID minted by any one
// of them is identical to the same value minted by another.
//
// # Mental model
//
// A FlowCatalyst consumer app emits domain events through a
// UnitOfWork. The UoW writes the aggregate row, the event, and (if
// enabled) the audit log inside one transaction; on commit, the
// event lands in outbox_messages, and fc-outbox-processor forwards
// it to the platform's /api/events/batch. Use cases never call the
// platform directly during a transaction.
//
//	HTTP request → command DTO
//	             → usecase.ExecutionContext (principal + correlation)
//	             → usecase.Run(useCase, cmd, ec)
//	                  ↓ Validate / Authorize / Execute
//	             → usecasepgx.Commit (single transaction)
//	                  ↳ <Repo>.Persist
//	                  ↳ Sink.WriteEvent → outbox_messages
//	                  ↳ Sink.WriteAudit → outbox_messages (optional)
//	             → usecase.Into(result) → (T, error)
//	             → HTTP 201 / 4xx / 500
//
// The Sink slot is what makes the SDK reusable. Consumer apps wire
// outboxpgx.Sink (writes to outbox_messages); the platform itself
// wires its own sink that writes directly to msg_events.
//
// # Package map
//
// Domain primitives (no I/O):
//
//   - usecase    — UseCase + Result + DomainEvent + ExecutionContext.
//                  Result[E] is a sealed sum: Success requires a
//                  sealed.Token only SDK packages can mint, so the
//                  only path to Success outside the SDK is through
//                  usecasepgx / usecasesql Commit*. This is the
//                  Go analogue of the Rust SDK's pub(in crate::usecase)
//                  guarantee — compile-time enforced.
//   - tsid       — Time-Sorted IDs (Crockford Base32). 35 typed
//                  EntityType prefixes plus GenerateWithPrefix for
//                  app-specific IDs.
//
// UnitOfWork drivers:
//
//   - usecasepgx — pgx-backed UoW. Construct: usecasepgx.New(pool, sink).
//                  Entry points: Commit / CommitDelete / CommitAll /
//                  EmitEvent / Run (for orchestrated multi-aggregate tx).
//   - usecasesql — same shape, backed by database/sql (Postgres + MySQL).
//
// Sinks (where committed events go):
//
//   - outboxpgx  — writes to outbox_messages via pgx. The default
//                  for consumer apps.
//   - outboxsql  — same, via database/sql.
//
// HTTP I/O:
//
//   - client          — *FlowCatalystClient + 15 resource families
//                       (event_types, subscriptions, dispatch_pools,
//                       applications, processes, principals, roles,
//                       permissions, audit_logs, clients,
//                       connections, me, router, scheduled_jobs,
//                       openapi). Retry on transient 5xx, typed
//                       *APIError. Bearer token or TokenProvider auth.
//   - auth            — AccessTokenClaims + AuthContext;
//                       TokenValidator (RS256 via JWKS auto-discovery
//                       through lestrrat-go/jwx/v2);
//                       HmacTokenValidator (HS256);
//                       OAuthClient (PKCE auth-code, refresh, revoke,
//                       introspect, userinfo, RP-initiated logout);
//                       ClientCredentialsProvider (satisfies
//                       client.TokenProvider).
//   - webhook         — HMAC-SHA256 inbound webhook validator.
//                       Stdlib only; framework-agnostic.
//   - sync            — DefinitionSet + Synchronizer for declarative
//                       reconciliation. One call per category;
//                       failures captured per-category (no abort).
//   - scheduledjobs   — consumer-side Runner. Register HandlerFuncs
//                       by job code; the runner serialises via a
//                       lock.Provider, streams log lines back, and
//                       reports completion.
//
// Infrastructure (TTL-required):
//
//   - cache       — pluggable byte-oriented Cache. Generic
//                   Get[T] / Set[T] / GetOrSet[T] JSON helpers.
//                   MemoryCache ships here; cache/postgrescache (pgx)
//                   and cache/rediscache (go-redis/v9) are opt-in
//                   sub-packages so callers who only need memory
//                   don't pull driver deps.
//   - lock        — distributed-lock Provider + Handle. NoOp +
//                   Memory ship here; lock/postgreslock (table-based
//                   with WHERE-on-upsert + UUID holder tokens) and
//                   lock/redislock (SET NX PX + Lua check-and-delete)
//                   are opt-in sub-packages.
//
// Internal:
//
//   - internal/sealed — Token type that gates usecase.Success.
//                       Constructable only by packages under
//                       clients/go-sdk/ (Go's internal rule).
//
// # Examples
//
// Runnable example apps live in examples/:
//
//   - order-service        — end-to-end UoW flow. The single best
//                            place to start for "how do I write a
//                            consumer app".
//   - list-event-types     — minimal client + auth wiring.
//   - fc-sync              — declarative reconciliation across roles,
//                            event types, dispatch pools, subscriptions.
//   - scheduled-jobs-runner — webhook-driven Runner with two handlers
//                            and a memory lock provider.
//   - webhook-receiver     — HMAC-validated stdlib HTTP handler with
//                            sentinel-based error mapping.
//
// # Errors
//
// See ERRORS.md at the SDK root for the full table of which sentinels
// to errors.Is vs which to errors.As, per package.
//
// # Design notes
//
// The design doc at docs/architecture/go-sdk.md in the parent repo
// walks through every architectural decision: why generics free
// functions over interface methods, why opt-in sub-packages for
// drivers, why the seal lives in internal/.
package gosdk

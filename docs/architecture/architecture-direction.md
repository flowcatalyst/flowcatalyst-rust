# Architecture Direction

Working notes on the platform's long-term shape — what Rust earns, what the
OIDC bridge does and doesn't do, and how upcoming features (scheduler,
distributed configs) fit the existing patterns. Captured from a strategic
discussion; treat as living guidance, not finalised spec.

## The control-plane pattern

FlowCatalyst is a multi-tenant control plane, not just a router. The
shape:

- **Applications self-register** their domain via typed sync endpoints
  (`/api/sync/event-types`, `/api/sync/roles`, `/api/sync/subscriptions`,
  `/api/sync/dispatch-pools`, `/api/sync/principals`, soon
  `/api/sync/scheduled-tasks` and per-app config schemas).
- **The platform is the trust anchor for authn/authz** across all of them
  via the OIDC bridge plus local password fallback for clients without an
  IDP, plus service-account credentials.
- **Cross-app assignment lives centrally** — users get roles per
  application, accessible_clients, applications[], scope. The platform
  enforces invariants across all of these.
- **Domain events are the integration mechanism** between apps. Event types,
  subscriptions, dispatch jobs, and (soon) scheduled tasks all flow through
  the same pipeline.

This pattern — typed registration in, typed assignment in the middle,
events out — is the spine. Every new feature should map to it before it
gets its own machinery.

## Why all-Rust, given this shape

Three properties of the architecture that compound Rust's structural
strengths:

1. **End-to-end typed contracts.** Every sync endpoint is `utoipa`-annotated,
   the SDK in each language (Rust/TS/Laravel) is regenerated from the
   OpenAPI, and an app's own code consumes typed clients. A typo in a role
   name fails at the *app's* compile time, not at runtime in production.
   The wider the registry surface grows, the more this compounds.

2. **Many invariants under concurrency.** Principal-client-role consistency,
   the UoW seal, accessible_clients narrowing for non-anchors, batch+group
   FIFO. The compile-time UoW seal we have today, plus Drop-on-callback
   resource cleanup, are language properties — not patterns we'd ever
   reproduce safely in Node.

3. **Predictable latency and memory at scale.** Router, scheduler, configs:
   all in-memory state under concurrent mutation, all tail-latency
   sensitive. Rust's deterministic memory model and tokio's primitives
   (`watch`, `RwLock`, timer wheels) outperform Node's GC + libuv at the
   kind of throughput a multi-tenant SaaS targets.

The OIDC argument that nearly tipped this the other way — "Rust ecosystem
is weak at OIDC providers" — only applies to general-purpose OPs. See
below.

## The OIDC bridge: scoped narrowly on purpose

### Role

The OIDC server is a **domain-aware token bridge**, not a general-purpose
identity provider. Its job:

- Receive a token from a customer's IDP (Entra, Google Workspace, …).
- Validate via the configured `IdentityProvider` + JWKS cache.
- Resolve the principal in `iam_principals` (or auto-create on first
  login if `EmailDomainMapping` permits — see `auth/oidc_sync_service.rs`).
- Issue an FC token enriched with FC-specific claims (`scope`, `clients`,
  `roles`, `applications`).
- Manage the FC frontend session.

For tenants without their own IDP, **local password authentication** stands
in for step 1. For machine clients, **service-account credentials** (OAuth
client_credentials grant or signed token) replace the user flow. Same
post-validation path — the scope claim and assignment model don't change.

### What's in scope

| Surface | Library / pattern | Status |
|---|---|---|
| OIDC bridge to customer IDPs | `openidconnect-rs` (consumer side), `JwksCache` | Done |
| Auth-code + PKCE issuer | hand-rolled in `auth/oauth_api.rs` | Done |
| ID/access/refresh token issuance with FC claims | `auth/auth_service.rs::generate_*` | Done |
| Local password (clients without IDP) | `argon2` crate | Verify |
| Service account auth | `jsonwebtoken` + `oauth_clients` | Done |
| Token rotation, single-use codes | `find_and_consume_state` (DELETE…RETURNING) | Done |
| RP-initiated logout | `auth/oidc_login_api.rs` | Done |
| JWKS rotation | manual today | Worth automating |
| UserInfo / introspection / revocation | RFC 7662, RFC 7009 | Done |

### What's deliberately out of scope

Reject these explicitly (in code, not just by convention) and write tests
that they're rejected:

- Implicit flow (`response_type=token`) — deprecated.
- Hybrid flow.
- `grant_type=password` (OAuth resource owner password credentials).
- Dynamic client registration (RFC 7591).
- CIBA (banking-grade backchannel).
- FAPI 2.0 profiles.
- Token exchange (RFC 8693).
- UMA / consent screens.
- Front-channel logout (RP-initiated covers what we need).

The narrower the bridge stays, the smaller the attack surface. Anyone
proposing to add one of these should produce a real customer requirement
and a security review plan first.

### Maintenance vigilance

Wire-protocol correctness is a permanent cost. The places that have to be
airtight:

- **JWT signing/verification** — clock skew, `kid` handling, alg confusion.
- **Refresh token rotation** — single-use, parent-chain detection of
  replay (rotate-on-refresh and revoke-on-reuse).
- **Authorization code one-shot consumption** — already enforced via
  `find_and_consume_state` (DELETE…RETURNING). Don't regress.
- **JWKS rotation** — when keys roll, tokens minted under the old kid must
  validate until expiry. Cache TTL must respect this.
- **Login attempt rate limiting** — `iam_login_attempts` table exists; make
  sure local password and service-account paths actually consult it.

Schedule a focused security review of just this layer once a year, language
notwithstanding. Document the in-scope flows in a single source of truth
(this doc, or a sibling `docs/auth/scope.md`) so a reviewer knows what to
test against.

## Forward-looking: scheduled tasks (EventBridge-style)

Slot into the existing pattern rather than inventing new infrastructure.

### Registration

Apps declare scheduled tasks the same way they declare subscriptions:

```jsonc
// in an app's manifest
"scheduledTasks": [
  {
    "code": "billing:nightly-rollup",
    "cron": "0 2 * * *",
    "timezone": "Australia/Brisbane",
    "target": { "kind": "event", "eventType": "billing:rollup:requested" },
    "payload": { ... }
  }
]
```

`POST /api/sync/scheduled-tasks` upserts with diff, emits a single
`ScheduledTasksSynced` event, audit-logs the change. Authorisation to
edit at runtime via UI is a role check on a new permission
(`scheduled_tasks:write`), per the existing convention.

### Storage and triggering

- New table `msg_scheduled_tasks`: id, app code, cron, timezone, next
  fire_at, payload, target, status.
- Poller (background task on the platform or a dedicated scheduler crate):
  `SELECT … WHERE next_fire_at <= NOW() … FOR UPDATE SKIP LOCKED LIMIT N`
  — same pattern as the dispatch-jobs scheduler in
  `crates/fc-platform-messaging/src/scheduler/`.
- Cron parser at registration time, fail-loudly. Use `croner` (DST and
  timezone correctness baked in) over `cron`.

### Trigger output: events, not direct dispatch

When a schedule fires, **emit a domain event** (`billing:rollup:requested`
in the example) into the existing event pipeline. The pipeline already
knows how to fan that out into dispatch jobs via subscriptions. Don't
short-circuit straight to dispatch — that creates a parallel pipeline and
breaks the "events are the integration mechanism" invariant.

The schedule itself can carry a small `payload` object that becomes the
event's data. For larger payloads, the schedule references an
event-type-defined schema and the app's manifest validates at sync time.

## Forward-looking: distributed configs

Same registration pattern as everything else.

### Shape

- App declares its config schema in the manifest:
  ```jsonc
  "configs": [
    { "key": "ratesPerCustomer", "type": "object", "schema": "…" },
    { "key": "featureFlags",     "type": "object", "schema": "…" }
  ]
  ```
- `POST /api/sync/configs` registers schemas. Schema validation is enforced
  at write time on the values.
- Values stored per `(app, client, key)`. Optionally per-environment.
- Hot reload via `tokio::sync::watch` for in-process subscribers.
- Out-of-process consumers (apps, the message router) use a sync polling
  endpoint, like the router already does for queue/pool config — that
  pattern is mature and we just hardened it (parallel fetch, partial
  failure tolerance, first-wins merge).

### Compile-time win

Because each app's config schema is part of its manifest, the app's SDK
codegen produces a typed `Config` struct. App code reads `config.rates_per_customer`
with full IDE support; mistyped keys fail at the app's compile time. The
Rust→codegen→consumer pipeline is what makes this work end-to-end.

## Local password / service-account auth checklist

Small surface, high stakes. Things to verify (or set up alarms for):

- **Argon2id parameters** match OWASP recommendations
  (memory ≥ 47 MiB, time cost ≥ 1, parallelism = 1 minimum).
- **Constant-time comparison** on credential checks (no early-exit string
  compare). `argon2` does this for hashes; double-check any fallback paths.
- **Rate limiting on login** wired through `iam_login_attempts` for both
  local-password and OIDC redirect-back paths. Threshold and lockout
  behaviour documented.
- **Service-account secret rotation** uses the `regenerate_oauth_client_secret`
  pattern (encrypt-at-edge via `EncryptionService`, plaintext returned
  exactly once). Already correctly implemented; don't regress.
- **Login failure observability** — credential-stuffing patterns surface
  via the existing audit-log + warning service.

## Trade-offs we accepted

- **Rust verbosity vs TS iteration speed.** For business-logic CRUD on the
  control plane, we pay a verbosity tax in exchange for compile-time
  correctness across the registry surface. The tax is worth it because the
  surface is wide (many entity types, many invariants between them) and
  changes infrequently per entity once stable.
- **Hand-rolled OIDC bridge vs library-backed full OP.** We forfeit
  ecosystem maturity in exchange for domain-aware tokens and avoiding the
  "wedge an external IDP into our domain model" tax. The narrowness of
  scope makes this defensible long-term, *provided* the maintenance
  vigilance items above stay current.
- **In-house scheduler/configs vs managed services (EventBridge,
  AppConfig).** We forfeit AWS-managed reliability in exchange for being
  self-hostable across cloud providers and integrating natively with the
  app-self-registration model. The same `FOR UPDATE SKIP LOCKED` pattern
  that already runs in production is the design template.

## Where to spend engineering vigilance

In rough priority order:

1. **The sync registration endpoints + diff-event model.** These are the
   seams where the platform absorbs application-declared truth. A bug here
   ripples to every app. Keep tests for: idempotency, partial-failure
   atomicity, diff correctness, audit-log coverage.
2. **The OIDC bridge wire-protocol layer.** See the maintenance section
   above. Annual focused security review.
3. **The router's in-pipeline tracking.** Recently hardened
   (callback Drop, reaper, panic-guard queue drain). Ongoing: watch the
   reaper warning logs; if they ever fire, treat as a P2 to find what's
   leaking.
4. **The scheduler triggering pipeline (when built).** Time-sensitive
   correctness needs care — DST handling, timezone storage, missed-fire
   semantics on platform restart.
5. **Distributed config consistency.** Eventual-consistency model is fine;
   document explicitly so callers don't assume strong reads.

## Open questions worth deciding before each piece lands

- **Scheduler: missed fires on restart** — fire all overdue triggers, fire
  only the most recent one, or skip? Document semantics; align with
  customer expectations.
- **Configs: scoping model** — per-app, per-client-per-app, or
  per-environment-per-client-per-app? Affects schema and access controls.
  Pick before designing the table.
- **Local password: optional MFA** — out of scope today, but a likely
  enterprise ask. Plan the data model for `iam_user_authenticators` so it
  doesn't require a migration when MFA is added.
- **OIDC bridge: when to expire the in-memory JWKS cache** — currently
  manual-or-time-based; consider listening to issuer-side rotation
  signals (`x5t#S256` change, kid not found triggers refetch).

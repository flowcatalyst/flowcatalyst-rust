# SDK Parity Plan

Three SDKs ship from this repo: **Rust** (`crates/fc-sdk/`), **TypeScript**
(`clients/typescript-sdk/`), **Laravel/PHP** (`clients/laravel-sdk/`).
A May 2026 audit identified substantial drift, especially in the Rust
SDK. This document is the prescriptive work plan to close the gap, one
workstream per session.

> **How to use this doc:** each workstream below is sized to a single
> session of work and has explicit deliverables (files + symbols),
> dependencies, and rough LoC. To execute a workstream, point a fresh
> session at the workstream ID (e.g. "execute R1 from
> `docs/sdk-parity-plan.md`").

## Audit summary (May 2026)

| Coverage axis | Rust | TS | Laravel |
|---|---|---|---|
| Aggregates with **full or near-full** HTTP coverage | 5 | 7 | 8 |
| Aggregates **entirely missing** | ~13 | ~12 | ~12 |
| Code-discovery + multi-category sync orchestrator | ❌ | ✅ (programmatic) | ✅ (filesystem + attributes) |
| Inbound JWT/JWKS verifier | ✅ | ❌ | ❌ |
| Outbound OAuth client-credentials manager | ✅ | ✅ | ◐ |
| Webhook HMAC signature verifier | ✅ | ❌ | ✅ |
| Outbox driver shipped (vs interface only) | ✅ Postgres | ❌ BYO | ✅ DB + Mongo |
| Scheduled-job runner / lock provider | ❌ | ✅ | ✅ |

### Per-aggregate coverage

✅ full, ◐ partial, ❌ missing (no module).

| Aggregate | Rust | TS | Laravel |
|---|---|---|---|
| event_types | ✅ | ✅ | ✅ |
| subscriptions | ✅ | ✅ | ✅ |
| processes | ✅ | ✅ | ✅ |
| connections | ✅ | ✅ | ✅ |
| applications | ✅ | ◐ | ◐ |
| clients | ✅ | ◐ | ◐ |
| principals | ◐ | ◐ | ◐ |
| roles | ◐ | ◐ | ◐ |
| dispatch_pools | ◐ | ◐ | ✅ |
| me | ✅ | ✅ | ✅ |
| router (pipeline introspection) | ✅ | ✅ | ✅ |
| dispatch_jobs | ❌ | ❌ | ❌ |
| scheduled_jobs | ❌ | ✅ | ✅ |
| events | ❌ | ❌ | ❌ |
| audit_logs | ◐ (2/10) | ❌ | ❌ |
| oauth_clients | ❌ | ❌ | ❌ |
| service_accounts | ❌ | ❌ | ❌ |
| identity_providers | ❌ | ❌ | ❌ |
| email_domain_mappings | ❌ | ❌ | ❌ |
| anchor_domains | ❌ | ❌ | ❌ |
| auth_configs | ❌ | ❌ | ❌ |
| idp_role_mappings | ❌ | ❌ | ❌ |
| cors | ❌ | ❌ | ❌ |
| login_attempts | ❌ | ❌ | ❌ |
| password_reset | ❌ | ❌ | ❌ |
| public (`/api/public`) | ❌ | ❌ | ❌ |

### Verification flagged during audit

The implementing session must verify these before writing code — the
audit relied on stale `MEMORY.md` notes for two items:

1. **OAuth client routes**: `MEMORY.md` says `/api/oauth-clients/*` but
   no `oauth_client/api.rs` exists in `crates/fc-platform/src/`. **Verified
   2026-05-15** — routes live in `crates/fc-platform-iam/src/auth/oauth_clients_api.rs`.
   R7 should target that file.
2. **Rust SDK `sync.rs::sync_processes`**: not present. **Verified
   2026-05-15** — and the audit's framing was partially wrong. The split is:
   `client/sync.rs` hosts `sync_{roles,event_types,subscriptions,dispatch_pools}`
   and `sync_principals` (added in R6); `client/processes.rs` owns
   `sync_processes` because the processes sync endpoint is not app-scoped
   in the URL (`POST /api/processes/sync` with `applicationCode` in body).
   `client/sync.rs` doc comments now describe this split. The new
   high-level orchestrator lives at `fc_sdk::sync` (the R6 module tree).

## Workstream sequencing

Rough priority. Higher rows unlock or de-risk lower ones.

```
Phase 1 — Cleanup (closes loose ends from prior sessions)
  L1  Laravel: complete Processes integration
  L2  Laravel: regenerate or delete Generated/Endpoint tree

Phase 2 — Rust SDK aggregate coverage (highest user-visible impact)
  R1  Rust: dispatch_jobs.rs
  R2  Rust: scheduled_jobs.rs
  R3  Rust: events.rs (incl. batch ingest)
  R4  Rust: expand audit_logs.rs
  R5  Rust: expand principals/dispatch_pools/roles (small gaps)

Phase 3 — Rust SDK architectural surfaces
  R6  Rust: sync orchestrator (new sync/ tree)
  R7  Rust: oauth_clients.rs + service_accounts.rs

Phase 4 — Rust SDK long tail (low priority, do as consumers ask)
  R8  Rust: identity_providers + email_domain_mappings + anchor_domains
  R9  Rust: auth_configs + idp_role_mappings + cors + login_attempts
  R10 Rust: password_reset + public

Phase 5 — TS SDK gaps
  T1  TS: webhook validator
  T2  TS: JWKS verifier
  T3  TS: dispatch-jobs + audit-logs + service-accounts + events resources

Phase 6 — Laravel SDK auth parity
  P1  Laravel: JWKS verifier + outbound OIDC token manager
```

L1 should land first — it closes a known incomplete piece. Phase 2 then
moves the Rust SDK from "behind on most aggregates" to "behind on long
tail only," which is the level where most consumers stop hitting gaps.

---

## L1 — Laravel: complete Processes integration — ✅ DONE (2026-05-15)

Landed `Resources/Processes.php`, `SyncProcessEntry`, `ProcessList`,
`processes()` accessor on `FlowCatalystClient`, `AsProcess` wiring in
`DefinitionScanner`, `processes` on `ScannedDefinitions`/`SyncDefinitionSet`/
`SyncResult`/`SyncOptions`/`DefinitionSynchronizer`, and surfaced the new
category in `flowcatalyst:sync`'s dry-run and result-table output.
`php artisan flowcatalyst:sync` now pushes processes alongside roles, event
types, subscriptions, and dispatch pools.

**Why this is first:** the prior session shipped DTOs/Enums/Attribute/SyncDefinition for Processes but skipped the HTTP wrapper and scanner wiring, so `php artisan flowcatalyst:sync` can't push processes today.

**Files to create:**

- `clients/laravel-sdk/src/Client/Resources/Processes.php` — HTTP wrapper class. Mirror `src/Client/Resources/EventTypes.php` exactly: constructor `__construct(private FlowCatalystClient $client)`, methods `list()`, `get(string $id)`, `getByCode(string $code)`, `create(array $data)`, `update(string $id, array $data)`, `archive(string $id)`, `delete(string $id)`, `sync(string $appCode, array $processes, bool $removeUnlisted)`.

**Files to edit:**

- `clients/laravel-sdk/src/Client/FlowCatalystClient.php` — add `processes()` accessor returning `Resources\Processes`. Mirror the `eventTypes()` lazy-init pattern.
- `clients/laravel-sdk/src/Definition/DefinitionScanner.php` — import `FlowCatalyst\Attributes\AsProcess`; add a `$processes` parameter to `processClass(...)` signature; loop through `AsProcess` attributes the same way `AsEventType` is handled (~10 lines).
- `clients/laravel-sdk/src/Definition/ScannedDefinitions.php` — add `array $processes = []` to constructor, expose in `toArray()` and `fromArray()`, include in `isEmpty()` and `count()`.
- `clients/laravel-sdk/src/Sync/SyncDefinitionSet.php` — add `$processes` array property, `withProcesses(array)`, `addProcess(ProcessDefinition|array)`, `getProcesses()` (with same `instanceof` mapping as event types), `hasProcesses()`. Wire into `isEmpty()` and `fromScannedDefinitions()`.
- `clients/laravel-sdk/src/Sync/DefinitionSynchronizer.php` — add `$processesResult` initialiser, an `if ($options->syncProcesses && $definitions->hasProcesses())` block calling a new private `syncProcesses(...)` method, return field in `SyncResult`.
- `clients/laravel-sdk/src/Sync/SyncResult.php` — add `array $processes` to constructor (default `['created' => 0, 'updated' => 0, 'deleted' => 0]`).
- `clients/laravel-sdk/src/Sync/SyncOptions.php` — add `bool $syncProcesses = true` field; include in `::defaults()` and `::all()` factories.

**Generated/ tree:** `Generated/Endpoint/*Process*.php` would normally come from jane-openapi. **L2 below** handles whether to regenerate or delete the whole tree; L1 doesn't depend on it.

**Verification:**

```bash
cd clients/laravel-sdk
XDEBUG_MODE=off composer test          # if a test suite exists
XDEBUG_MODE=off php -l src/Client/Resources/Processes.php
```

**Estimated scope:** ~250 LoC across 8 files.

---

## L2 — Laravel: regenerate or delete `Generated/Endpoint`

`clients/laravel-sdk/src/Generated/Endpoint/` is jane-openapi output against the **old** `/api/admin/*` and `/api/sdk/*` paths that no longer exist. Hand-written `Resources/` call `$this->http->{verb}('/api/{aggregate}/...')` directly, so the generated tree is dead weight.

**Decide one path:**

- **Path A — regenerate.** Run `jane-openapi` against the current `/q/openapi` document (platform must be running). Update `Resources/*.php` to use the regenerated endpoint classes for type safety. Adds compile-time-checked DTOs for request/response shapes.
- **Path B — delete.** Remove `src/Generated/` entirely. `Resources/*.php` already work without it. Lose nothing real; reduce surface and end the misleading file tree.

Recommend Path B unless you specifically want jane-generated DTOs for request validation.

**Estimated scope:** 1-2 hours either path.

---

## R1 — Rust SDK: `dispatch_jobs.rs`

**Why:** highest-value missing aggregate. Most consumers want to list/inspect/retry jobs.

**File to create:** `crates/fc-sdk/src/client/dispatch_jobs.rs`. Mirror `event_types.rs` for shape.

**Types:**

- `DispatchJobResponse` — id, externalId, source, kind, code, subject, eventId, correlationId, targetUrl, protocol, serviceAccountId, clientId, subscriptionId, dispatchPoolId, mode, status, maxRetries, retryStrategy, scheduledFor, expiresAt, attemptCount, lastAttemptAt, completedAt, durationMillis, lastError, idempotencyKey, createdAt, updatedAt. Match exactly what `crates/fc-platform-messaging/src/dispatch_job/api.rs` returns.
- `DispatchJobAttemptResponse` — id, dispatchJobId, attemptNumber, status, responseCode, responseBody, errorMessage, errorStackTrace, errorType, durationMillis, attemptedAt, completedAt, createdAt.
- `DispatchJobListResponse` — `{ items, totalItems, page, size }` per CLAUDE.md's documented contract for dispatch jobs.
- `CreateDispatchJobRequest`, `BatchCreateDispatchJobsRequest` (Vec wrapper), filter options struct.

**Methods on `FlowCatalystClient`:**

- `list_dispatch_jobs(filters: &DispatchJobFilters)` → `DispatchJobListResponse`
- `get_dispatch_job(id: &str)` → `DispatchJobResponse`
- `get_dispatch_job_raw(id: &str)` → `serde_json::Value` (raw payload — separate permission `can_read_dispatch_jobs_raw`)
- `get_dispatch_job_attempts(id: &str)` → `Vec<DispatchJobAttemptResponse>`
- `get_dispatch_jobs_for_event(event_id: &str)` → `Vec<DispatchJobResponse>`
- `dispatch_job_filter_options()` → `DispatchJobFilterOptions`
- `create_dispatch_job(req: &CreateDispatchJobRequest)` → `DispatchJobResponse`
- `batch_create_dispatch_jobs(req: &BatchCreateDispatchJobsRequest)` → batch result
- `list_dispatch_jobs_raw(filters: &DispatchJobFilters)` → list with payloads (separate permission)

**Wire:** add `pub mod dispatch_jobs;` and `pub use dispatch_jobs::*;` to `client/mod.rs`.

**Verification:**

```bash
cargo check -p fc-sdk --features client
cargo test -p fc-sdk --features client
```

**Estimated scope:** ~250 LoC, single file.

---

## R2 — Rust SDK: `scheduled_jobs.rs`

**Why:** TS and Laravel both ship this aggregate. Rust is the outlier.

**File:** `crates/fc-sdk/src/client/scheduled_jobs.rs`.

**Types** (mirror `crates/fc-platform-scheduled-jobs/src/scheduled_job/api.rs` responses): `ScheduledJobResponse`, `ScheduledJobInstanceResponse`, `ScheduledJobInstanceLogResponse`, `CreateScheduledJobRequest`, `UpdateScheduledJobRequest`, `FireScheduledJobRequest`, `LogInstanceRequest`, `CompleteInstanceRequest`. Enums: `ScheduledJobStatus`, `TriggerKind`, `InstanceStatus`, `CompletionStatus`, `LogLevel`.

**Methods on `FlowCatalystClient`:** 15 total mirroring the platform routes:

- `create_scheduled_job`, `list_scheduled_jobs(filters)`, `get_scheduled_job(id)`, `update_scheduled_job(id, req)`, `delete_scheduled_job(id)`, `get_scheduled_job_by_code(code)`, `pause_scheduled_job(id)`, `resume_scheduled_job(id)`, `archive_scheduled_job(id)`, `fire_scheduled_job(id, req)`, `list_scheduled_job_instances(id, filters)`, `get_scheduled_job_instance(instance_id)`, `list_instance_logs(instance_id)`, `log_instance(instance_id, req)` (SDK callback), `complete_instance(instance_id, req)` (SDK callback).

**Note:** the two "SDK callback" paths (`log_instance`, `complete_instance`) are what consumer apps call when running a fired job. They have a different permission (`platform:application-service:scheduled-job-instance:write`).

**Optional companion (defer to a sub-workstream):** `crates/fc-sdk/src/runner/` tree mirroring `clients/typescript-sdk/src/runner/`. Includes a `ScheduledJobRunner` + `LockProvider` trait + `InMemoryLockProvider`. Adds ~400 LoC. Worth doing in the same session if and only if there's a concrete Rust consumer asking for it.

**Estimated scope:** ~300 LoC for HTTP methods only; ~700 LoC if runner included.

---

## R3 — Rust SDK: `events.rs`

**Why:** event ingest is the most-trafficked SDK path and Rust consumers currently have to call `reqwest` directly.

**File:** `crates/fc-sdk/src/client/events.rs`.

**Types:** `EventResponse`, `EventListResponse`, `CreateEventRequest`, `BatchCreateEventsRequest`, `EventFilterOptions`. The `EventRequest` shape is the CloudEvents v1 envelope — match `crates/fc-platform-messaging/src/event/api.rs` exactly so the platform's validation accepts SDK-generated bodies.

**Methods:**

- `create_event(req: &CreateEventRequest)` → `EventResponse`
- `batch_create_events(req: &BatchCreateEventsRequest)` → batch result with per-event status; the platform returns 207-style partial-success bodies
- `list_events(filters: &EventFilters)` → `EventListResponse`
- `get_event(id: &str)` → `EventResponse`
- `list_events_raw(filters: &EventFilters)` → `Vec<EventResponse>` with full payloads (separate permission `can_read_events_raw`)
- `event_filter_options()` → `EventFilterOptions`

**Important:** batch ingest is documented in `CLAUDE.md` as exempt from the UoW invariant on the platform side (it's infrastructure ingest). The SDK side just needs to accept partial-success responses gracefully — don't promote a 207 with some failed events into a hard error.

**Estimated scope:** ~200 LoC.

---

## R4 — Rust SDK: expand `audit_logs.rs`

Currently 2/10 methods. Add the missing 8:

- `list_audit_log_entity_types()` → `Vec<String>`
- `list_audit_log_operations()` → `Vec<String>`
- `list_audit_logs_for_entity(entity_type: &str, entity_id: &str)` → list
- `list_audit_logs_for_principal(principal_id: &str)` → list
- `list_recent_audit_logs(limit: Option<u32>)` → list
- `list_audit_log_application_ids()` → `Vec<String>`
- `list_audit_log_client_ids()` → `Vec<String>`
- `batch_audit_logs(req: &BatchAuditLogsRequest)` — SDK ingest path

**Estimated scope:** ~100 LoC.

---

## R5 — Rust SDK: small per-aggregate expansions

One session of mop-up across the partial aggregates:

**`principals.rs`** — add: `delete_principal(id)`, `send_password_reset(id)`, `check_email_domain(email)`, `get_application_access(principal_id)`, `set_application_access(principal_id, req)`, `get_available_applications(principal_id)`.

**`dispatch_pools.rs`** — add: `archive_dispatch_pool(id)` (POST `/{id}/archive`).

**`roles.rs`** — add: `get_roles_by_source(source: &str)`, `get_role_filter_applications()`.

**`sync.rs`** — add the missing per-category sync wrappers if you want them centralised here (otherwise leave them on per-resource files): `sync_principals`, `sync_scheduled_jobs`, `sync_openapi`. **Decision required** during the session: do we want `sync.rs` to mirror `Sync/DefinitionSynchronizer.php` (single entry point) or keep `sync_*` on per-resource files (current pattern)?

**Estimated scope:** ~150 LoC.

---

## R6 — Rust SDK: sync orchestrator (`crates/fc-sdk/src/sync/`) — ✅ DONE (2026-05-15)

Landed `crates/fc-sdk/src/sync/` with six modules: `definitions.rs`
(`RoleDefinition`, `EventTypeDefinition`, `SubscriptionDefinition` +
`SubscriptionBinding`, `DispatchPoolDefinition`, `PrincipalDefinition`,
`ProcessDefinition` — each with `make()` + `with_*` builders and a
crate-private `into_wire()` conversion), `definition_set.rs`
(`DefinitionSet::for_application(code).add_*/with_*`), `options.rs`
(`SyncOptions::{defaults,with_remove_unlisted,roles_only,…,processes_only}`),
`result.rs` (`SyncResult` + `CategorySyncResult` with
`has_changes/has_errors/errors/totals`), `synchronizer.rs`
(`DefinitionSynchronizer::{sync,sync_all}` — fail-soft per category),
and `mod.rs` (public re-exports).

Also in scope:
- **Bug fix in `client/sync.rs`**: `SyncDispatchPoolsRequest.dispatch_pools`
  → `pools` (the platform's app-scoped endpoint expects `{ pools: [...] }`,
  not `{ dispatchPools: [...] }`). The previous Rust SDK would have failed
  on every dispatch-pool sync.
- **`sync_principals` HTTP method added to `client/sync.rs`** — required
  by the orchestrator and a genuine SDK gap.
- **Doc clarification on `client/sync.rs`**: it's the low-level
  per-category layer; the bundled orchestrator lives at `fc_sdk::sync`.

Categories shipped: roles, event_types, subscriptions, dispatch_pools,
principals, processes. **scheduled_jobs deferred** until R2 lands the
`sync_scheduled_jobs` HTTP method (it has a different wire response
shape — `{ created: Vec<String>, updated: Vec<String>, archived: Vec<String> }`
— and depends on `scheduled_jobs.rs`).

Filesystem reflection skipped per the recommended design — programmatic
builder use only. 17 unit tests cover builder→wire serialization, fluent
chaining, fail-soft error capture, and `sync_all` ordering. `cargo check
--workspace` clean.

**The biggest single architectural gap.** TS and Laravel both ship a `DefinitionSynchronizer` that takes a bundled `DefinitionSet` and pushes it in one orchestrated call. Rust requires the caller to manually call each per-category `sync_*` HTTP method.

**Why this is one session, not a sprint:** ~600 LoC, but mostly mechanical — DTOs mirror existing platform sync request shapes, the orchestrator is a thin loop over categories.

**New tree:**

- `crates/fc-sdk/src/sync/mod.rs` — re-exports.
- `crates/fc-sdk/src/sync/definitions.rs` — `RoleDefinition`, `EventTypeDefinition`, `SubscriptionDefinition`, `DispatchPoolDefinition`, `PrincipalDefinition`, `ProcessDefinition`, `ScheduledJobDefinition`. Each impls `Serialize` matching the platform sync wire shape and has builder constructors (`make()`, `with_description()`).
- `crates/fc-sdk/src/sync/definition_set.rs` — `DefinitionSet { application_code, roles, event_types, subscriptions, dispatch_pools, principals, processes, scheduled_jobs }` + `DefinitionSetBuilder` for programmatic assembly. Mirror TS `sync/definitions.ts::DefinitionSetBuilder`.
- `crates/fc-sdk/src/sync/synchronizer.rs` — `DefinitionSynchronizer { client: FlowCatalystClient, options: SyncOptions }` with `async fn sync(set: &DefinitionSet) -> SyncResult` and `sync_all(sets: &[DefinitionSet]) -> Vec<SyncResult>`. Calls the seven per-category `/api/applications/{app}/.../sync` endpoints in deterministic order: roles → event_types → subscriptions → dispatch_pools → principals → processes → scheduled_jobs. Each step is fail-soft (errors collected in `SyncResult`, not propagated).
- `crates/fc-sdk/src/sync/result.rs` — `SyncResult { application_code, roles, event_types, subscriptions, dispatch_pools, principals, processes, scheduled_jobs }` each `Option<CategorySyncResult>`.
- `crates/fc-sdk/src/sync/options.rs` — `SyncOptions { dry_run, remove_unlisted, sync_roles, sync_event_types, ... }` with `defaults()` and `all()` factories.

**Wire:** `pub mod sync;` to `lib.rs`. Re-export the public types.

**Design decisions to make during the session:**

1. **Filesystem reflection.** PHP scans classes for `#[AsEventType]` attributes. Rust has no equivalent built-in. Options: (a) skip filesystem discovery entirely, require programmatic builder use (matches TS); (b) use the `inventory` crate for compile-time registration via `submit!` macros. **Recommendation: (a)** — keep the surface predictable; `inventory` adds a magic dependency.
2. **Async vs blocking surface.** Match the rest of `fc-sdk::client` — async only.
3. **Error handling.** Per-category errors collected, not propagated. Caller inspects `SyncResult` to find what failed. Same as TS/Laravel.

**Estimated scope:** ~600 LoC across 6 files, plus tests.

---

## R7 — Rust SDK: `oauth_clients.rs` + `service_accounts.rs`

Two related missing aggregates, paired because both are auth-adjacent and have similar shape (CRUD + activate/deactivate + secret rotation).

**`oauth_clients.rs`:** verify routes first — `MEMORY.md` claims `/api/oauth-clients/*` but actual location may be `auth/oauth_clients_api.rs`. Methods: `create_oauth_client`, `list_oauth_clients`, `get_oauth_client(id)`, `get_oauth_client_by_client_id(client_id)`, `update_oauth_client(id, req)`, `delete_oauth_client(id)`, `activate_oauth_client(id)`, `deactivate_oauth_client(id)`, `regenerate_oauth_client_secret(id)`, `rotate_oauth_client_secret(id)` (verify both exist or only one).

**`service_accounts.rs`:** Methods: `create_service_account`, `list_service_accounts`, `get_service_account(id)`, `get_service_account_by_code(code)`, `update_service_account(id, req)`, `delete_service_account(id)`, `update_service_account_auth_token(id, req)`, `regenerate_service_account_auth_token(id)`, `regenerate_service_account_signing_secret(id)`, `get_service_account_roles(id)`, `set_service_account_roles(id, req)`.

**Estimated scope:** ~300 LoC across two files.

---

## R8-R10 — Rust SDK long tail

Bundle in 1-2 sessions when a consumer asks. Low priority because most apps don't manage IdPs, CORS origins, or login attempts via the SDK.

**R8:** `identity_providers.rs`, `email_domain_mappings.rs`, `anchor_domains.rs`.

**R9:** `auth_configs.rs`, `idp_role_mappings.rs`, `cors.rs`, `login_attempts.rs`.

**R10:** `password_reset.rs`, `public.rs`.

**Estimated scope:** ~150 LoC per aggregate; bundle 3-4 per session.

---

## T1 — TypeScript SDK: webhook validator

**Why:** TS is the only SDK without an HMAC validator. Consumers either reimplement it or skip signature checks.

**File:** `clients/typescript-sdk/src/webhook.ts`. Mirror `crates/fc-sdk/src/webhook.rs` API:

```ts
export class WebhookValidator {
    constructor(private secret: string, private toleranceSeconds = 300) {}
    validate(timestamp: number, payload: string, signature: string): Result<true, WebhookError>;
    validateRequest(req: Request): ResultAsync<true, WebhookError>;
    computeSignature(timestamp: number, payload: string): string;
    static fromEnv(): Result<WebhookValidator, WebhookError>;
}
```

Use `crypto.subtle` (Web Crypto, works in Node and browsers) for HMAC-SHA-256. Return `neverthrow` `Result` types to match the rest of the SDK.

**Cross-test:** hash the same payload with all three SDKs (Rust + Laravel + new TS) and assert byte-for-byte equality.

**Estimated scope:** ~80 LoC + tests.

---

## T2 — TypeScript SDK: JWKS verifier

**File:** `clients/typescript-sdk/src/jwks.ts`. Mirror `crates/fc-sdk/src/auth/jwks.rs`:

```ts
export class JwksCache {
    constructor(private config: { ttlSeconds: number });
    fetchKeys(issuerUrl: string): ResultAsync<JwkSet, JwksError>;
    invalidate(issuerUrl: string): void;
}

export class TokenValidator {
    constructor(private config: TokenValidatorConfig);
    validate(token: string): ResultAsync<AccessTokenClaims, TokenError>;
    validateBearer(authHeader: string): ResultAsync<AccessTokenClaims, TokenError>;
}
```

Use `jose` npm package for RS256. Cache keys per issuer with TTL.

**Estimated scope:** ~250 LoC + tests.

---

## T3 — TypeScript SDK: missing aggregates

Mirror R1/R3/R4 + service_accounts for the TS SDK:

- `clients/typescript-sdk/src/resources/dispatch-jobs.ts`
- `clients/typescript-sdk/src/resources/events.ts`
- `clients/typescript-sdk/src/resources/audit-logs.ts`
- `clients/typescript-sdk/src/resources/service-accounts.ts`

Wire each into `src/resources/index.ts` and `src/client.ts`. Generated bindings come from `pnpm generate` against the live platform.

**Estimated scope:** ~600 LoC across four files.

---

## P1 — Laravel SDK: JWKS verifier + outbound OIDC token manager

**Files:**

- `clients/laravel-sdk/src/Auth/Support/JwksCache.php` — TTL-bounded per-issuer cache. Use `firebase/php-jwt` or `web-token/jwt-framework` for RS256.
- `clients/laravel-sdk/src/Auth/Support/TokenValidator.php` — `validate(string $token): AccessTokenClaims` + `validateBearer(string $header): AccessTokenClaims`.
- `clients/laravel-sdk/src/Auth/Support/OidcTokenManager.php` — outbound client-credentials token acquisition with caching, mirroring `clients/typescript-sdk/src/auth.ts`.

**Estimated scope:** ~400 LoC + composer dependency addition.

---

## Backlog (not yet workstreams)

- TypeScript SDK: outbox concrete drivers (currently interface-only, BYO).
- Rust SDK: scheduled-job runner crate companion to R2 (`crates/fc-sdk/src/runner/`).
- Cross-SDK: shared CloudEvents request struct generation from a single source.
- OpenAPI codegen: pin a single codegen pipeline so all three SDKs regenerate from one CI step. Today TS uses `openapi-typescript`/`@hey-api/openapi-ts`, Laravel uses `jane-openapi`, Rust has no codegen at all.

## Conventions for every Rust SDK workstream

1. Module file in `crates/fc-sdk/src/client/<aggregate>.rs`. Wire via `pub mod`/`pub use` in `client/mod.rs`.
2. Request/response structs serialize as camelCase (`#[serde(rename_all = "camelCase")]`).
3. Methods live as `impl FlowCatalystClient { … }` extensions in the aggregate's file (mirror `event_types.rs` pattern).
4. Use the existing helpers: `get`, `post`, `put`, `delete_req`, `delete_with_response`, `post_action`, `post_empty`, `put_empty`. Add a new helper only if no existing one fits.
5. 204 responses: use `*_empty` helpers, return `Result<(), ClientError>`.
6. Verify with `cargo check -p fc-sdk --features client` before declaring done.
7. Update `docs/sdk-parity-plan.md` (this file) to mark the workstream complete.

## Memory updates required after each workstream

When a workstream lands, append a one-line note to `feedback_three_sdk_parity.md` confirming all three SDKs were updated (or that the workstream was scoped to one SDK by design — Laravel-only or Rust-only). Do not let the parity feedback go stale.

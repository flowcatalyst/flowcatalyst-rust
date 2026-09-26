# API parity, area D: subscriptions, connections, dispatch pools, router-config

Branch `feat/api-area-d`, off `main` at `f7580210`. Scenario groups: `subscriptions/*`, `connections/*`,
`dispatch-pools/*`, `code-first-connections/*`, `router-config/*`, `smoke/*`, run together in one harness
run (`fc-parity --only '{subscriptions,connections,dispatch-pools,code-first-connections,router-config,smoke}/*'`,
Go `73a6918`, debug Rust builds). "Before" is `main` built unmodified; "after" is this branch.

| scenario file | before OK / ACC / DIFF / ERR | after OK / ACC / DIFF / ERR |
|---|---|---|
| code-first-connections | 4 / 0 / 3 / 3 | 6 / 3 / 1 / 0 |
| connections | 15 / 0 / 18 / 1 | 30 / 1 / 3 / 0 |
| dispatch-pools | 17 / 0 / 19 / 0 | 35 / 0 / 1 / 0 |
| router-config | 8 / 0 / 5 / 0 | 8 / 1 / 4 / 0 |
| smoke | 12 / 0 / 6 / 0 | 12 / 0 / 6 / 0 |
| subscriptions | 17 / 0 / 18 / 0 | 33 / 1 / 1 / 0 |
| **total (146 steps)** | **73 / 0 / 69 / 4** | **124 / 6 / 16 / 0** |

## What changed

**Subscriptions** (`subscription/api.rs`, `operations/*`, `repository.rs`):
- `GET /api/subscriptions` returns every status; `status` and `clientId` are filters (Go `FindWithFilters`), then
  Go's `FilterClientScoped`. It used to return ACTIVE rows only.
- Create and update take Go's whole request. Update used to drop the event types, mode, pool, service account,
  dataOnly, queue, delay and max age; now each is applied (mode per X-01: unknown means `NEXT_ON_ERROR`). `queue`
  is DEFAULT or HIGH_PRIORITY in any case, stored upper-case, and an explicit blank clears it on update, as Go.
  `sequence` stays server-side, as Go. Bindings keep the `eventTypeId` and `specVersion` the SPA sends. `dataOnly`
  defaults to true, as Go. An empty update is a no-op write (Go has no `NO_UPDATES`).
- Go's validation codes and messages: the code rule `^[a-z][a-z0-9-]*$`, `INVALID_ENDPOINT` for a non-http(s)
  endpoint (Rust accepted `not-a-url`), `INVALID_QUEUE`, `EVENT_TYPES_REQUIRED`; duplicates 409 `CODE_EXISTS`
  within the `(application, client, code)` key.
- Pause and resume answer 204 and are unconditional (a repeat is a no-op write that still records its event); they
  used to answer 200 with the row, and a repeated resume was 409 `ALREADY_ACTIVE`.
- The response is Go's: unset members omitted rather than `null`, `source` (was always `null`) and `createdBy`.
- Scope: every write checks Go's `CheckScopeAccess` (403 `SCOPE_FORBIDDEN`, "no access to this resource's client"
  / "anchor scope required for this resource"). A non-anchor could pause a platform subscription before; it cannot
  now, as Go.
- The application sync (`POST /api/applications/{appCode}/subscriptions/sync`) takes `clientId` (id or
  identifier, 404 when unknown), scopes its rows to `(application, client)`, and resolves `connectionCode` within
  an explicit namespace (`sharedConnection`) with Go's `CONNECTION_NOT_FOUND`, `CONNECTION_MISMATCH`,
  `CONNECTION_SCOPE_MISMATCH` and `SHARED_CONNECTION_REQUIRES_CODE`. Rust ignored all four fields, so a
  code-first subscription was created with no connection and a shared connection was never refused. The owner
  rulings stay: the sync honours `mode` (SUB-7) and rejects unknown pool codes.

**Connections** (`connection/*`):
- Create returns the stored connection (201), as Go; the SPA puts it straight into a select. It returned `{id}`.
- Writes need the connection permission plus Go's per-resource scope, not anchor scope: a client administrator
  manages its own client's connections (`b-can-create-own` was a 403).
- The entity carries `applicationCode` and `source` (Go 056's columns, already in migration 050); the response
  shows them and omits unset members; an optional `applicationCode` on create/update is resolved within the
  caller's application scope (out of scope is the same 404 as missing, per the owner ruling). Duplicates are 409
  `CODE_EXISTS` in the `(application, client, code)` key; `SERVICE_ACCOUNT_REQUIRED` as Go. `PUT` requires the
  name and replaces description and externalId as sent, as Go.

**Dispatch pools** (`dispatch_pool/*`):
- The list returns every status (it returned ACTIVE only without a client filter), and a `clientId` filter no
  longer adds the platform pools.
- Archive, suspend and activate answer 204, as Go (and as the SPA's wrappers already expect).
- Go's validation: the pool-code rule `^[a-z][a-z0-9_-]*$`, `INVALID_CONCURRENCY` (below 1),
  `INVALID_RATE_LIMIT` (negative, was a serde `VALIDATION`); Rust accepted `Bad Code` and a concurrency of 0,
  which then showed up in the router-config document as `platform-Bad Code` with concurrency 0.
- Writes check Go's scope after `CanWriteDispatchPools` (kept); delete no longer needs anchor scope on top of the
  permission, as Go.

**Shared**: `caller_reach::check_scope_access` is Go's `CheckScopeAccess`, used by all three. The Rust SDK's pool
archive/suspend/activate now read the pool back after the 204 (they failed to decode against Go and Rust alike).

**Migration**: `053_subscription_created_by` adds Go 035's `msg_subscriptions.created_by` (nullable, `IF NOT
EXISTS`; a Go-migrated database already has it, and its probe marks it applied).

## Ruled (expected-diffs.json, #31 provisional)

- `code-first-connections`: Go's connection and subscription syncs fail with 500 `AUDIT_WRITE` for any application
  code longer than 17 characters (`aud_logs.entity_id VARCHAR(17)`; the rollup's audit row is keyed by the
  application code, `cfc-app-<run>` is 20). Rust widened the column in migration 038 (Java V18). The three sync
  steps are `!go-expect`; the missing synced rows cascade into `connections/list-by-status` and
  `subscriptions/list-by-status`, which accept their list and total. In production this bites any application
  whose code exceeds 17 characters.
- `router-config`: Rust lists every queue the scheduler can publish to (both priorities, every client's tenant),
  decision #31's router-config case; the entry also covers `queueUri`, which names each side's own database.

## What remains (16 DIFF), and whose it is

- `login-as-b` (connections, dispatch-pools, subscriptions, smoke): Rust's `platform:messaging-admin` carries the
  eight `platform:function:*` permissions Go lacks. Role catalogue (area a); functions follow Java by the
  Direction, so this likely wants an owner-cited entry rather than a code change.
- `create-service-account`, `create-service-account-b`, `create-shared-signer`, and router-config's
  `provision-router-service-account`, `router-token`, `router-service-account`: the service-account create/read
  shape (`principalId` is the account id, `null` members, `sub`). Area c.
- router-config `router-role-exists`: `shortName` on the role. Area a.
- smoke `get-created`, `get-updated`, `list`, `cross-tenant-read`: the event-type response (`createdBy`,
  `eventName` vs `event`, `source`, `null` description). Area b (event-types).
- smoke `health`: `/version` is Go's build-time version (`dev` in the harness build) vs Rust's crate version. No
  behaviour; needs an owner call on what Rust should report.

Not changed on purpose: a connection delete still refuses one a subscription uses (Rust's guard; Go deletes it and
orphans the subscription), and pool/connection creates still do not store the client identifier (Go does not
either; the router-config pool code composes with it).

## App impact

- SPA (Go's): connection create now returns the connection; subscription and pool lists show paused, suspended and
  archived rows; the 204 answers are what its wrappers already expected.
- Laravel SDK apps (integral, hr, rfp): code-first subscription syncs by `connectionCode`/`clientId` now resolve as
  on Go; pause/resume and pool status flips answer 204 as Go.
- Delivery: `fc-delivery-harness --only plain-events,next-on-error-group,burst-pool-capacity` passes on both sides.

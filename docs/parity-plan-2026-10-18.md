# Rust ↔ Go parity plan (2026-10-18)

The owner's goal (2026-10-18) is to keep this platform **ready to take over** from the Go platform
(`../flowcatalyst-go`). This plan lists what Rust lacks as of Go `a87be6e1` (2026-10-18) and Rust `0440b0ac`
(2026-10-14, the "park the Rust port" point), and how to close the gap.

This is a planning document. Nothing here is implemented. It supersedes nothing: `docs/cutover-checklist.md`
and `docs/go-mirror/*` stay authoritative for the work they record.

**Evidence markers:** **[R]** = read in code, SQL or OpenAPI. **[S]** = taken from the spec diff run for this
plan (below). **[D]** = taken from a Go or Rust doc only. **[I]** = inferred, not verified.

## 0. How this was measured

**Spec diff [S].** Rust's document was dumped with the existing contract test, built in a scratch target dir
with no database:

```
CARGO_TARGET_DIR=<scratch> OPENAPI_DUMP=<scratch>/rust-openapi.json OPENAPI_REPORT=1 \
  cargo test -p fc-platform --test it -- openapi_go_contract_test::shared_operations_carry_gos_operation_ids
```

That test **passes today only because it compares against a stale copy.** `frontend/openapi/openapi.json` is Go's
lock from 2026-09-25. Diffed against Go's **current** `api/openapi.lock.json` (paths normalised `{x}`→`{}`, by
method + path):

| | Count |
|---|---|
| Go operations | 309 |
| Rust operations | 296 |
| Shared | 274 |
| Go-only | 35 |
| Rust-only | 22 |
| `operationId` mismatches | 18 (all function operations, plus `POST /api/dispatch-jobs`) |
| Go component schemas missing from Rust | 63 |

**Other sources:**
- Go ledger `docs/platform-bug-fixes.md` (F1–F26), read in full for F11 and F17–F26.
- Rust `docs/go-mirror/*` (eight batches).
- Both migration sets, compared pairwise.
- Both function runners.
- Go `git log --since=2026-10-01`.
- Java's `../flowcatalyst-javalin/docs/go-mirror/PENDING.md`. It is the model for §6: Java paused 2026-10-16 with F20, F21 and fn.dispatch unmirrored.

## 1. Current state per area (Rust vs Go)

| Area | Rust today | Gap vs Go |
|---|---|---|
| **IAM / OIDC** | Go's token claims, session cookie, F16 CSRF and SSO binding, 2FA, passkeys, portal, the role and permission ceilings F8/F10/F12/F15 (`fc-platform-iam`, `fc-platform-auth`). The auth-code grant already refuses an inactive principal (`oauth_api.rs:1377`) [R]. | Missing: F18 role-name validation and `iam_roles.application_id`; F21 confined role reads; F20 application users and invitations; F25 `fcdev-cli` client, loopback redirect, absolute refresh TTL, `refresh_expires_in`; F26 role; last-all-applications-admin guard (Go `9966a2fa`). |
| **Messaging / router** | Mirrored to about Go 2026-10-04: SQS batching and lingers, mediation policy, buffers [R/I]. | Missing (Go 10-08/09):<br>• missing-SQS-queue tolerance (`c52d1150`)<br>• urgent poll-loop acks: Rust `fc-queue/src/sqs.rs:1056` and `manager/routing.rs:751` use non-urgent `ack` [R]<br>• HTTP/2 PING keepalive with `FC_ROUTER_HTTP2_{READ_IDLE,PING}_TIMEOUT_SECONDS`: none in `mediator/inner.rs` [R]<br>• NATS pull expiry (`c6b534d6`) [I]<br>• dashboard modal (`5cbe502e`)<br>• Go's 09-22 HOL reservation schedule: Rust defers by a fixed `CAPACITY_DEFER_SECONDS` [R] |
| **Scheduler / dispatch jobs** | Lanes, drain loop, paused-in-claim, queue-table retirement: level with Go to 10-05 [R]. | Missing:<br>• `connection_id` on jobs and paused-connection claim (F24, Go 082)<br>• `msg_scheduled_job_instances.not_before` (Go 060): Rust would fire Go-deferred 429 instances early<br>• requeue-by-filter (`POST /api/dispatch-jobs/requeue-by-filter[/preview]`)<br>• metric names are dotted, not `fc_scheduler_*` |
| **Outbox / stream** | Level. No Go outbox or stream changes since 10-01 [R]. | none |
| **Connections / pools** | Code-first sync and client identifier (F5). | Missing:<br>• F17: the sync copies `app_applications.service_account_id` (a `prn_` id) into `msg_connections.service_account_id` (`fc-platform-messaging/src/connection/operations/sync.rs:119`) [R]. Rust's signer tolerates it because `ServiceAccountRepository::find_by_id` matches prn_ or sac_ [R], but Go, on the same DB, expects `sac_`.<br>• F19 pool ownership (`msg_dispatch_pools.application_id`, `DISPATCH_POOL_NOT_OWNED`, app-service `dispatch-pool:sync`). |
| **Functions** | A separate design, ported from **Java**:<br>• WASI 0.2 components on wasmtime 49 (`fc-fnhost-core`), V8 JS (`fc-fnhost-js`)<br>• Sigstore signing (`fc-function-signing`)<br>• `fnr_*` tables (migration 062)<br>• three-label addresses `app.service.fn`<br>• Java-wire control routes `/control/functions/{desired-state,heartbeat,events,artifacts/{versionId}}` [R] | Wholly incompatible with Go's core-module ABI v1 on wazero, its `fng_*` tables, two-label addresses, `/api/functions/{id}` API and `/control/functions/{desired,heartbeat,events,artifacts/{digest},dispatch-jobs}`. No `fn.dispatch` anywhere [R]. See §4. |
| **Control route `POST /control/functions/dispatch-jobs`** | absent [R/S] | F23/F24: pool and connection resolution, `connection_id`, describe `dispatches[]` |
| **fcdev / CLI** | `fc-dev fn` has init/build/publish/deploy/promote/invoke/validate/config/secret only, against the Rust function API (`bin/fc-dev/src/fn_cli/mod.rs`) [R]. Shares Go's embedded-PG cluster (`docs/cutover-checklist.md`). | No delete/dev/run/describe/status/set, profiles, login, or CI flags. Go's F25 says "the fcdev CLI is Go-only". See decision D6. |
| **Console SPA** | Go's SPA vendored at Go `73a6918` (2026-09-25) plus hand-ported fixes (`frontend/PROVENANCE.md`) [R]. Own function pages. | Missing Go SPA commits:<br>• requeue-by-filter `5496c3ff`<br>• self-restriction confirm `01fead7e`<br>• **F8/F10 SPA locks `90e41132`, `5a712d75`** (only the backend halves were mirrored)<br>• credential confirm `c38933f0`<br>• ConfirmDialog width `77d35c9c`<br>• F20/F25 pages<br>• x-sdk regenerated types<br>• Go's function drawer |
| **OpenAPI** | Rust's `/q/openapi` documents Go's operationIds for 256 shared ops [S]. | 35 Go-only ops (§2.3). No `x-sdk` tags (Go `21f9734d`). 13 routes served but undocumented. |
| **Migrations** | 001–070. 001–030 are identical to Go once comments are stripped. Tracker `_schema_migrations` (`crates/fc-migrations`) [R]. | Go is at **082**. §3. |
| **Licence** | AGPL-3.0-or-later (`LICENSE`, `Cargo.toml`, `frontend/package.json`) [R] | Go moved to Apache-2.0 (`7baee1ce`). Decision D7. |

## 2. Gap table

Sizes: **S** under half a day, **M** about a day, **L** several days, **XL** more than a week.

### 2.1 Go ledger F1–F26

| Id | What Rust lacks | Size | Risk | Where in Rust |
|---|---|---|---|---|
| F1–F10, F12–F16 | Nothing: mirrored in batches 1–8 (`docs/go-mirror/2026-10-10…2026-10-14-*.md`). Exception: the **SPA halves of F8/F10** (role/permission locks, provisioning gate) [R by grep]. | S | low | `frontend/src/…` (port Go `90e41132`, `5a712d75`) |
| F11 | Not applicable while Rust keeps its own function events. It already emits `platform:function:*` with Java's data. If Rust adopts Go's function model (§4), it must also emit Go-only `platform:admin:function:promoted`, `function-schedule:wired`, `function-route:{set,deleted}`, `function-setting:*` (DB), and drop `runtime` from `function:created`. | S (with §4) | low | `fc-platform-functions/src/function/operations/events.rs`, seed catalogue |
| F17 | Sync must store the `sac_` id (`service_account_for_principal`). `APPLICATION_SERVICE_ACCOUNT_REQUIRED` exists already. `PUT /api/connections/{id}` needs an optional `serviceAccountId` (in Go's lock). Data fix = Go 075. | S | **shared-DB:** Rust writes `prn_` into rows Go then cannot sign with | `fc-platform-messaging/src/connection/{operations/sync.rs,api.rs}` |
| F18 | Missing:<br>• `INVALID_ROLE_NAME` (absent [R])<br>• shape validation at every assignment write<br>• service-account `assign_roles` existence check (its `validate` is empty [R])<br>• role create setting `application_id` (only `role/operations/sync.rs:216` sets it [R])<br>• `IsPlatform` by code<br>• data fixes 076/077 | M | medium (token contents) | `fc-platform-iam/src/{role,principal,service_account}/operations/*`, IdP mapping create |
| F19 | `msg_dispatch_pools.application_id` (Go 078); ownership rules in pool sync; `DISPATCH_POOL_NOT_OWNED`; sweep confined; `dispatch-pool:sync` in `application_service::ALL` (only the constant exists, `permissions.rs:66` [R]); grant migration (Go 079). | M | security (cross-app pool takeover) | `fc-platform-messaging/src/dispatch_pool/*`, `fc-platform-core/src/permissions.rs` |
| F20 | Six app-service permissions (Go 080); `AppScope` confinement on six event/dispatch-job reads; new `GET/POST /api/applications/{appCode}/users`, `GET …/users/{id}`, `GET …/invitations`, `POST …/invitations/{id}/resend`, `DELETE …/invitations/{id}` (6 ops, 4 schemas [S]). | L | security (confinement) | new `fc-platform-iam/src/appusers/`; `fc-platform-messaging/src/{event,dispatch_job}/api.rs` |
| F21 | Confinement of nine role/permission read routes for application-service callers; `APPLICATION_ACCESS_REQUIRED` on by-application. No migration. | M | security | `fc-platform-iam/src/role/api.rs` |
| F22 | Go's version FAILED state, publish-replay reset, heartbeat fencing (`requeued_at_revision`, Go 081), runner `RETRYING`. Rust versions have no FAILED state at all [R]. Applies only under §4. | M (with §4) | medium | function control plane |
| F23 + F24 | `describe.dispatches[]` `{target,path,connection,pool?}`; `POST /control/functions/dispatch-jobs`; pool and connection resolution rules; `msg_dispatch_jobs.connection_id` (Go 082); claim excludes paused connections (`PausedConnectionCache` + `PausedConnectionIDs`); signer: a subscription-less job is signed by its connection's account, never by its own `serviceAccountId`. | L | **behavioural on a shared DB:** Rust's scheduler would deliver Go fn jobs of a PAUSED connection, and sign them as the application | `fc-platform-messaging/src/{scheduler,dispatch_job/delivery_credentials.rs}`, function control |
| F25 (platform half) | Missing:<br>• seed and re-assert the public `fcdev-cli` client<br>• RFC 8252 loopback redirect matcher for that client only<br>• `FC_FCDEV_LOGIN_REFRESH_TTL_SECS` absolute refresh lifetime<br>• `refresh_expires_in` on `/oauth/token` (none [R])<br>The inactive-principal check is already present [R]. | M | medium (OAuth) | `fc-platform-auth/src/auth/oauth_api.rs`, seed |
| F26 | Built-in `platform:function-deployer` role (6 permissions). Only meaningful once Go's function permissions exist (`platform:admin:client:view` etc. already do). | S | low | role catalogue |

### 2.2 Go work outside the ledger (post-2026-10-01)

| Go change | Commit(s) | Rust | Size |
|---|---|---|---|
| Requeue-by-filter API (409 `FILTER_MATCH_CHANGED`) + SPA dialog | `165feb6d`, `5496c3ff` | MISSING [R] | M |
| Never demote the last all-applications admin (`LAST_ALL_APPLICATIONS_ADMIN`, `confirmSelfRestriction`) | `9966a2fa`, `01fead7e` | MISSING [R] | M |
| `x-sdk` app/admin tagging; spec stubs for plain handlers (`/api/me*`, `/oauth/*`, `/.well-known/*`, dispatch-jobs create/batch, audit-logs batch) | `21f9734d` | MISSING. The routes are served [R] but undocumented [S]. | M |
| Log every 5xx with its cause; artifact-store diagnostics; S3 startup self-check | `c0d27994` | Partial (`fc-platform-core/src/shared/error.rs:682`, only status 500); no self-check | S |
| Router items | listed in §1 | MISSING | 4×S |
| fn.dispatch capability, runner identity on recreate, `fcdev fn` delete/dev/profiles/CI/login | `5796e829`, `9d017689`, `4edf16a6`, `410b38c4`, `577643bc`, `8254cf0c` | MISSING / different design | see §4, D6 |
| SPA: credential confirm, ConfirmDialog width | `c38933f0`, `77d35c9c` | MISSING | S |
| Event type pruning differs. Go 071 also pruned `platform:iam:user:password-reset-requested`, which Rust emits and re-seeds. | Go 071 vs Rust 069 | divergent | S + D8 |
| Stale Rust docs:<br>• `docs/function-runner-plan.md` §9 ("Go has no function runner")<br>• `migrations/062` header ("Go uses `fn_`"; it is `fng_`)<br>• `docs/parity/router-*-vs-go.md` (09-28/29) | n/a | stale | S |

### 2.3 The 35 Go-only operations [S]

| Kind | Count | Operations |
|---|---|---|
| Served by Rust, undocumented | 13 | `GET /api/me`, `/api/me/applications`, `/api/me/clients`, `/api/me/clients/{clientId}`, `/api/me/clients/{clientId}/applications`; `POST /oauth/{token,introspect,revoke}`; `GET /oauth/userinfo`; `GET /.well-known/{jwks.json,openid-configuration}`; `POST /api/audit-logs/batch`; `POST /api/dispatch-jobs/batch` |
| F20 | 6 | listed in §2.1 |
| Requeue-by-filter | 2 | `POST /api/dispatch-jobs/requeue-by-filter` and `/preview` |
| Go function API | 9 | `PATCH /api/functions/{id}`; `GET`/`PUT /api/functions/{id}/routes`; `DELETE …/routes/{routeId}`; `GET …/settings`; `PUT`/`DELETE …/config/{key}`; `PUT`/`DELETE …/db/{name}` |
| Go control plane | 5 | `GET /control/functions/{desired,artifacts/{digest}}`; `POST /control/functions/{heartbeat,events,dispatch-jobs}`. Rust serves differently shaped routes and does not document them. |

The 18 `operationId` mismatches are the function routes that share a path shape. Rust's ids are
auto-generated (`getApiFunctionsByAddress`) where Go names them (`getFunction`).

## 3. Migration reconciliation (one database for both)

**Tracking** [R]:
- Go uses goose's `goose_db_version`, keyed by number. Its `internal/migrate/migrate.go` `bootstrap()` gives goose
  precedence. At step (2) it **seeds goose from `_schema_migrations` by numeric prefix**.
- Rust uses `_schema_migrations`, keyed by `NNN_name` plus a checksum. It ignores goose. On an empty tracker
  with `tnt_clients` present it backfills: it probes where a probe is defined and assumes applied otherwise.
- Every Rust migration from 031 onward is idempotent.

**Hazards:**
- **A (Go starting on a Rust-first database).** Go's step (2) maps Rust's numbers onto Go's. They diverge after
  030 (Rust 034 is functions, Go 059). Go would mark 059 applied, never create `fng_*`, and leave 040/045/048/049
  as missing. Fix **in Go**: limit step (2) to ≤030, or map by name. Size S, a Go-repo change.
- **B (Rust on a Go database older than Go 072).** Rust 067–070 have no probes and are assumed applied. Add probes.
  Size S.
- **C (Rust 070 against Go 072).** Rust 070 fixes 4 more schemas than Go 072 (`oauth-client:secret-rotated`,
  `admin:roles:synced`, `admin:subscription:synced`, `iam:user:logged-in`; the last is Rust-shaped). On a Go
  database they never land, because 070 is assumed applied. Reconcile the payloads first: either Go adopts them,
  or Rust's emitters match Go's.

**Schema differences on a Go database at 082 that Rust ignores** [R]:

| Go object (migration) | Rust today | Consequence |
|---|---|---|
| `fng_*`, 8 tables (059/061/062/064/081) | invisible; Rust creates `fnr_*` (062) beside them | Two function registries in one DB (§4) |
| `msg_dispatch_jobs.connection_id` (082) | unread | Paused connection not honoured; wrong signer (F24) |
| `msg_dispatch_pools.application_id` (078) | unread | No ownership (F19) |
| `msg_scheduled_job_instances.not_before` (060) | unread | 429-deferred instances fire early |
| `msg_subscriptions.function_id`, `msg_scheduled_jobs.function_id` (059) | unread; Rust uses `fnr_trigger_objects` | Function wiring invisible |
| `oauth_idp_role_mappings.idp_type` (035); defaults `mode='NEXT_ON_ERROR'` (048); `iam_login_attempts` partitioned (049; Rust code copes, `login_attempt/repository.rs:236`); ~34 enum CHECKs (051/052, Rust values fit [R spot-check]); read-projection indexes (036/037) | absent on a **fresh Rust** DB | Fresh Rust DB ≠ Go DB |
| Data fixes 075, 076, 077, 079, 080 | none | F17/F18/F19/F20 data |

**Rust-only objects** [R]:
- `app_client_configs.{base_url_override,config_json}` (058)
- `iam_principal_roles.assigned_by` (059)
- `iam_service_accounts.wh_*` (060)
- `msg_dispatch_jobs_read.queue` (061)
- `fnr_*` (062)

All are nullable or additive. Go uses explicit column lists, so they do not break Go.

**Role-catalogue hazard** [R]: Rust's boot seed only inserts missing roles, but `POST /bff/roles/sync-platform`
(`fc-platform-iam/src/shared/role_sync_service.rs`) reconciles CODE roles to **Rust's** catalogue. On a Go DB that
strips the 7 Go-only grants from `platform:application-service` (Go 079/080). Rust's catalogue must equal Go's
before anyone presses that button.

**Function table prefixes.** Owner ruling 2026-09-28: Go `fng_`, Rust `fnr_`, Java `fn_`, renamed back to `fn_`
when one implementation is chosen. The schemas are incompatible (two- vs three-label addresses; hex vs
`sha256:` digest; describe vs manifest; FAILED state; settings CONFIG/SECRET/DB vs `fnr_config`/`fnr_secrets`;
`function_id` columns vs trigger objects). No migration can turn one into the other. Options:
1. Rust adopts `fng_*` as the schema of record for the takeover. `fnr_*` is kept only for Rust-native components, or dropped.
2. Rust keeps `fnr_*` and migrates Go data into it. Lossy: it synthesises `service_name` and maps DSN settings to secretRefs.
3. Both coexist and every function is re-published.

**Recommendation: (1).** See D1–D2.

**Data-compatibility plan:**
1. Make Go's goose set the schema of record. Rust gains one migration per Go migration it lacks: 060, 075–082 data
   and DDL, plus the fresh-DB DDL for 035/036/037/048/049/051/052 and `fng_*` 059/061/062/064/081. All are
   `IF NOT EXISTS` and probe-guarded, with a mapping table `Go NNN → Rust MMM` in the header of each.
2. Fix hazards A, B and C.
3. A gate test (§5, G3) migrates three databases and compares their fingerprints (`pg_dump -s` normalised):
   - a scratch Go-082 database, migrated by Rust;
   - a fresh Rust database, migrated by Go;
   - a fresh Go database.

   Allowed difference: Rust-only additive objects only.
4. Bring Rust's role catalogue and event catalogue to Go's (079/080/F26; the 071 divergence, D8).

## 4. Function runner

**Guest ABI** [R]:
- **Go, ABI v1.** A core wasm module. Exports `fc_abi_v1`, `fc_alloc`, `fc_handle(ptr,len)->i64`,
  `fc_describe`. Imports only `fc.call(op,ptr,len)` and `fc.take`, plus `wasi_snapshot_preview1`. Frames are
  `u32 metaLen | JSON meta | raw body`. Eleven host ops: log, config, secret, http.fetch, event.emit, db.*,
  dispatch.create. The artifact **describes itself** (`fc_describe` at publish). JS runs on a shared QuickJS
  core module.
- **Rust.** A WASI 0.2 component exporting `wasi:http/incoming-handler`, with typed WIT imports
  `flowcatalyst:function@0.1.2` (config, secrets, log, events, invocation, db). The manifest is supplied by the
  publisher. There is no `emits` and no `dispatches`. **Core modules are refused** (`inspect.rs`
  `WASM_CORE_MODULE_UNSUPPORTED`). JS runs on V8.
- **Overlap:** none at the binary or wire level.

**Runner protocols** [R]:

| | Go | Rust (= Java) |
|---|---|---|
| Desired state | `GET /control/functions/desired?pool=&wait=`: long-poll ≤60 s, `LISTEN fng_desired`, ETag, nested functions→versions with roles, `revision`, DSNs | `GET …/desired-state?pool=`: 15 s poll, ETag, flat per-version entries, `artifactRef`, `signer`, `unload`; no revision |
| Heartbeat | `{runnerId, pool, revision, budget{…}, versions[{functionId, number, state LOADED\|COMPILED\|FAILED\|RETRYING}]}`. FAILED is fenced by `requeued_at_revision` (F22). | `{hostId, pool, state, loaded[{address, version, state}], runtimes[]}` |
| Runtimes filter | none; every runner runs wasm and js | hosts advertise `runtimes`; publish answers 409 `POOL_RUNTIME_UNSUPPORTED` |
| Artifacts / events / dispatch | `artifacts/{digest}` (302 presign); single event, b64; `dispatch-jobs` | `artifacts/{versionId}` stream, OCI/ECR; batch events; no dispatch |
| Auth | `platform:function:runner:control` | `platform:function:host:control` |

**Signing:** Go never signs (`function/operations/events.go:144`). Rust requires Sigstore bundles unless `FC_FN_SIGNATURES=off`.

**Paths:**
- **(a) Rust fnhost as a runner for the Go platform: L.** Two parts:
  - **A Go-protocol client in `fc-fnhost-core`: M.** Long-poll; revision tracking for the F22 fence;
    describe→internal manifest; DSNs; single-event emit; dispatch client; follow the 302.
  - **A Go-ABI core-module `FunctionLoader` on wasmtime: L.** This needs `wasmtime-wasi` preview1 (only p2 is
    enabled today), `fc.call`/`fc.take` as async `func_wrap`, the frame codec, Go's endpoint matching and
    caller meta, all eleven ops at Go semantics (http.fetch follows redirects with re-check), an instance pool,
    and Go's QuickJS engine module for `js`.

  Biggest risk: `fc_function_abi::FunctionAddress` is three labels across the whole host [I: wide refactor].
- **(b) Rust takes over the platform role and keeps Go-ABI artifacts: L–XL.** The platform side must serve
  Go's function API (`/api/functions/{id}`, routes, settings, PATCH, Go DTOs and operationIds) and Go's control
  plane, on `fng_*`, with:
  - describe at publish, which needs a sandboxed core-module instantiation (the (a) loader);
  - F22 requeue;
  - F23/F24 dispatch;
  - F11's Go-only events.

  Unsigned artifacts must be allowed.

- **(c) Cheapest takeover (recommended first step): M–L.** The **Rust platform serves Go's function control
  plane and API** (the (b) platform half, minus describe-in-process), and **Go's wazero runner keeps running**
  as the function pool. Go's runner is a separate process speaking HTTP, so it works against any platform that
  speaks its protocol [I from the protocol shape]. Describe at publish can call a small runner endpoint, or
  reuse the runner's describe path, instead of embedding wazero in Rust (D3).

  The Rust component host becomes optional: either a second runtime (`component`) behind the same control
  plane, or parked. (a) is only needed if the owner wants one language end to end.

## 5. Ordered implementation plan

Effort is in agent sessions, one bounded batch each, with a ledger per batch as in `docs/go-mirror/`.

| Phase | Work | Depends on | Effort | Parallel? |
|---|---|---|---|---|
| **P0 Baseline** | Refresh `frontend/openapi/openapi.json` to Go's current lock, so the contract test fails honestly and lists the 35. Probes for 067–070 (hazard B). Fix stale docs. Go-side hazard-A guard (a Go commit). | — | 1 | no |
| **P1 Schema** | Rust migrations for Go 060, 075–082 and the fresh-DB DDL (§3 step 1, without `fng_*` unless D1 says so). Role and event catalogues set to Go's (079/080/F26, D8). Schema-fingerprint gate. | P0 | 2 | split DDL / data |
| **P2 IAM & messaging fixes** | Five independent batches:<br>• **A:** F18 + F21 (roles)<br>• **B:** F17 + F19 (connections, pools)<br>• **C:** F20 (app users/invitations, after A for the `AppScope` helper)<br>• **D:** F25 platform half + F26<br>• **E:** requeue-by-filter + last-admin guard | P1 | A 1, B 1, C 2, D 1, E 1 | **yes:** A, B, D, E concurrently; C after A |
| **P3 Runtime behaviour** | F24 claim (paused connections) + signer rule; `not_before`; router S-items (urgent acks, missing queue, h2 PING + env, NATS expiry, dashboard); 5xx logging; S3 self-check. | P1 | 2 | yes, router vs scheduler |
| **P4 Functions (option c)** | Go function API and control plane on `fng_*` in `fc-platform-functions`: CRUD, routes, settings, domains, publish (hex digest, describe via D3), desired-state render + NOTIFY, heartbeat with F22 fence, events, `dispatch-jobs` (F23/F24). F11 events, F26. | P1, P3 (F24) | 6–8 | 2–3 agents: API / control / dispatch |
| **P5 Contract surface** | `x-sdk` tags and the 13 doc stubs; Go operationIds on function routes; re-vendor Go's SPA at the current commit and re-apply `frontend/PROVENANCE.md` deltas (this brings in F8/F10 locks, requeue, F20, credential confirm). | P2, P4 | 2 | yes |
| **P6 Optional** | (a): Go-ABI loader in `fc-fnhost-core`. | P4 | 5+ | — |

**Verification gates** (each phase merges only when its gates pass):
- **G1, spec diff.** `openapi_go_contract_test` against the refreshed Go lock. Go-only must be empty (the test
  asserts it). Track the Rust-only list and the schema-name test (`shared_operations_name_gos_schemas`) to zero
  unexplained entries.
- **G2, behaviour.** Port the Go tests named in each ledger entry, and mutation-check the load-bearing ones
  (the ledger convention).
- **G3, schema.** Fingerprints as in §3 step 3: Rust on a Go-082 scratch DB, Go on a Rust-first scratch DB.
  Scratch embedded Postgres only, never the owner's fcdev cluster.
- **G4, cross-platform contract.** Use `harness/parity`, Go vs Rust on one seed DB. Add scenarios for every
  ledger entry since F17, keeping `expected-diffs.json` with no stale entries. Use `harness/delivery` for F24
  pause/release and signing.
- **G5, functions (new, P4).** Run Go's `internal/functions/runner` against the **Rust** control plane: deploy
  Go's `examples/functions/hello-go` and `dispatch-demo-*` with Go's `fcdev fn deploy`, invoke, emit, dispatch,
  and pause the connection.

Total to "ready to take over" (P0–P5): about **18–22 sessions**, or about 8–10 with three parallel agents.

## 6. Owner decisions (with recommendations)

1. **D1, function schema of record.**
   *Recommend:* `fng_*` (Go's) for the takeover.
   `fnr_*` is kept only if the component host survives (D2). The rename to `fn_` happens when the Java function
   service is retired.
2. **D2, Rust component host (WASI 0.2, Java wire).**
   *Recommend:* park it behind the Go control plane as an optional second runtime; do not delete it.
   Java parity for functions is no longer a goal.
3. **D3, describe at publish on Rust.**
   *Recommend:* the Rust platform asks a runner, i.e. Go's runner over a new internal control route, rather
   than embedding a core-module engine now.
4. **D4, signing.**
   *Recommend:* unsigned allowed by default, as in Go. Sigstore stays an opt-in per-client policy
   (`fnr_client_policies` semantics ported).
5. **D5, migration source of truth.**
   *Recommend:* Go's goose files are canonical. Rust mirrors each one under an explicit mapping, plus the
   Go-side hazard-A fix.
   Alternative: Rust executes Go's SQL directly from a vendored copy, with goose-compatible tracking.
6. **D6, CLI.**
   *Recommend:* `fcdev` stays Go-only (as F25 says) and must work unchanged against a Rust platform.
   That is a G5 requirement. Rust's `fc-dev fn` is frozen, not extended.
7. **D7, licence.**
   *Recommend:* Apache-2.0, to match Go.
8. **D8, catalogue divergences.**
   *Recommend:*
   - Go's catalogue wins: prune `password-reset-requested`, or Go re-adds it if it is emitted.
   - Settle the 4 extra schema fixes of Rust 070 in Go first, then mirror.

## 7. Maintenance protocol (from now on)

1. **Hand-off note per Go ledger entry.** Every new Go F-entry ends with a "Rust hand-off" paragraph, as it has
   a "Java hand-off" now: the defect, the files, the migration number, the tests to port. A running
   `docs/go-mirror/PENDING.md` lists unmirrored entries (Java's format: id, commits, size, depends on). It is
   created at P0 and seeded from §2 of this plan.
2. **Weekly spec diff (automatable).** A scheduled job copies Go's `api/openapi.lock.json` to a scratch path,
   runs §0's command with the document path overridable (P0 adds an env var such as `GO_LOCK_PATH` to
   `go_document()`), and appends the counts (go-only, rust-only, operationId and schema mismatches) to
   `PENDING.md`. A non-zero go-only count with no PENDING row is a protocol failure.
3. **Migration watch.** Any new file in Go `internal/migrate/sql/` needs a row in the §3 mapping table and a
   PENDING item within the same week. G3 runs in Rust CI on each Rust migration.
4. **Catalogue watch.** A test reads Go's seed (roles, permissions, event types) from a vendored JSON export and
   fails on drift. The export is a Go `make` target to add.
5. **Batch cadence.** One mirror batch per Go ledger batch, one agent per batch, never combined. Each ledger
   records commits, tests (mutation-checked), divergences and "not verified". Only the owner pauses or resumes.
6. **Who.** The Go session that writes a ledger entry writes the hand-off and the PENDING row. A Rust session
   (a Sonnet agent with a spec, per the owner's practice) implements it. The weekly diff can be a scheduled cloud
   agent, read-only, reporting to the owner.

## 8. Verified vs inferred

- **Verified:**
  - the spec diff numbers and lists (Rust dump built and diffed for this plan);
  - Rust's F17 storage path and tolerant lookup;
  - the absence of F18–F21, F25 (except the inactive check) and F26 markers (grep and read);
  - Rust's function ABI, routes and tables;
  - Go's ABI and control routes;
  - both migration trackers and every Go 055–082 file against Rust;
  - the role-sync stripping path;
  - the router ack, h2 and defer facts cited;
  - the SPA provenance and licence.
- **Inferred:**
  - goose's refusal behaviour in hazard A;
  - that Go's wazero runner works unchanged against a Rust control plane (option c);
  - wasmtime's suitability for Go core modules (option a) and the address refactor's breadth;
  - Rust's missing-SQS-queue and NATS-expiry behaviour;
  - whether Rust's reconciler has Go's recreate-identity bug (`9d017689`);
  - commit-subject equivalence for the Oct 2–5 router, scheduler and SQS work;
  - all effort sizes.
- **Not run:**
  - the parity or delivery harnesses;
  - any migration;
  - any Rust test beyond the OpenAPI dump.

# Go to Rust mirror: pending ledger

Unmirrored Go changes, newest ledger first. Created 2026-10-18 from `docs/parity-plan-2026-10-18.md` (section 2);
the protocol is that plan's section 7. Go's ledger is `../flowcatalyst-go/docs/platform-bug-fixes.md`.

A row leaves this file only when its Rust batch is committed and its ledger names the commits. A Go change with
no row here is a protocol failure. Sizes: S under half a day, M about a day, L several days, XL over a week.

## Entries

| Id | Go commit(s) | What Rust lacks | Size | Depends on | Status |
|---|---|---|---|---|---|
| F17 | `c74650d5` | Done (batch A): code-first sync stores the `sac_` service-account id, not the application's `prn_` id (400 `APPLICATION_SERVICE_ACCOUNT_REQUIRED` when the principal has none); optional `serviceAccountId` on `PUT /api/connections/{id}`. Data fix = Go 075 (Rust 087). | S | P1 | done (`d704e283`); `docs/go-mirror/2026-10-19-f17-f18-f21-iam-correctness.md` |
| F18 | `52c51e68` | Done (batch A): `INVALID_ROLE_NAME` / `INVALID_PERMISSION` at every role write, SA `assign_roles` existence check, role create sets `application_id` (platform seed and sync heal it), principal syncs skip names that are not roles. Data fixes Go 076/077 (Rust 088/089). Open: no `role.Name` type; the IdP login sync checks shape only. | M | P1 | done (`7ba569a6`); `docs/go-mirror/2026-10-19-f17-f18-f21-iam-correctness.md` |
| F19 | `489f52c3` | Done (batch B): `msg_dispatch_pools.application_id` ownership, `DISPATCH_POOL_NOT_OWNED`, owner-confined sweep, client-less code match, `dispatch-pool:sync` in the application-service catalogue. Go 078/079 = Rust 090/091. Remaining wire difference: a caller not bound to the target application gets 404 here (the shared `require_application_access`), Go answers 403 `FORBIDDEN`; the use case itself raises Go's 403. | M | P1 | done (`01ed215c`, `93e60403`) |
| F20 | `8376f6e1` | Six app-service permissions (Go 080 = Rust 092 grants them; the catalogue holds them, `01ed215c`), confined event and dispatch-job reads, `/api/applications/{appCode}/users` and `/invitations` (6 operations). | L | none | done (`a98c17fa` reads and checks, `9463c887` users and invitations); Rust mirror in `docs/go-mirror/2026-10-19-f20-application-users.md`; open: out-of-scope `{appCode}` stays 404 where Go answers 403 (owner ruling), SPA pages are P5 |
| F21 | `bf81422f` | Done (batch A): nine role and permission read routes confined for application-service callers; `confined_applications` is the `AppScope.ConfinedCodes` helper. No migration. | M | none | done (`6d657e96`); `docs/go-mirror/2026-10-19-f17-f18-f21-iam-correctness.md` |
| F22 | `e3a9fbb5` | Version FAILED state, publish-replay reset, heartbeat fencing (`requeued_at_revision`, Go 081). Applies to the Go function model only (P4). | M | P4 | pending (column done) |
| F23 | `7ce31723` | `describe.dispatches[]`, pool and connection resolution for function dispatch. | L | P4 | pending |
| F24 | `e599f1de` | `POST /control/functions/dispatch-jobs`, `msg_dispatch_jobs.connection_id` (Go 082, Rust 094), paused-connection claim, connection-signed subscription-less jobs. | L | P1, P3 | pending (column done) |
| F25 | `8254cf0c` | Done (batch D): `fcdev-cli` public client seeded at start, RFC 8252 loopback redirect for that client only, `FC_FCDEV_LOGIN_REFRESH_TTL_SECS` absolute refresh lifetime, `refresh_expires_in`. Platform half only (the CLI stays Go: D6). The inactive-user refusal on the code grant was already here; Go's test ported. `refresh_expires_in` is pinned by a schema unit test until the `/oauth/token` stub lands (P5). | M | none | done (`123247c1`); Rust mirror in `docs/go-mirror/2026-10-19-batch-d-fcdev-login-and-small-items.md` |
| F26 | `c7ffd7c2` | Done (batch B): built-in `platform:function-deployer` (the six permissions of Go's seed; the function permissions already exist in the Rust catalogue). No migration; the start-up/`sync-platform` role sync creates it. | S | none | done (`01ed215c`) |
| F27 | (in progress, Go session) | Go migration bootstrap hazard A: goose must not be seeded from Rust's `_schema_migrations` by number past 030. A Go-repo change; Rust has no code to mirror, but P1's G3 fingerprint test assumes it. | S | none | pending (Go side) |
| F28 | `ebb18542` | Done, off by default: `400 SCOPE_REACH_MISMATCH` when a service account's `scope` disagrees with its client links, behind `FC_SERVICE_ACCOUNT_SCOPE_REACH_STRICT` (the vendored SPA drawer sent the old links beside ANCHOR; it carries Go's fix now, so the switch is safe after a frontend rebuild). | S | none | done (`5592932a`); `docs/go-mirror/2026-10-19-identity-fixes-and-catch-up.md` |
| F29 | `0fb7f0c7` | Done: ETag/304 on router-config, validator kept per URL, default poll 30 s, paced missing-queue recheck, in-process nudge (fc-server). fc-dev uses its built-in document and has no watcher to nudge. | M | none | done (`8a6f1a0e`) |
| F30 | `87cbc30f` | Done: SDK role sync, platform catalogue sync, connection sync and subscription sync skip an unchanged row. Rust has one rollup event per sync (no per-row `*Updated` for connections), so there is none to suppress there; roles and subscriptions emit one per row and now emit none for an unchanged one. | M | none | done (`9725441f`) |
| F31 | `4f2b3908` | Done: `complete` and `fail` bump `attempt_count`; the scheduled-job rollup subject is `platform.scheduledjobs.<app>`. | S | none | done (`fa6dae50`) |
| F32 | `49ba1494` | Already true here since F20 (404 `Application_NOT_FOUND` for another application, an unbound credential and an unknown code, for every caller); the test now pins the body code. | S | none | done (`e8410ba6`, test only) |
| F33 | `281d40d3` | Done: login-time sync skips a mapping whose platform role no longer exists. The write-time `400 ROLE_NOT_FOUND` is behind `FC_IDP_MAPPING_REQUIRE_ROLE`, off by default (a mapping may be written before the application that owns the role first syncs). | S | none | done (`8e71db64`) |
| F34 | `8165e614` | Done: `token_use` allowlist (`api` or none) on the bearer middleware and the function-endpoint verifier; the cookie path already refuses any token carrying `token_use`/`type`/`jti`. | S | none | done (`f0b06002`) |
| F35 | `f0e70ee4` | Function webhook service account (Go 084, `fng_functions.service_account_id`). Function model, not mirrored here. | M | P4 | pending (Go function model only) |
| F36-F45 | `377fe770` to `d7114f37` | Go's identity review fixes. F36/F37 (atomic rotation, TOTP claim) and F39 (one cookie constructor) already held here; F38 is Go-only; F40-F42, F44 mirrored (`b16378a8` and `d53a827d`, `0ecf4a07`, `819e6ead`, with Go's switch names); F43 holds (webauthn-rs refuses a counter regression and the use case persists the counter); F45 is Go dependency bumps. | M | none | done; see hand-off |

## Non-ledger Go work

| Go change | Commit(s) | Size | Status |
|---|---|---|---|
| Requeue-by-filter API and SPA dialog | `165feb6d`, `5496c3ff` | M | API done (`3aa51476`; baseline: 2 operations and 4 schemas closed; the shared WHERE builder with the list `a34dfc92`); SPA dialog pending (P5) |
| Last all-applications admin guard (`LAST_ALL_APPLICATIONS_ADMIN`, `confirmSelfRestriction`) | `9966a2fa`, `01fead7e` | M | API done (`dab7e18e`); SPA confirm pending (P5) |
| `x-sdk` tagging and spec stubs for plain handlers (13 documented-by-Go-only operations) | `21f9734d` | M | tagging done (`98a2ae89`); the stubs and the per-SDK app/admin split pending (P5) |
| Router: urgent poll-loop acks, missing-SQS-queue tolerance, h2 PING, NATS pull expiry, dashboard modal | `c34f230a`, `c52d1150`, `8d1abaf3`, `c6b534d6`, `5cbe502e` | 4 x S | done (P3, 2026-10-19); `docs/go-mirror/2026-10-19-router-s-items.md`. Still pending: Go's 09-22 HOL reservation schedule `1c37a339` (Rust defers by a fixed `CAPACITY_DEFER_SECONDS`): an L router redesign (consumer polling budget, completion-rate reservations, NATS limits), left for its own batch, see the 2026-10-19 hand-off |
| Dispatch job payload edit: `PUT /api/dispatch-jobs/{id}/payload` (+ `/bff`), `edit-payload` permission, `payload-edited` event | `29e8cb11` | M | API done (`8ab6c939`); drawer dialog pending (P5) |
| Dispatch jobs: mandatory 24h window on list reads, requeue/redrive status guard with double confirmation, split write permissions (Go 085 = Rust 095) | `ad37de21` | L | API done (`50b14d05`, `de81a920`, `60f63ae9`); the `resent` event stays `{ids}` by owner ruling; window picker, redrive confirmation and button gating pending (P5) |
| Dispatch jobs: a status race on cancel, complete or edit payload | `b591d703` | S | already a 409 here (`NOT_FAILED` / `NOT_EDITABLE`, not Go's new `STATUS_CHANGED` code); left |
| Dispatch jobs: event type, subscription and connection filters, read-table `connection_id`, index swap (Go 086 = Rust 096), cached filter-options | `bbbb4bb3` | M | API done (`6f43f32f`); filter UI pending (P5). Hand-off for all three: `docs/go-mirror/2026-10-19-dispatch-job-operations.md` |
| Scheduler: 429 honours `Retry-After` via `msg_scheduled_job_instances.not_before` (Go 060, Rust 080); terminal `FlowCatalyst-Outcome: reject` in dispatch jobs and scheduled jobs | `6e6b9307` | S | done (`7748677d`) |
| 5xx logging, S3 startup self-check | `c0d27994` | S | done (`aaf78687`) |
| SPA: F8/F10 locks, credential confirm, ConfirmDialog width, requeue, F20/F25 pages | `90e41132`, `5a712d75`, `c38933f0`, `77d35c9c` | M | pending |
| Licence Apache-2.0 | `7baee1ce` | S | pending (decision D7) |

## Spec diff log

`OPENAPI_REPORT=1 GO_LOCK_PATH=<go>/api/openapi.lock.json cargo test -p fc-platform --test it -- openapi_go_contract_test`
prints the gap list. The ratchet baseline is `crates/fc-platform/tests/data/openapi_go_gaps.baseline.txt`.

| Date | Go commit | go ops | rust ops | go-only | rust-only | operation-id | request | response | schemas missing |
|---|---|---|---|---|---|---|---|---|---|
| 2026-10-18 | `8eb84651` | 309 | 296 | 35 | 22 | 18 | 4 | 9 | 63 |
| 2026-10-19 | `bbbb4bb3`+ (live lock, after the dispatch-job batch) | 311 | 305 | 28 | 22 | 18 | 4 | 9 | 69 |
| 2026-10-19 | `207c217e` (live lock, after the identity and F28-F34 batch) | 311 | 305 | 29 | 22 | 18 | 4 | 9 | 59 |

## Migration mapping

Go migration to Rust migration: the table is in `docs/go-mirror/migration-mapping.md`. A new file in Go's
`internal/migrate/sql/` needs a row there and a row above within the same week.

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

## Non-ledger Go work

| Go change | Commit(s) | Size | Status |
|---|---|---|---|
| Requeue-by-filter API and SPA dialog | `165feb6d`, `5496c3ff` | M | API done (`3aa51476`; baseline: 2 operations and 4 schemas closed); SPA dialog pending (P5) |
| Last all-applications admin guard (`LAST_ALL_APPLICATIONS_ADMIN`, `confirmSelfRestriction`) | `9966a2fa`, `01fead7e` | M | API done (`dab7e18e`); SPA confirm pending (P5) |
| `x-sdk` tagging and spec stubs for plain handlers (13 documented-by-Go-only operations) | `21f9734d` | M | tagging done (`98a2ae89`); the stubs and the per-SDK app/admin split pending (P5) |
| Router: urgent poll-loop acks, missing-SQS-queue tolerance, h2 PING, NATS pull expiry, dashboard modal | `c34f230a`, `c52d1150`, `8d1abaf3`, `c6b534d6`, `5cbe502e` | 4 x S | done (P3, 2026-10-19); `docs/go-mirror/2026-10-19-router-s-items.md`. Still pending: Go's 09-22 HOL reservation schedule (Rust defers by a fixed `CAPACITY_DEFER_SECONDS`) |
| Scheduler: `msg_scheduled_job_instances.not_before` (Go 060, Rust 080) | Go 060 | S | pending (column done) |
| 5xx logging, S3 startup self-check | `c0d27994` | S | done (`aaf78687`) |
| SPA: F8/F10 locks, credential confirm, ConfirmDialog width, requeue, F20/F25 pages | `90e41132`, `5a712d75`, `c38933f0`, `77d35c9c` | M | pending |
| Licence Apache-2.0 | `7baee1ce` | S | pending (decision D7) |

## Spec diff log

`OPENAPI_REPORT=1 GO_LOCK_PATH=<go>/api/openapi.lock.json cargo test -p fc-platform --test it -- openapi_go_contract_test`
prints the gap list. The ratchet baseline is `crates/fc-platform/tests/data/openapi_go_gaps.baseline.txt`.

| Date | Go commit | go ops | rust ops | go-only | rust-only | operation-id | request | response | schemas missing |
|---|---|---|---|---|---|---|---|---|---|
| 2026-10-18 | `8eb84651` | 309 | 296 | 35 | 22 | 18 | 4 | 9 | 63 |

## Migration mapping

Go migration to Rust migration: the table is in `docs/go-mirror/migration-mapping.md`. A new file in Go's
`internal/migrate/sql/` needs a row there and a row above within the same week.

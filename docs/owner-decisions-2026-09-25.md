# Owner decisions — 2026-09-25

The binding record for the work that follows. It supersedes anything in older docs that contradicts it.

## Direction

- **Production moves from Go to the Rust platform.** Rust must be a **drop-in replacement for Go**:
  - For **existing platform behaviour** (IAM, tokens, SDK routes, principals, events, dispatch, scheduling,
    OAuth/OIDC), the reference is **Go** (`../flowcatalyst-go`, read-only).
  - For **new features Go lacks** (the function runner and its management API), the reference is **Java**
    (`../flowcatalyst-javalin`, read-only). The guest runtime is free.
- **Explicit owner rulings override both:**
  - X-06: strict enums
  - X-01: an absent or unknown dispatch mode means `NEXT_ON_ERROR`
  - an out-of-scope application answers 404
  - PKCE is S256 only
  - new service accounts get no application access
  - subscription sync rejects unknown pool codes
  - everything listed below
- **Dependencies may be upgraded to their latest versions.**

## Apps that must keep working after cutover

- `inhance/InhanceMono/apps/integral`, `apps/hr`, `apps/rfp`: Laravel, published `flowcatalyst/laravel-sdk`.
- `inhance/AgentPlanner`: Python, its own OIDC/JWT client.

## Decisions

| # | Topic | Decision |
|---|---|---|
| 1 | Cron | The Rust scheduler evaluates **Java/Go's dialect**: 6-field robfig, Sunday = 0, and the OR rule when both day fields are restricted. Stored crons then mean the same on every platform. Delete the translation adapter. A one-off **migration** rewrites existing Rust-created jobs so they keep firing at their current times. |
| 2 | Time zones | Legacy zones Rust's tz database lacks (e.g. `SystemV/*`) are **rejected** with `TIMEZONE_INVALID`, instead of being accepted and never firing. |
| 3 | JWT claims | Rust issues **Go's shape** exactly: `tier` for the tenancy tier, `scope` = the space-delimited permissions, `token_use`, and the same `type`, `roles`, `clients`, `applications` (+ all-applications) formats. The Rust function host already expects this. |
| 4 | App compatibility | (a) tier and token_use (see 3); (b) `/api/principals` accepts `active=true` and returns all rows when no page size is given, and so do applications and oauth-clients if Go does the same; (c) `POST /api/principals/sync` with password hashes; (d) verify bcrypt `$2y$` hashes at login and rehash to argon2; (e) `clientId` on user create is resolved by id **or** identifier; (f) `clientCode` is resolved on `/api/events/batch`; (g) `/auth/oidc/session/end` accepts `client_id` without `id_token_hint`. Match Go in each case. |
| 5 | Function contract | Improve it **in Rust, backward-compatibly**: no-op writes return 200 (same digest, unchanged alias, already active or retired); optional `expectedVersion` precondition on promote (412); `runtime: component` while still accepting `wasm` plus the `wasi_http_incoming_handler` alias; a `code` field alongside `error` in error bodies; cron accepts 5 or 6 fields with a single `CRON_INVALID` code; hosts compute `unload` themselves (the platform keeps sending it for a release); UPPER_SNAKE not-found codes; the emit size limit measured on raw bytes. |
| 6 | JS/TS functions | Add **V8 isolates** (`deno_core`) as a JS/TS runtime in the Rust host, alongside WASI components. |
| 7 | Function DB access | **One shared, bounded pool per DSN**, shared by every function using that database. Design a per-function share limit, so one function with slow callers can't monopolise a pool; build it now or leave a clear hook. A function needing isolation gets its own DB user, so its own DSN and its own pool. |
| 8 | SPA permissions | `/auth/me` returns the effective permissions. The SPA gates pages by permission and **hides** nav items the user can't use. |
| 9 | Overnight defaults | All accepted: 503 for host-unavailable and pool exhaustion; lower-case header names to guests; h2c by prior knowledge only; response cap = `wasmMemoryMb`; a handler `Err` returns the generic body (only `Response::fail` sends a message); generic `INTERNAL_ERROR` 500 bodies; an instance per request. |
| 10 | Licence | `fc-function-model` becomes **MPL-2.0**, like `fc-function-abi` and the PDK. |
| 11 | SDK home | **The Rust repo becomes the home of the SDKs.** Bring `clients/{typescript,laravel,go}-sdk` up to date with the published versions, which today are built from `flowcatalyst-go/clients/*`, and publish from here from now on. |
| 12 | Built-in roles | Align Rust's catalogue with Go's: add `client-admin`, `portal-administrator`, `router`, and the connection-sync permission on `messaging-admin`, as production defines them. |
| 13 | Fuel metering | Meter **fuel and peak memory per invocation** (metrics per function and per client), with an **optional per-function fuel limit** in manifest `limits`. |
| 14 | Private registries | Add **ECR** support for `oci://` artifact refs, using the host's AWS role. Light testing is fine. |
| 15 | PDK publishing | Make `fc-function-pdk` self-contained (vendor its WIT) so it *can* go to crates.io, and prove it with `cargo publish --dry-run`. The git dependency must always keep working. The owner does the actual publish. |
| 16 | CLAUDE.md | Add three infrastructure exceptions: the function-host heartbeat (host upsert and purge), artifact blob uploads, and the lazy OAuth secret rehash on login. |
| 17 | Flaky tests | Fix the fc-router end-to-end tests that fail under parallel load, and the function example test that hits 429. |
| 19 | Service-account writes | Keep requiring **anchor** scope for service-account create, update and delete, on top of Go's permission check. This is a deliberate deviation from Go, which lets a non-anchor admin mint an ANCHOR-tier account (an escalation path worth fixing in Go too). |
| 20 | JWT (revisited) | Rust issues **Go's** token shape (see #3). This supersedes the earlier "keep Rust's shape". |
| 21 | Java rulings of 2026-09-25 | The owner's rulings recorded in the Java repo (`docs/backlog.md` @ f6e10994, items 2–17) apply to Rust too. Triage: `docs/parity/java-2026-09-25-triage.md`. |
| 22 | Sync `passwordHash` | **Ruling 4:** used only when the sync creates the principal, never applied to an existing one (any caller); the result reports it ignored. Supersedes the overwrite in #4(c); hr/rfp re-runs no longer rotate existing users' passwords. The app-scoped sync must still carry the hash for creates. |
| 23 | App-sync role names | Refuse names prefixed `platform:` or another application's code; other names are accepted as today. |
| 24 | Ingest tenancy and ids | A non-anchor caller must name a client it can access: a client-less or unknown-client event is refused (Java S3.2a). Supplied ids are honoured, as Go: a duplicate dispatch-job id refuses the whole batch with 409 `DUPLICATE_ID` (live table); events stay idempotent (#18). |
| 25 | Anchor is reach, never authority | `/api/roles` writes and client-access grants need anchor **and** the permission (stricter than Go, as #19). |
| 26 | Session cookie | Adopt Go's design: the cookie carries the subject only and the principal is reloaded per request (immediate deactivation; Go-issued cookies survive cutover; cookie-only routes are distinguishable). |
| 27 | JS functions | `runtime: js` with a JS bundle artifact, and a **Rust-shaped JS API** modelled on the WIT interfaces (not Java's `@flowcatalyst/function`). |
| 28 | Pipeline alignment | The message pipeline (outbox → ingest → scheduler → queue → router → `/api/dispatch/process`) is aligned to **Go** as a drop-in, per `docs/reviews/message-pipeline-review-2026-09-25.md`. Correctness is gated by harnesses: Java's mediation conformance corpus, a new Go-vs-Rust delivery harness, and API parity on Java's scenario files. |
| 29 | Harnesses | The API parity runner is **ported into the Rust repo** (reads Java's scenario JSON, copied with provenance; runs Go and Rust side by side). The mediation conformance corpus is vendored with a Rust runner; where the corpus rules Go's behaviour a defect, **the corpus wins** and each such row is listed as a deliberate deviation from Go. |
| 30 | `$schema` in responses | **Confirmed by the owner 2026-09-26 ("we don't need $schema").** Rust does **not** emit huma's `$schema` member (`"<base>/<Model>.json"`) that Go adds to every JSON response body. No consumer reads it: the Laravel SDK's generated models treat it as optional, and the TypeScript and Rust SDKs and the SPA ignore it. The API parity harness drops a body's top-level `$schema` on both sides (normaliser rule 0) under this decision. |
| 31 | Go's delivery defects (**confirmed by the owner 2026-09-26: "keep your fixes"**) | Where the delivery harness shows Go losing, duplicating or stranding messages and Rust not (e.g. Go loses 2 of 40 on a worker SIGKILL; Go's `router-config` omits client-scoped queues), Rust keeps the correct behaviour. The harness records each as an `expected-diffs` entry citing this decision, like #29's corpus rule. |
| 32 | 2FA and self-service password writes | Written directly, as Go does, with Go's audit rows and no domain events; recorded as an infrastructure exception in `CLAUDE.md` (owner, 2026-09-26). |
| 33 | `/version` | Rust reports its real build version (Go reports `dev` locally). Owner, 2026-09-26. |
| 34 | Delete guards | Deleting a connection still used by subscriptions, or an application that still has access grants, is refused (409) — Go allows both and leaves dangling references. Owner, 2026-09-26. |
| 35 | Event-type catalogue | Rust seeds Go's catalogue plus every event type Rust emits (131 vs Go's 73); nothing is removed. Owner, 2026-09-26. |
| 36 | Audit command JSON | Stored command JSON stays camelCase (Go stores PascalCase field names); operation names match Go. Owner, 2026-09-26. |
| 37 | Developer portal | `/bff/developer` also admits application-scoped developers, for the applications they can access (Go: anchor + `openapi:view` only). Owner, 2026-09-26. |
| 38 | Go 500s and data loss in admin writes | Where Go answers 500 or loses data and Rust answers correctly (anchor-domain update into an existing domain and duplicate IdP role mapping → 409; an email-domain mapping update clears `primaryClientId` only on explicit `null`), Rust keeps its behaviour, as #31. Owner, 2026-09-26. |
| 39 | Developer portal and the platform app | Application-scoped developers (#37) also see the seeded `platform` application (the platform's own API reference). Owner, 2026-09-26. |
| 40 | More Go defects Rust keeps correct | Go's app-scoped syncs answer 500 `AUDIT_WRITE` for application codes over 17 characters (Rust widened the column, migration 038), and Go's service-account delete leaves the principal and OAuth client behind so a deleted account's credentials still work. Rust stays correct; the harness allow-lists them citing this decision. Owner, 2026-09-26. |
| 41 | Router release delays (deviation D1) | On a connection error, 5xx or open breaker, Rust returns the message to the broker after 30 s (5 s breaker, 10 s siblings); Go returns it at once, hot-looping during an outage and spending SQS receive counts toward the DLQ. Rust keeps its delays. Owner, 2026-09-26. |
| 42 | Topcoat UI | `fc-web` uses Topcoat UI's components and Tailwind as its base ("use as much of Topcoat UI as makes sense"); the no-Tailwind rule is only for the PrimeVue SPA. fc-web stays optional behind `--features web` until the owner makes it primary. Owner, 2026-09-26. |
| 43 | Router `AUTH_MODE=NONE` (transitional; coordinator decision, a deviation from ruling 2) | Ruling 2 (via #21) says Basic and `AUTH_MODE=NONE` apply in dev mode only. Production's router task (`inhance/iac/compute/fc-router.ts`) sets `AUTH_MODE=NONE`, and the apps' SDKs do not send router tokens yet (laravel-sdk 0.10.27 must reach integral, hr and rfp first; hr is on `^0.8`). So, until the owner drops it from the task: outside dev mode `AUTH_MODE=NONE` is **still honoured**, with a WARN at startup and an `authWarning` on the router's `/health`, `/monitoring/health` (the dashboard shows it as a banner) and `/monitoring` output: "router API unauthenticated; remove AUTH_MODE=NONE once SDKs send the platform bearer". `AUTH_MODE` unset, `BEARER` or `OIDC` enforce the platform bearer; `BASIC`, `OIDC_FLOW`, any other value and `FC_ROUTER_AUTH_USER`/`_PASS` are ignored outside dev mode with a WARN. The dev-only routes (mocks, benchmark, seed) are absent outside dev mode whatever `AUTH_MODE` says. Remove the `NONE` exception once production no longer sets it (cutover checklist, "Router auth"). |
| 44 | Platform-config write permission (owner, 2026-09-27) | Config writes are gated by `platform:admin:config:manage` (Java V17's name), not Go's `…:config:update`. The built-in roles grant `manage`. A stored role holding Go's `…:config:update` is still honoured as `manage` (`permissions::admin::CONFIG_UPDATE_GO`), so the cutover needs no data rewrite and a rollback to Go keeps custom roles working; Go's own built-in roles are reset by whichever binary starts. |
| 45 | `FC_FN_MAX_EXECUTING` (owner, 2026-09-27) | One host-wide budget shared by every function runtime (WASM and JS together), not one per runtime. |
| 46 | Members Go documents but never fills (owner, 2026-09-27: "go ahead") | Where Go's OpenAPI contract names a member that Go never stores or answers, Rust implements it, so a client generated from the contract loses nothing: client-config `baseUrlOverride`/`configJson` (APP-3, migration 058), event `contextData`, role-assignment `assignedBy` (059), the service-account webhook credential members Go drops (060), and `DispatchJobRead.priority` (DJ-10, 061: 1 for a job claiming `HIGH_PRIORITY`, 0 for `DEFAULT`, absent without a claim). Supersedes the "Go behaviour stands" deferral of APP-3 and DJ-10 for those members. The harness allow-lists the resulting extra members citing this decision. |
| 47 | Java functions (owner, 2026-09-28) | No Java-to-Wasm toolchain (TeaVM, GraalVM's Wasm backend) for the Rust function host. Java work runs either as a regular service using the FlowCatalyst Java SDK, or as `runtime: jvm` functions on Java's function host (flowcatalyst-javalin) against this platform. |
| 48 | Function-registry table prefixes (owner, 2026-09-28; temporary) | Go's function runner (its migration 059) has `fn_*` tables incompatible with Java's and Rust's, and every platform creates them with `CREATE TABLE IF NOT EXISTS` on shared databases. Until the owner picks one implementation, each has its own prefix: **Java keeps `fn_`, Rust uses `fnr_`, Go uses `fng_`**. In Rust, migration 062 creates `fnr_*` (the end state of 034 + 037 + 056, renamed), and the runner retires 034, 037 and 056 (`RETIRED_MIGRATIONS`: never run, a recorded row tolerated), so Rust never creates or alters a `fn_*` table. No data is copied: functions registered in Rust's old `fn_*` are published again. |
| 49 | Role ceiling on SDK role writes (owner, 2026-09-30) | `POST`/`DELETE /api/applications/{app}/roles` apply ruling 14's ceiling through the use case (403 `PERMISSION_ABOVE_CALLER`): an SDK service account cannot create or delete a role carrying permissions it does not hold. Go has no ceiling here; Rust keeps it, as the safer behaviour. |
| 50 | Router-config queues per tenant (owner, 2026-09-30) | Every tenant (platform, pool, active-subscription and every client in `tnt_clients`) gets both `DEFAULT` and `HIGH_PRIORITY` queues. Java drops its old "one queue per priority in use" rule, which stopped matching once a job's own `queue` claim wins over its subscription's; Go and Rust already do this. |
| 51 | SDK licence split (owner, 2026-09-30) | The types the Apache-2.0 `fc-sdk` needs from AGPL `fc-common` (outbox status and item types, TSIDs, audit redaction) move to a permissively licensed crate, so SDK users take no AGPL code. |
| 52 | No SQLite in `fc-server` (owner, 2026-09-30) | `fc-server`'s outbox role reads Postgres and MySQL; SQLite stays in `fc-outbox-processor` and fc-dev only. Removes the SQLite C build from the production binary. |
| 53 | Go delete guards (owner, 2026-09-30, decision #34 applied to Go) | Go refuses deleting a connection that still has subscriptions, and an application that still has grants (409), matching Rust. |
| 54 | Function secret references (owner, 2026-10-01) | `PUT /api/functions/{address}/secrets/{key}` (and DB settings) keep an external secret-manager reference (`aws-sm://…`) or an already-`encrypted:` value as given, resolved when the function runs; every other value, including a `postgres://` DSN, is plaintext and encrypted. A malformed `aws-sm://` or `encrypted:` value is a 400 with its message (Java's `INVALID_SECRET_REF`). The middle road of `docs/plans/go-function-service-fixes.md` §1.2. |
| 55 | Application delete and disabled client configs (owner, 2026-10-01) | The #34 guard counts only *enabled* client configs; disabled ones no longer block an application delete and are removed with it. Applies to Rust and Go. |
| 18 | Housekeeping | Keep the fc-router dev-only `hyper` 1.9.0 pin. Make Rust's event ingest idempotent (`ON CONFLICT DO NOTHING`), as Go and Java do. |

## Re-check needed

The 2026-09-24 "Java is the reference" re-alignment changed these to follow Java. With Go now the reference for
existing behaviour, each must be re-verified against Go and reverted to Go where they differ, unless an owner
ruling applies:
- `cca61675`: pool sweep, anchor gate
- `5de4c4dd`: config routes gated by config permissions
- `69e69f47`: OAuth events renamed `platform:admin:oauth-client:*`
- `ceb1f002`: use-case error codes in the body
- `6a8c6e24`: OIDC secret with no app key returns 400
- `cd1da9cf`: service-account code rule and `CODE_EXISTS`

This includes anything in `docs/parity/l9-idiom-inventory.md`'s "Remaining deviations from Java" table that
concerns existing, non-function behaviour.

## Follow-ups found during wave 1 (to do)

- **Cutover blocker:** about 40 domain event type names differ from Go's (e.g. `platform:iam:client:*` vs Go's
  `platform:admin:client:*`), and Rust's event `data` carries extra metadata. Subscribers match on these, so align
  them to Go.
- **Cutover blocker:** Go can store IDP secrets as secret-manager references (`aws-sm://…`). Rust must resolve them as
  Go does, and the `backfill-secrets` tool must **skip** references and never encrypt a ref string. Production
  currently holds only `encrypted:` values (audit 2026-09-24).
- **Read permissions:** many Rust list and read endpoints only require login where Go checks a permission. Enforce
  Go's read permissions.
- **Guardrail:** add a convention test requiring every `/api` and `/bff` route to authenticate unless explicitly
  allowlisted. `/bff/debug/*` had no auth at all; fixed in `4845d960`, and still open on `main`.
- `/auth/me` should include the caller's scope/tier, so the SPA can gate anchor-only pages.
- Missing Go routes: `connections/sync`, `docs/sync`, `POST /api/processes/sync`, `router-config`.
- `client-admin` and `portal-administrator` exist as roles, but Go's enforcement behind them isn't built.

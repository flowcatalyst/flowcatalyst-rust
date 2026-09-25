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

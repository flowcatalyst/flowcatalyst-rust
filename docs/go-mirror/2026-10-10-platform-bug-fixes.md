# Go platform bug fixes, mirrored (2026-10-10)

Go's batch: `flowcatalyst-go/docs/platform-bug-fixes.md` (bugs 1 to 25, the dispatch
pool/connection items, the "found while fixing" items), its commits `3d536a7..0eb2b5f`, plus
`fcce86c` (platform-config confinement) and `c59e246` (dispatch-pool per-operation permissions).
The owner lifted the Java/Rust pause for this list only; nothing else Go did since was mirrored
(requeue-by-filter, router/perf, the server-rendered `/ui`, compression, schemacodegen).

Each entry: whether the defect existed here, the Rust commit, the test (see "Verified" for which
tests were run against the code before their fix), and any deviation from Go.

Tests named `*_test::` below are in `crates/fc-platform/tests/it/` (the `it` binary; database
tests, run with `FC_TEST_PG_BIN=<postgres bin> cargo test -p fc-platform --test it --
--include-ignored <module>::`). SPA tests are `frontend/tests/*.test.ts` (`pnpm test`).

## Summary

| Go | Bug | Applied here | Rust commit |
|---|---|---|---|
| `3d536a7` `6be78ee` `210e4b5` `1fa33ef` `1f83887` `e03e54a` `b49b73a` `4c87621` `c59e246` | 8, 13, 17 and the per-operation gates | yes | `4729429b` (+ `6a229313`) |
| `308589b` | 1, 2 `/bff/event-types` reads | partly (filters unguarded; facets single-valued) | `f5c8327d` |
| `7b5453a` | 3 add schema stored double-encoded | yes (storage, MIME default by type, codes) | `f5c8327d` |
| `17b79ef` | 4 stored XSS in the SPA schema viewer | yes (SPA copy) | `ffdc07e0` |
| `62b72e3` | 5 archive/delete rules | archive yes; delete had the rule with another code | `f5c8327d` |
| `b7aaaa4` | 6 sync counts unchanged as updated | yes | `f5c8327d` |
| `b7b9a06` | 7 SPA pencil on archived event type | yes (SPA) | `cb77acdd` |
| `eb537a7` | 9 subscription create drops clientScoped/sequence | yes | `83167f31` |
| `8b98b1f` | 10 SPA Queue free text | yes (SPA) | `c655e726` |
| `e6d1dc0` | 11 dispatchPoolCode only by sync | yes | `83167f31` |
| `c3768c7` | 12 blank connectionId stored "" | yes | `83167f31` |
| `61dbcb9` | 14 deleting an active application | yes | `e4f782db` |
| `ff65ec4` | 15 client Deactivate deleted | yes | `e4f782db` |
| `db4e776` | 16 client list paging | yes (SPA; the API was already unpaged) | `2d4922e8` |
| `563102a` | 18 filter options not tenant-scoped | yes (both) | `b339675f` (+ `1ce49621`) |
| `069a65d` | 19 SPA dispatch-job facets empty | yes (SPA, applied by hand) | `c1f637d1` |
| `6116eee` | 20 SPA Sign without view-raw | yes (SPA) | `87755538`, `aaf47943` |
| `858f02c` | 21 size above 1,000 reset to 100 | yes (both lists) | `b339675f` (+ `1ce49621`) |
| `b3c28a4` | 22 event detail without context data | no: already read from `msg_events` | none |
| `ccf2dd5` | 23, 25 process guards; blank description | guards existed with other codes; blank description yes | `bc360b68` |
| `10a5ac1` | 24 SPA pencil on archived process | yes (SPA) | `48bb9edc` |
| `60dc063` | client identifier on create | yes (pool, connection, subscription) | `83167f31` |
| `d1158fa` | pool edit cannot clear rate limit/description | yes (API and SPA) | `83167f31` |
| `108894d` | client user cannot pick its client | yes (SPA half; the BFF options already right) | `50a64dad` |
| `a3414cc` | Sync Platform Events gate | yes (it took any event-type write, no anchor) | `f5c8327d` |
| `5b75de5` | `/bff/roles` gates | reads and define-permission and sync yes; writes already per-operation | `cabaabbd` |
| `fcce86c` | platform-config confinement | partly (unregistered codes were not confined) | `2c8c4da0` |
| `8b8322e` | `/ui` dialog slot | not applicable (Go's server-rendered `/ui`) | none |

## Permission gates (Go bugs 8, 13, 17; event types, processes, roles, service accounts, principals, dispatch pools)

- **Applied.** Every handler listed in Go's table used an any-of check (`can_write_*`). Event-type
  `DELETE /api/event-types/{id}` and process archive (any of four, including `process:archive`)
  were looser still. The BFF event-type writes used the any-of check too (Go's BFF checks the
  operation's permission). Fixed as Go's table; the per-operation checks answer Go's 403
  `PERMISSION_REQUIRED`, `permission required: <code>` (several answered `FORBIDDEN`,
  `Cannot … `). `require_user_admin` takes the operation's check.
- **Test**: `per_operation_gate_test` (every write of the eight modules, plus the BFF event-type
  writes, with each single permission of the trio; refused unless it is the operation's own).
  (Not run against the old handlers; their any-of checks admit the callers it expects refused.)
- **Deviations** (kept, older owner rulings): role writes keep anchor reach (decision #25);
  principal role assignment keeps `user:assign-roles` (ruling 14; Go now asks `user:update`);
  service-account deactivate keeps the anchor check Rust already had (Go's lacks it, a Go
  finding). Service-account create/update/delete now ask the permission first, then anchor, in
  Go's order (`requireAnchorWriter`); `service_account_scope_test` pinned the old order and now
  reads the tier through an anchor-first route (`6a229313`).
- **Not changed**: fc-web (the Topcoat `/ui` trial, outside the workspace) still gates its pages
  with the any-of checks (`can_write_*` kept for it). Go fixed its `/ui` in the same commits; the
  owner excluded the server-rendered UI from this mirror. Open item.

## 1, 2. `/bff/event-types` reads

- **Partly applied.** List and get already checked `event-type:view` and client scoping; the three
  filter endpoints (`/filters/applications`, `/subdomains`, `/aggregates`) checked nothing. The
  facets were single `Option<String>` parameters, so the SPA's repeated
  `application=a&application=b` was refused (duplicate field) instead of matching either.
- **Fix**: the filters make the list's checks; every facet is read as a list (any value
  matches); the filters read every status, as Go (they read CURRENT only).
- **Deviation**: the BFF admits only the messaging view permission (Rust's established BFF rule),
  so an application service account is refused (403) where Go confines it to its own applications.
- **Test**: `event_type_bug_fixes_test::bff_event_type_reads_check_the_permission_and_match_every_facet_value`.

## 3. Add schema

- **Applied.** MIME and schema type were already passed from the BFF, but a JSON Schema sent as
  a JSON string (the SPA's textarea) was stored as a string; the MIME default ignored the type;
  an unknown type answered the enum parser's code; a MIME longer than 100 hit the column.
- **Fix**: `AddSchemaCommand` carries the raw `schemaType` and optional `mimeType`; validation and
  storage as Go (`INVALID_SCHEMA`, `INVALID_SCHEMA_TYPE`, `INVALID_MIME_TYPE`, `SCHEMA_REQUIRED`,
  MIME default by type). `POST /api/event-types/{id}/schemas` and `/versions` get the same
  normalisation; no body change.
- **Deviation**: the BFF create with an initial schema runs `AddSchema` here, so that schema is
  normalised too (Go's create stores it as sent); the SPA's create sends no schema.
- **Tests**: `event_type_bug_fixes_test::add_schema_stores_the_document_and_honours_mime_and_schema_type`;
  unit tests in `event_type/entity.rs`.

## 4, 7, 10, 16, 19, 20, 24, pool edit, client pick. SPA

The Rust repo carries Go's SPA. Each Go SPA commit was applied to `frontend/` (all applied
cleanly but `069a65d`, applied by hand: this copy has no requeue-by-filter dialog), with Go's
tests. `aaf47943` fixes a Rust-only drawer test that mounted the drawer without the auth store the
Sign gate now reads. `frontend/openapi/openapi.json` takes only Go's two lock hunks (bug 9's
fields, the pool field docs), and `pnpm api:generate` reproduces the patched
`src/api/generated/types.gen.ts` byte for byte. Checks: `pnpm test` (22 files, 86 tests),
`vue-tsc -b`, `oxlint` (one existing warning, `useReturnTo.ts`).

## 5. Archive and delete rules

- **Archive applied** (`SCHEMAS_NOT_DEPRECATED`, "Deprecate every schema version first.").
  **Delete** already refused with the rule, as `CANNOT_DELETE` and its own message; it now answers
  Go's `DELETE_NOT_ALLOWED` and message. Rules on the entity (`can_archive`, `can_delete`).
- **Test**: `event_type_bug_fixes_test::archive_and_delete_enforce_the_version_rules`.

## 6. Sync counts unchanged types

- **Applied**: an unchanged listed type (name, description, the 1.0 schema by value) is neither
  saved nor counted, and emits no `EventTypeUpdated`.
- **Test**: `event_type_bug_fixes_test::sync_platform_needs_the_sync_permission_and_counts_unchanged_types_as_unchanged`.

## 9, 11, 12, client identifier, pool edit

- **Applied**: create accepts `clientScoped` and `sequence` (negative: `INVALID_SEQUENCE`); create
  and update store the pool code (`SubscriptionRepository::dispatch_pool_code`); a blank
  `connectionId` on update is "not provided"; pool, connection and subscription create store the
  client identifier (`client_identifier` on each repository); a pool edit clears a blank
  description and a zero rate limit. Two checked queries, `.sqlx/` regenerated.
- **Not backfilled** (as Go): existing rows keep no identifier/code.
- **Test**: `messaging_write_bug_fixes_test::pools_connections_and_subscriptions_store_what_they_are_given`.

## 14, 15. Application delete, client deactivate

- **Applied**: deleting an active application is 409 `APPLICATION_ACTIVE` before the reference
  check; `POST /api/clients/{id}/deactivate` runs the new `DeactivateClientUseCase` (INACTIVE with
  the reason, `platform:admin:client:deactivated`), `DELETE` still deletes.
- **Rust addition**: `platform:admin:client:deactivated` is added to the platform event-type
  catalogue and schemas, next to Rust's other `platform:admin:client:*` entries (Go lists it as
  uncatalogued).
- **Tests**: `application_client_lifecycle_test`; `application_delete_client_configs_test` and
  `service_account_delete_test` now deactivate before deleting.

## 18, 21. Filter options scope; size clamp

- **Applied**: both filter-options endpoints pass the list's tenant scope; both lists clamp a
  `size`/`limit` above 1,000 to 1,000 (`list_limit`). The debug grids already clamped.
- **Tests**: `read_scope_bug_fixes_test` (both cases); unit test of `list_limit`.

## 22. Event detail context

- **Not applicable**: `GET /api/events/{id}` already joins `msg_events` for `contextData`
  (`find_read_detail_by_id`).

## 23, 25. Processes

- **Guards existed** with other codes; now Go's `PROCESS_ARCHIVED` / `PROCESS_NOT_ARCHIVED` and
  messages. A blank description is stored as none in create and update (applied).
- **Finding, not changed**: Rust's update answers 400 `NO_CHANGES` for an edit that changes
  nothing; Go has no such rule.
- **Test**: `process_lifecycle_test`.

## Sync Platform Events, `/bff/roles`, platform config

- **a3414cc applied**: anchor reach, then `event-type:sync` or `:manage` (it took any event-type
  write and no anchor). Test in `event_type_bug_fixes_test`.
- **5b75de5**: reads (list, filters, permissions, one role) checked nothing: now `can_read_roles`;
  defining a permission checked anchor only: now `role:create` then anchor; the platform role
  sync asked anchor and any role write: now create, update and delete. Role create/update/delete
  were already per-operation (decision #25); their order is now Go's (permission, then anchor).
  Deviation: the reads use Rust's `can_read_roles`, which also admits `role:manage` and the
  application-service role view. Test: `bff_roles_gate_test`.
- **fcce86c, partly applied**: the `/api/platform-config/*` and `/api/config/{app}/{section}/{property}`
  routes already confined a caller to registered applications it reaches (404, the owner's rule
  for an out-of-scope application, where Go answers 403). An unregistered code was not confined;
  now an anchor caller that does not reach every application is refused it (the same 404).
  Rust keeps letting an application revoke its own application's grants (Go refuses every revoke
  to an application-scoped anchor). `go_routes_test`'s unregistered-code test now uses a caller
  reaching every application. Test: `platform_config_confinement_test`.

## Verified

- Before the fixes: the seven new test modules (12 tests) were run against the code before them
  (source stashed, tests kept): all 12 failed, each at an assertion about its bug. Two tests
  stop at an earlier assertion of the same commit there: the BFF facet test at the missing
  filter permission (bug 1) before its multi-value part (bug 2), and the Sync Platform Events
  test at the gate (a3414cc) before its unchanged-count part (bug 6); bugs 2 and 6 were not seen
  failing on their own. `per_operation_gate_test` was only run against the fixed handlers: it was not
  observed failing on the old code (whose any-of checks admit exactly the callers it expects
  refused).
- After: `cargo test -p fc-platform --test it -- --include-ignored` with the local PostgreSQL
  harness (`FC_TEST_PG_BIN`): the new modules plus 23 neighbouring and convention modules, 110
  passed, 0 failed. The full `it` suite: 438 passed, 31 failed; 27 of those start Docker
  containers themselves (`api_integration_tests`, `login_backoff_integration_tests`,
  `webauthn_integration_tests`, `id_roundtrip_test`, `function_table_prefix_test`, the LocalStack
  scheduler test; no Docker here), the other 4 were this work's and were fixed
  (`service_account_scope_test`, `go_routes_test`'s config case, and two new tests' setup) and
  re-run green. The full suite was not re-run after those fixes.
- `cargo test --lib` for fc-platform-core, -iam and -messaging: green (one catalogue alignment
  failure, `platform:admin:client:deactivated` missing from the definitions, fixed in `e4f782db`
  and its seed tests re-run).
- `route_table_snapshot_test`: only the OpenAPI document hashes changed; regenerated in
  `ea7d73d4` (the three commits before it fail it).
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --exclude fc-dev --all-targets --
  -D warnings` clean (fc-dev does not compile at the base, see below).
- Frontend: `pnpm test` 22 files / 86 tests, `vue-tsc -b`, `oxlint` (one existing warning).

## Found, not fixed

- fc-web (the Topcoat `/ui` trial) still uses the any-of gates on its pool, subscription,
  application, event-type and user pages, its Deactivate client action still runs the delete use
  case, and its Sync Platform Events action has its own gate. Go fixed its `/ui` in the same
  commits; the server-rendered UI is outside this mirror (owner decision needed).
- Process update answers 400 `NO_CHANGES` for an edit that changes nothing; Go has no such rule.
- `fc-dev` does not compile at the base of this work: `bin/fc-dev/src/main.rs` builds
  `DispatchJobsState` without `app_scope` (added in `14a1ec9f`). It blocks
  `cargo clippy --all-targets` and `scripts/sqlx-prepare.sh` for the whole workspace.

## Not verified

- No browser run of the SPA; no fc-web build (`crates/fc-web` gets two fields in one struct
  literal and is otherwise untouched).
- `scripts/sqlx-prepare.sh` wrote the two new `.sqlx/` files (checked: the two queries, non-null
  text) but then failed on `fc-dev`, so `--check` was not run and stale entries, if any, were not
  pruned.
- The SDK OpenAPI copies (`clients/*/openapi`) were not refreshed, as in Go.

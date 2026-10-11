# Go platform bug fixes, second batch, mirrored (2026-10-11)

Go's second batch: the F1 to F6 entries of `flowcatalyst-go/docs/platform-bug-fixes.md`
("Follow-up: the findings fixed"), its commits `5d63669`, `0a8df19`, `25c3ed6`, `0ae0988`,
`3f0dba4`, `fb36438` (docs `62f71d0`), and F7 that landed while this ran (`e02159b`, `18c0c27`,
docs `ce7a0bb`). Plus the owner's rulings for Rust: (a) `user:assign-roles` is the user role
permission (Rust already); (b) the fc-dev compile break; (c) the fc-web `/ui` trial brought in
line with the first batch; (d) Process update's `NO_CHANGES`; (e) platform-config keeps 404 for an
unknown application code; (f) service-account rotations need update + anchor. The first batch is
`2026-10-10-platform-bug-fixes.md`.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`FC_TEST_PG_BIN=<postgres bin> cargo
test -p fc-platform --test it -- --include-ignored <module>::`). SPA tests are
`frontend/tests/*.test.ts` (`pnpm test`).

## Summary

| Item | Go | Defect here | Rust commit |
|---|---|---|---|
| (b) fc-dev compile | none | yes: `DispatchJobsState` built without `app_scope` | `4bf43109` |
| F1 scheduled-job `/log`, `/complete` | `5d63669` | yes, the other way round: the grant was admitted with no application confinement | `bf2f86fc` |
| F2 event-type `clientId` on create | `0a8df19` | yes (same scope bypass) | `8a3f4003` |
| F3 client event codes | `25c3ed6` | partly (both sets catalogued; the iam set never emitted) | `973801fc` |
| F4 service-account writes | `0ae0988` | partly (permission + anchor already, anchor-first; no application confinement) | `5c592c03` |
| F5 code-first sync client identifier | `3f0dba4` | yes | `a96e3ffe` |
| F6 SPA sync and role controls | `fb36438` | yes (SPA) | `197895e2` |
| F7 user role assignment | `e02159b`, `18c0c27` | API no (already `user:assign-roles`); SPA yes | `a5318245` |
| (d) Process update `NO_CHANGES` | none (Go has no rule) | yes | `be1ace0e` |
| (c) fc-web `/ui` trial | `/ui` parts of the first batch | yes | `c1e9e8f3` |
| (e) platform-config 404 | `fcce86c` (403) | kept (ruling) | none |

`a54d8df0` only moves the six new test modules to their alphabetical places in `tests/it/main.rs`
(the earlier commits registered them at the top).

## (b) fc-dev did not compile

- **Found**: `bin/fc-dev/src/main.rs` built a `DispatchJobsState` without the `app_scope` field
  `14a1ec9f` added, so `cargo clippy --workspace --all-targets` and `scripts/sqlx-prepare.sh`
  failed at fc-dev. The state was never mounted (`let _ = dispatch_jobs_state;`, its router would
  double-register `/batch`).
- **Fix**: the dead state (and its two imports) removed; nothing else in fc-dev changes.
- **Check**: `cargo clippy --workspace --all-targets -- -D warnings` clean, fc-dev included.

## F1. Scheduled-job runner reports

- **Found**: `POST /api/scheduled-jobs/instances/{instanceId}/log` and `/complete` already
  admitted `platform:application-service:scheduled-job-instance:write` (so the SDK was not
  refused, unlike Go), but with no application check: an application's service account reported
  on (logged, completed) any application's firings, and on jobs of no application. They also
  admitted `scheduled-job:manage`, which Go does not, and refused as `FORBIDDEN` "Cannot write to
  scheduled job instance".
- **Fix**: `checks::can_report_scheduled_job_instances` (Go `CanReportScheduledJobInstances`:
  the messaging create/update/delete grants or the application-service grant, 403
  `PERMISSION_REQUIRED`), then, for a caller holding only the application-service grant, the
  instance's job must belong to an application its scope covers
  (`checks::can_report_scheduled_job_instance`, 403 `APPLICATION_ACCESS_REQUIRED`), then the
  per-instance client scope, in Go's order (the coarse gate in each handler, the rest in
  `reportable_instance`). `ScheduledJobsState` gets `app_access`. The two convention scanners
  (`permission_convention_test`, `route_auth_convention_test`) learn the `can_report_` family.
- **Deviation removed**: `scheduled-job:manage` alone no longer admits these two endpoints (Go).
- **Test**: `scheduled_job_report_scope_test` (a provisioned-style application service account:
  own firing logged and completed; another application's and a no-application firing refused;
  the job view refused; the update grant reports on any).

## F2. Event-type `clientId` on create

- **Found**: as Go: `CreateEventTypeCommand.client_id` drove the scope check
  (`check_scope_access` on it) and was set on the entity, which has no client column; a client
  user with `event-type:create` naming its own client created a platform-wide type, and the
  `EventTypeCreated` event named the client. The BFF made its own `ensure_can_create` pre-check
  on the client too.
- **Fix**: the command carries no client; `authorize` is `check_scope_access(caller, None)`
  (anchor reach, 403 `SCOPE_FORBIDDEN`); the BFF's pre-check removed (the use case answers, as
  Go's BFF does); the wire `clientId` stays, ignored, with Go's description on
  `CreateEventTypeRequest` (and a doc note on the BFF request); fc-web's create builds no client.
  `frontend/openapi/openapi.json` and `types.gen.ts` take Go's one description hunk
  (`pnpm api:generate` reproduces the file byte for byte).
- **Test**: `event_type_client_ignored_test` (client user refused on `/api` and `/bff`, nothing
  created; an anchor's create's event names no client).

## F3. Client event codes

- **Found**: Rust's seed catalogued Go's `platform:iam:client:{created,updated,activated,
  suspended,deleted,note-added}` (nothing emits them) and, in its "emitted beyond Go" section, the
  seven emitted `platform:admin:client:*` and `platform:iam:client:applications-updated` with
  schemas of the emitted data. So a Rust database has both sets; a database Go seeded before
  `25c3ed6` has only the iam rows.
- **Fix**: the six iam codes leave the catalogue (definitions and schemas). Migration **068**
  (`migrations/068_client_event_type_codes.sql`) is Go's 070 Up section verbatim (rename the six
  rows where the emitted code is free, re-point subscription bindings, replace a 1.0 schema only
  while it is exactly Go's seeded one). The runner here is forward-only, so Go's Down is not
  carried; the SQL is idempotent, so a database Go already migrated runs it as a no-op.
- **Not changed (owner decision)**: on a database Rust seeded, both sets exist, so 068 leaves both
  (Go's clash rule): the six iam rows stay as catalogue entries nothing emits, and a subscription
  bound to one still never fires. Deleting them, or re-pointing their bindings to the admin rows,
  is a decision this mirror did not take.
- **Consistency test** (Go's `TestEveryEmittedEventTypeIsCatalogued`): `emitted_catalogue_test`
  reads every platform file that defines domain events and fails on an emitted code the catalogue
  lacks. **Found, not fixed**: 33 emitted codes are not catalogued here (the function registry's
  14, the portal's 8, `app-docs:synced`, `audit-log:redacted`,
  `email-domain-mapping:provider-changed`, `permission:defined/deleted`,
  `serviceaccount:token-minted`, the developer-credential pair, dispatch-job cancel/complete and
  `dispatch-jobs:resent`); they are listed as known gaps the test only lets shrink.
- **Tests**: `client_event_codes_migration_test` (rename, ids kept, binding, schemas, edited
  schema kept, re-seed adds the rest once, re-run no-op; the clash case),
  `emitted_catalogue_test` (2 tests, no database).

## F4. Service-account writes

- **Found**: deactivate, assign roles, both regenerations, the PUT auth token and the mint already
  asked `service-account:update` and anchor (ruling (f) held), but anchor first (Go now asks the
  permission first everywhere); update and delete asked permission then anchor. No write confined
  an application-scoped caller: an application's own (anchor-tier) service account holding the
  permission could re-role, rotate or mint for a platform-wide account or another application's,
  and create a platform-wide account.
- **Fix**: `checks::can_update_service_accounts` is the permission, then anchor (the
  `can_update_service_account_details` variant folded into it); `authorize_write` (Go
  `authorizeWrite`) on update, delete, deactivate, assign roles, `regenerate-auth-token`,
  `regenerate-signing-secret` (and their Go alias spellings), the PUT auth token and the mint: the
  operation's permission and anchor, then, for an application-scoped caller (its stored scope is
  not all-applications), the account must be bound to an application it reaches (403
  `APPLICATION_ACCESS_REQUIRED`; an unknown account is 404 there, as Go). Create refuses such a
  caller a platform-wide account or another application's
  (`checks::require_service_account_reach`), before Rust's own 400 for a create naming an
  application (unchanged). `ServiceAccountAdminState` gets `app_access`.
- **Rust semantics**: the application scope is the stored principal's
  (`ApplicationAccessService::scope_for`), as for the ingest endpoints: a token whose principal is
  not stored reaches no application, so the existing tests that create or rotate service accounts
  with an unstored caller now use a stored all-applications one (`anchor_admin_token_all_applications`,
  `persist_all_applications`, or the file's `stored_caller`): `application_scope_test` (1 test),
  `audit_redaction_test` (1), `go_field_gaps_test` (3, and its `known_admin`), `go_routes_test` (3),
  `iam_authority_test` (2), `oauth_secret_rotation_test` (1), `principal_go_parity_test` (1),
  `service_account_delete_test`, `service_account_scope_test` (every admin token). The last read
  a token's tier through `regenerate-auth-token`, which asked anchor first; it now reads it through
  `POST /api/clients` (`anchorWith`: anchor, then the permission).
- **Already bounded here**: Go's new finding "service-account role assignment is unbounded" does
  not apply: Rust's `AssignRolesUseCase` keeps the role ceiling (`ROLE_ABOVE_CALLER`, owner
  ruling 14).
- **Tests**: `service_account_app_reach_test` (2 tests: the confinement on every write and on
  create, its own application's account managed, all-applications not confined; every write
  `PERMISSION_REQUIRED` then `ANCHOR_REQUIRED`).

## F5. Code-first sync client identifier

- **Fix**: `SyncConnectionsUseCase` and `SyncSubscriptionsUseCase` look the sync client's
  identifier up once (`client_identifier`, as create) and store it on the rows they create; rows
  they update are not backfilled.
- **Test**: `code_first_sync_identifier_test`.

## F6, F7. SPA

- `fb36438` applied (the dashboard hunk by hand: this copy has the audit-redaction card) and
  `18c0c27` (F7's SPA half; `UserDetailBody.vue` by hand: it had no auth store import). Rust's
  BFF gates match `utils/admin-permissions.ts` except that Rust's role writes also admit
  `role:manage` (decision #25), which the SPA does not offer: a `role:manage`-only user sees no
  role editors though the API admits it (kept as Go's SPA). The role permission is
  `user:assign-roles` on the API already (`can_assign_principal_roles`).
- **Checks**: `pnpm test` 23 files, 92 tests (Go's `admin-permissions.test.ts` included),
  `vue-tsc -b`, `oxlint` (the one existing warning).

## (d) Process update `NO_CHANGES`

- No test, SPA code or SDK reads `NO_CHANGES` for processes. Aligned to Go: an edit that changes
  nothing is saved (204, a `ProcessUpdated` event). The other fields are applied as before.
  Rust still ignores a blank `diagramType` (Go stores what is sent): not changed.
- `NO_CHANGES` remains on client, role and scheduled-job updates (not in this batch).
- **Test**: `process_lifecycle_test` (a no-op edit is 204).

## (c) fc-web (Topcoat `/ui` trial)

- **Per-operation gates**: every `can_write_*` gate became the API's per-operation check:
  dispatch pools (create; update, suspend, activate), subscriptions (create; update, pause,
  resume), applications (create; update, activate, deactivate), event types (create; update,
  schema finalise/deprecate and archive are `can_update_event_types` as the BFF; delete
  `can_delete_event_types`), users (create and the new-user domain check `can_create_principals`;
  every drawer write `can_update_principals`). The buttons and drawers show what the gate admits.
- **Deactivate** a client now runs `DeactivateClientUseCase` (INACTIVE with the reason "Manual
  deactivation"), not the delete.
- **Sync Platform Events** is gated, button and route, on `can_sync_platform_event_types` (the
  BFF's gate), separately from Create.
- **Checks**: `CARGO_TARGET_DIR=target cargo test --manifest-path crates/fc-web/Cargo.toml`
  compiles and passes `auth_convention_test` (4 tests; fc-web has no other tests, so the pages'
  behaviour is not exercised).

## Verified

- **Before the fixes**: the base sources of the server-side items (the 14 changed source files
  checked out from `c8375ef1`, the new tests kept) against the 10 new or changed tests: 8 failed,
  each at its bug's assertion (F1: another application's firing 204; F2: a client user's create
  201; F3: the iam client codes catalogued, the re-seed leaving 14 client types; F4: an
  application-scoped deactivate 204, and `ANCHOR_REQUIRED` before `PERMISSION_REQUIRED`; F5: no
  identifier; (d): 400 `NO_CHANGES`). The other 2 pass there by design:
  `emitted_catalogue_test::every_emitted_event_type_is_catalogued` (the old catalogue was a
  superset) and the migration's clash case (it tests the migration SQL, not the sources). The SPA
  and fc-web changes were not run against their old versions.
- **After**: the full `it` suite on the final commit, `FC_TEST_PG_BIN` harness, Docker-container
  tests skipped: **451 passed, 0 failed** (27 filtered out: `api_integration_tests`,
  `login_backoff_integration_tests`, `webauthn_integration_tests`, `id_roundtrip_test`,
  `function_table_prefix_test`, the LocalStack scheduler test). The first full run had 21
  failures, all from F4's confinement meeting unstored test callers (19) and the two convention
  scanners (2); fixed as described and re-run green.
- `route_table_snapshot_test`: only the OpenAPI document hashes changed (F2's description);
  regenerated with `UPDATE_ROUTE_SNAPSHOT=1` in `8a3f4003`.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, fc-dev included (ruling (b)).
  `cargo fmt --all -- --check`: clean.
- `FC_TEST_PG_BIN=… scripts/sqlx-prepare.sh`: `.sqlx/` unchanged (no new checked query, no
  stale entry to prune); `scripts/sqlx-prepare.sh --check`: passes.
- Lib unit tests (fc-platform-core, -iam, -messaging, -scheduled-jobs, fc-migrations): 889 passed, 0 failed (125, 450, 270, 37, 7).
- Frontend: `pnpm test` 23 files / 92 tests, `vue-tsc -b`, `oxlint` (the one existing warning);
  `pnpm api:generate` reproduces `types.gen.ts` byte for byte.

## Not verified

- No browser run of the SPA or fc-web.
- Migration 068 ran on the harness's scratch databases only, never on a deployed one.
- `cargo build -p fc-dev` (codegen and link) stalled in this environment (rustc idle on the
  jobserver for 30+ minutes, twice) and was stopped; fc-dev is verified by `cargo clippy
  --workspace --all-targets` (type check and lints), not by a built binary.
- The SDK OpenAPI copies (`clients/*/openapi`) were not refreshed.
- The Docker-container `it` tests (27) were not run: the brief said Docker is not available here.
  `docker ps` did answer (OrbStack) during this work; they were skipped as instructed.
- `pnpm api:generate` also rewrites one line of `src/api/generated-functions/types.gen.ts` from
  the functions document (unrelated drift, not committed).
- Builds here repeatedly stalled with rustc idle (fc-dev, and the `it` test binary twice); the
  test runs that completed used `CARGO_INCREMENTAL=0`. Nothing indicates a code cause.

## Owner decisions

1. **Stale iam client event types on Rust-seeded databases** (F3): 068 leaves both rows where the
   emitted code already exists, so the six `platform:iam:client:*` rows stay (never emitted) and a
   subscription bound to one never fires. Delete them, or re-point their bindings to the admin
   rows?
2. **33 emitted, uncatalogued event types** (F3): listed in `emitted_catalogue_test`; catalogue
   them (with schemas), as Go has the same open item for its 79?
3. **Application scope of an unstored principal** (F4): Rust reads the stored scope, so a token
   whose principal has no row is application-scoped and confined (as the ingest endpoints
   already do); Go reads the token's claim. Same result for every real caller; tests adjusted.
4. **`role:manage`-only users** (F6): Rust's BFF role writes admit `role:manage` (decision #25),
   Go's SPA checks create/update/delete only, so such a user sees no role editors. Kept as Go's
   SPA.
5. **`NO_CHANGES` elsewhere** ((d)): client, role and scheduled-job updates still answer 400
   `NO_CHANGES` for a no-op edit; Go: to check, not changed here.
6. **Process `diagramType`** ((d)): a blank one is ignored here, Go stores what is sent.
7. **`scheduled-job:manage`** (F1): no longer admits the runner reports (Go's gate); if a custom
   role relies on manage alone for `/log` or `/complete`, it is refused now.

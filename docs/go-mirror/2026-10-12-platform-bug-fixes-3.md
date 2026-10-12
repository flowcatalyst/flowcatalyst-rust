# Go platform bug fixes, third batch, mirrored (2026-10-12)

Two items from `flowcatalyst-go/docs/platform-bug-fixes.md`:

- **A. F9 follow-up** (Go `45537af`, `4c23adf`, docs `b3d2c45`; owner ruling 2026-10-11): delete
  every catalogue entry nothing emits, fix the older schemas against what the use cases emit, and
  align two catalogue names.
- **B. F8 extras** (Go `09b33e8`, `70c9ca7`, `e08eee4`, `8b1cb0f`): the role-ceiling paths Go
  bounds that Rust did not.

The earlier batches are `2026-10-10-platform-bug-fixes.md`, `2026-10-11-platform-bug-fixes-2.md`
and `2026-10-11-event-catalogue.md`.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`FC_TEST_PG_BIN=<postgres bin> cargo
test -p fc-platform --test it -- --include-ignored <module>::`).

## Summary

| Item | Go | Rust commit |
|---|---|---|
| A1. 15 unemitted event types pruned, migration 069, catalogued ⊆ emitted test | `45537af` | `cfdadfab` |
| A2. 32 older seeded schemas corrected, migration 070 | `4c23adf` | `5ba0f615` |
| A3. "OAuth Client …" and "IdP Role Mapping …" names | `8ddaed4` (`groupAs`) | `4beaa746` |
| B1. Service-account token mint bounded by the role ceiling | `09b33e8` | `462d6158` |
| B2. Service-account provisioning bounded by the role ceiling | `8b1cb0f` | `50ce04f4` |
| B3. Application-scoped principal sync bounded by the role ceiling | `e08eee4` | `1dc0b24a` |

## A1. Catalogued codes nothing emits

- **Found**: catalogued minus emitted (the new `emitted_catalogue_test` scan) is 15 codes:
  `platform:iam:role:{created,updated,deleted}`, `platform:iam:roles:synced`,
  `platform:iam:anchor-domain:{created,deleted}`, `platform:iam:auth-config:{created,updated,deleted}`,
  `platform:admin:idp:{created,updated,deleted}`, `platform:admin:edm:{created,updated,deleted}`.
  Checked by a whole-repo grep (crates, bin, frontend, clients, conformance, harness): each code
  appears only in the seed and in docs (`docs/parity/domain-events-vs-go.md`, which maps them to
  the emitted codes); the `platform:iam:role:*` and `platform:iam:auth-config:*` hits elsewhere are
  permission names (`:view`, `:create`, ...). A read-only audit of every event struct found no
  emitter either.
- **Differs from Go**: Go pruned 16; `platform:iam:user:password-reset-requested` is emitted here
  (`PasswordResetRequested`, `principal/operations/events.rs`) and stays catalogued.
- **History differs from Go**: Rust emitted the 15 codes until `cb1b3e4f` (2026-09-25), when its
  events took Go's codes (`platform:admin:role:*`, `platform:admin:roles:synced`,
  `platform:admin:anchor-domain:*`, `platform:admin:auth-config:*`,
  `platform:admin:identity-provider:*`, `platform:admin:email-domain-mapping:*`). So a Rust
  database may hold stored events of these codes, and a subscription bound to one did receive
  events before that date. Stored events are not touched.
- **Fix**: the seed drops the 15 (definitions and schemas). Migration **069**
  (`migrations/069_prune_unemitted_event_types.sql`) is Go's 071 Up section with this platform's
  15 codes: it deletes the rows with their schema versions and moves every binding naming one of
  the codes exactly, or pointing at one of the rows, to `msg_subscription_event_types_archive`
  (Go's table and its reason `071_event_type_not_emitted`, so either platform reads the archive the
  same). Bindings are **not re-pointed** at the emitted equivalent (the brief, as Go). Wildcard
  bindings (`platform:iam:role:*`) and bindings to emitted types stay. Forward-only runner: Go's
  Down is not carried. Idempotent, so a database Go already migrated past 071 runs it as a no-op
  (Go's set is a superset). `fc-dev fresh` drops the schema, so nothing to truncate there.
- **Tests**: `emitted_catalogue_test::every_catalogued_event_type_is_emitted` (new, catalogued ⊆
  emitted, beside `every_emitted_event_type_is_catalogued`); `prune_event_types_migration_test`
  (069 on an embedded-PG scratch database: two dead types with schemas and an emitted one; bindings
  by row and code, by code only, wildcard, to the emitted type; the dead types and schemas gone,
  the emitted kept; the two bindings archived exactly as they were, under their ids; the others
  kept; the seed does not add them back and keeps the password-reset request; re-run no-op).

## A2. Older schemas that reject what Rust emits

- **Found**: a read-only audit of every catalogued, non-function code against its emitting struct
  (serde attributes included), then a committed check with real events. 22 seeded schemas reject
  real emitted data, 7 more misdescribe it, plus the two Rust-only divergences the last ledger
  listed:

  | Codes | Was | Emitted |
  | --- | --- | --- |
  | `platform:admin:dispatch-pool:{created,updated,archived,deleted}` | `dispatchPoolId`; `clientId`, `rateLimit`, `concurrency` | `poolId` + `code`/`name` |
  | `platform:admin:eventtype:schema-{added,finalised,deprecated}` | `version`; added also `mimeType`, `schemaType` | `specVersion` (finalised + optional `deprecatedVersion`) |
  | `platform:admin:eventtype:updated` | `name` optional | `name` always, `description` optional |
  | `platform:iam:user:created` | `emailDomain`, `name`, `scope`, `isAnchorUser`, `clientId` | `{principalId, email}` |
  | `platform:iam:user:{updated,deactivated,deleted,password-reset-completed}` | updated `email`, deactivated `reason`, reset `email` never sent; deleted lacked `email` | `principalId` (+ `name` / `email` as sent) |
  | `platform:iam:application:{created,updated,activated,deactivated,deleted}` | `applicationType`, `description`, `code`, `name` never sent | `applicationId` + `code`/`name` as sent |
  | `platform:iam:serviceaccount:{created,updated}` | `applicationId`, `clientIds`, `description`, `clientIdsAdded/Removed` | `{serviceAccountId, code, name}` / `{serviceAccountId, name}` |
  | `platform:admin:connection:{created,updated,deleted}` | `endpoint`, `serviceAccountId`, `clientId`, `status`; updated required `code` | `connectionId` + `code`/`name` as sent |
  | `platform:admin:subscription:{created,updated,paused,resumed}` | `connectionId`, `eventTypes`, `clientId`, `eventTypesAdded/Removed`; paused/resumed `code` | `subscriptionId` (+ `code`, `name` as sent) |
  | `platform:admin:roles:synced` | `syncedCodes` `["array","null"]` | left out when empty, never null (Go's `optStrArray`) |
  | `platform:admin:oauth-client:secret-rotated` | `previousSecretExpiresAt` `["string","null"]` | RFC 3339 string or left out (Go's `optDateTime`, `format: date-time`) |

  The 28 Go codes' corrected schemas equal Go's 072 corrected forms byte for byte as JSON (checked
  by parsing Go's migration). `platform:iam:client:applications-updated` was already right here
  (null for an empty list).
- **Seeded forms**: the seeder never replaces an attached 1.0 (or legacy `v1`). Every historical
  form of `platform_event_schemas.rs` (all 12 commits that touched it, compiled and dumped) was
  compared with the corrected seed: 32 codes have one older seeded form each. For the 28 Go codes
  it is Go's old form (the Rust seed copied Go's); `platform:admin:subscription:synced` (lacked
  `clientId`) and `platform:iam:user:logged-in` (another payload) from the first seed `0b90178c`;
  `roles:synced` and `secret-rotated` from `34488c57`; and Go's older
  `platform:iam:client:applications-updated` (non-null arrays), which a Go-seeded database holds.
- **Fix**: the seed holds the corrected schemas (`opt_str_array` is now Go's `optStrArray`, its only
  user; new `opt_date_time`; `opt_u32` lost its last user). Migration **070**
  (`migrations/070_platform_event_schema_fixes.sql`) replaces each of the 32 while the stored 1.0
  (or `v1`, as the seeder treats it) still equals the seeded form; an edited schema is left alone.
  Idempotent; on a database Go migrated past 072 the 28 Go codes are already corrected.
- **Tests**: `event_catalogue_schema_test::emitted_data_of_the_corrected_codes_validates` (real
  events of all 32 codes, optional fields present and absent, against the seed);
  `every_code_migration_070_corrects_is_sampled` (fails on a 070 code without a sample);
  `the_corrected_schemas_reject_the_old_shapes`; the validator learns `format: date-time`.
  `event_schema_fixes_migration_test` (no database: each 070 row's corrected schema is the seed's;
  embedded PG: every seeded form replaced, an edited one kept, a `v1` replaced and a `1.1` not,
  re-run no-op, the seed attaches no second schema). The seed's unit tests: the count (144),
  `user:created` has exactly its two fields.

## A3. Names

`platform:admin:oauth-client:*` is "OAuth Client …" and `platform:admin:idp-role-mapping:*` "IdP
Role Mapping …", Go's `groupAs` labels (were "Oauth Client …", "Idp Role Mapping …"). The seeder
refreshes names on start, so existing rows take them on the next boot; no migration. Test:
`seed::platform_event_types::tests::go_catalogue_names_are_kept`.

## B. F8 extras

Rust already had the ceiling (`role::ceiling`) on service-account role assignment, user role
assignment, the platform-route user sync, IdP role mappings, email-domain allowed roles and role
permission edits, and the provisioning gate (anchor + `service-account:create` +
`application:update`). Go's F8 section lists three places where Go goes further; all three were
missing here and are added, with Go's code, status and wording.

### B1. Token mint

- **Found**: `POST /api/service-accounts/{id}/token` (`mint_service_account_token`) flattened and
  minted the account's whole authority for any holder of `service-account:update` (+ anchor and
  application reach): a token for an account a super-admin made stronger than the caller.
- **Fix**: after the active checks and before flattening (Go's order),
  `require_mint_within_caller` refuses when any role the account holds has a platform permission
  the caller lacks: 403 `ROLE_ABOVE_CALLER`, "You may only mint a token for a service account
  whose roles' permissions you hold yourself: <roles>" (Go's message). Nothing is minted or
  recorded.
- **Test**: `role_ceiling_extras_test::the_token_mint_is_bounded_by_the_callers_permissions` (an
  IAM administrator refused for a super-admin account, the exact message, no mint event; a
  super-admin mints; the same administrator mints once the account holds a role within its
  permissions).

### B2. Provisioning

- **Found**: `POST /api/applications/{id}/provision-service-account` and the fc-web `/ui` button
  had the gate but gave the new account `platform:application-service` without asking whether the
  caller holds that role's permissions.
- **Fix**: `application::api::require_provision_within_caller` (Go
  `RequireProvisionWithinCaller`) is the ceiling on that role, called by both handlers after their
  gate and before the application lookup (Go's order); 403 `ROLE_ABOVE_CALLER` with the standard
  role message. fc-dev's start-up provisioning writes its rows directly (the system), as Go's
  `fcdev init`, and is not bounded.
- **Test**: `role_ceiling_extras_test::provisioning_is_bounded_by_the_application_service_role`
  (the gate alone refused, nothing provisioned; the gate plus the role's permissions provisions; a
  super-admin provisions).

### B3. Application-scoped principal sync

- **Found**: the target-user reach (403 `SYNC_TARGET_FORBIDDEN`, Java S1.3) and the name rule (403
  `ROLE_APP_FORBIDDEN` for `platform:` or another application's prefix) were already here, but no
  ceiling. The name rule admits the application's own roles, unprefixed names and unknown prefixes,
  and an application's role can carry platform permissions (a super-admin's cross-application edit
  through the admin API, which `require_confined` allows, or a role written before the
  confinement rule). So an application's service account could sync such a role onto any user it
  reaches.
- **Fix**: `SyncPrincipalsUseCase` collects every role the sync adds or removes (each listed
  user's change, a created user's roles, the `removeUnlisted` sweep's removals) and applies
  `ceiling::require_roles` before anything is written: 403 `ROLE_ABOVE_CALLER`, the whole sync
  refused. Roles holding only application permissions pass for any caller, so an SDK syncing its
  own roles is unaffected. `SdkSyncState` gets `role_repo`.
- **Divergence kept**: Go has no name rule on this route and relies on the ceiling alone; Rust
  keeps both (the name rule answers first).
- **Test**: `role_ceiling_extras_test::the_application_principal_sync_is_bounded_by_the_callers_permissions`
  (adding, removing from a listed user and sweeping an unlisted user's role above the caller all
  refused with nothing written; the application's own role passes and a kept role is not checked;
  a super-admin may).

## Wire impact

- No shape change, no OpenAPI change (no `utoipa` annotation touched). New 403
  `ROLE_ABOVE_CALLER` on the mint, provisioning and the application principal sync.
- `GET /api/event-types` and Sync Platform Events lose the 15 codes; the 32 schemas served change
  to the emitted shapes (on existing databases, after 070). Event payloads are unchanged.

## Verified

All database tests ran on the `FC_TEST_PG_BIN` harness (embedded PostgreSQL 18.3 from the Go
repo's archive, a private cluster on a free loopback port); no dev database was touched.

- **Before the fixes**: the base sources (`e3d8ff8b`) with the new tests and the two migration
  files: **9 failed, each at its bug's assertion**:
  `every_catalogued_event_type_is_emitted` (the 15 codes listed);
  `emitted_data_of_the_corrected_codes_validates` (`user:created: missing required emailDomain`,
  ...); `the_corrected_schemas_reject_the_old_shapes` (the old dispatch-pool shape accepted);
  `the_corrected_schemas_are_the_seeds` and `migration_070_replaces_the_seeded_schemas_only` (the
  seed's schema is the old one); `migration_069_...` ("platform:iam:role:created came back" after
  the seed); the mint (200), provisioning (201) and application sync (200) tests. 6 passed: the
  existing tests of those modules and `every_code_migration_070_corrects_is_sampled`, which reads
  the migration file only. The name test (A3) is a lib unit test inside the seed file; its old
  names are those the last ledger records.
- **After, full suite** (run before the last cosmetic edits: rustfmt, a helper moved, a type alias
  clippy asked for, two seed comments): **464 passed, 2 failed**, 26 filtered out
  (`api_integration_tests`, `login_backoff_integration_tests`, `webauthn_integration_tests`,
  `id_roundtrip_test`, `function_table_prefix_test`). The 2 are unrelated:
  `dispatch_claim_plan_test::plans_empty_partitions_vacuumed_200k` (a timing bound: the backlog
  sample took 419 ms against 250 ms on a loaded machine, rows as expected) and
  `dispatch_scheduler_test::the_scheduler_publishes_to_its_tenants_sqs_fifo_queue` (starts a
  LocalStack container; Docker-only).
- **After, final commit** (`1dc0b24a`): the `it` modules `emitted_catalogue_test`,
  `event_catalogue_schema_test`, `event_schema_fixes_migration_test`,
  `prune_event_types_migration_test`, `role_ceiling_extras_test`,
  `client_event_codes_migration_test`, `iam_authority_test`, `go_routes_test`,
  `service_account_app_reach_test`, `application_scope_test`, `bff_sync_platform_test`: **60
  passed, 0 failed**. Lib unit tests: fc-platform-iam 450, fc-platform-messaging 270,
  fc-migrations 7, all passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (after the type alias).
  `cargo fmt --all -- --check`: clean. `cargo build --workspace`: built, fc-dev and fc-server
  included (the one warning is the existing linker note on fc-server's `__eh_frame` size).
- `FC_TEST_PG_BIN=… scripts/sqlx-prepare.sh --check`: passes (its `fc-migrate` step ran the full
  migration list, 069 and 070 included, on an empty database; no checked query added).

## Not verified

- Migrations 069 and 070 ran on the harness's scratch databases only, never on a deployed one, and
  not against a database Go has already migrated (reasoned idempotent, not run).
- Builds here stalled for long stretches with rustc idle (each proc-macro `dlopen` waiting on
  macOS `syspolicyd`); the runs completed with `CARGO_INCREMENTAL=0`, outside the command sandbox.
- No browser run of fc-web's provisioning refusal (it flashes the error, as for any refusal).

## Owner decisions

1. **Bindings to the 15 codes on Rust databases** (A1): unlike Go's case, Rust did emit these codes
   until `cb1b3e4f`, so a binding archived by 069 once delivered events. As briefed (and as Go),
   it is archived, not re-pointed at the `platform:admin:*` equivalent. Re-point instead?
2. **Archive reason** (A1): 069 writes Go's reason `071_event_type_not_emitted`, so Go's 071 Down
   would also restore bindings Rust archived. Kept for one archive both platforms read the same.
3. **Two name rules on the app sync** (B3): Rust keeps `ROLE_APP_FORBIDDEN` beside the ceiling; Go
   has only the ceiling.

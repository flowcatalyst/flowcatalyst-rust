# Go platform bug fixes, sixth batch, mirrored (2026-10-14)

Go's F15, the SDK role sync ceiling and unknown role names in the application principal sync
(`flowcatalyst-go/docs/platform-bug-fixes.md` F15 and decision 22;
`flowcatalyst-go/docs/wire-contract.md` "SDK sync: roles"; owner rulings 2026-10-13). Go
`2ba9763` (ruling 1's test; the rule has been in Go since F10, `c56a68c`) and `3aeb08c` (ruling 2).
Every commit is on `idiomatic-rust`, not pushed.

The earlier batches are `2026-10-10-platform-bug-fixes.md`, `2026-10-11-platform-bug-fixes-2.md`,
`2026-10-11-event-catalogue.md`, `2026-10-12-platform-bug-fixes-3.md`,
`2026-10-13-platform-bug-fixes-4.md` and `2026-10-14-platform-bug-fixes-5.md`.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`FC_TEST_PG_BIN=<postgres bin> cargo
test -p fc-platform --test it -- --include-ignored <module>::`).

## Summary

| Item | Go | Rust commit |
|---|---|---|
| 1. SDK role sync bounded by the caller's permissions | `c56a68c` (F10), test `2ba9763` | `e76d1d45` |
| 2. Application principal sync skips names that are not roles, `skippedRoles` | `3aeb08c` | `40e75703` |
| Ledger | `96a15cc` | (this file) |

The fourth batch listed both as "still different from Go" (its "F10 permission ceiling on the two
role syncs" and "Unknown role names on the application principal sync"); item 1 closes the SDK
half of the first, item 2 the second.

## 1. SDK role sync ceiling (`e76d1d45`)

**Found**: `role/operations/sync.rs` (`POST /api/applications/{appCode}/roles/sync`) applied no
permission ceiling; a caller holding only `platform:application-service:role:*` could give a role
of the `platform` application `platform:*:*:*`. Confirmed by the new test before the fix (the sync
answered 200, `created: 2`).

**Rule** (as Go `SyncRoles`, F15 ruling 1):

- per synced role: an existing SDK-sourced role with a **non-empty** `permissions` contributes
  `ceiling::changed(stored, sent)` (added, then removed); an empty or omitted list keeps the stored
  permissions and contributes nothing; a new role contributes all its permissions; a role
  `removeUnlisted` deletes contributes all its permissions; a same-named CODE or DATABASE role is
  skipped untouched and contributes nothing;
- the collected changes are checked once with `ceiling::require_permissions` (the role-edit
  routes' rule: only `platform:` permissions; held by `Authority::has_permission`, segment
  wildcards; `platform:*:*:*` holds all);
- order: confinement first (400 `PERMISSION_OUTSIDE_APPLICATION`, per role, unchanged), then
  `ROLE_HAS_ASSIGNMENTS`, then the ceiling: 403 `PERMISSION_ABOVE_CALLER`, "You may only add or
  remove permissions you hold yourself: `<p1>, <p2>`", each named once in collection order (per
  role in payload order its added then its removed permissions, then the deleted roles'
  permissions). The whole sync is refused and nothing written (the plan is built before the one
  `commit_sync`).

**Tests** (`sdk_role_sync_ceiling_test`):

- `the_sdk_role_sync_is_bounded_by_the_callers_permissions` (Go
  `TestSyncRoles_PlatformApplicationAdditionsBoundedByCaller`, on the `platform` application, plus
  the F15 cases Go's test does not reach): a held permission syncs; a new role with
  `platform:*:*:*` refuses the whole sync (the held role before it not created); an addition to an
  existing role refused, unchanged; the same list, an empty list and an omitted list re-synced by
  a caller lacking the kept permission; a removal refused; a `removeUnlisted` deletion refused, the
  role kept; a same-named CODE role given `platform:*:*:*` is skipped untouched (not refused, not
  changed); a holder adds the permission; the collection order across an update, a new role and a
  deletion ("platform:iam:client:view, platform:iam:user:delete, platform:iam:client:delete,
  platform:iam:role:delete"), nothing written; a super-admin may do all of it.
- `the_ceiling_runs_after_confinement_and_assignments`: application permissions pass for a caller
  holding none; a platform permission on an ordinary application is 400
  `PERMISSION_OUTSIDE_APPLICATION`; an assigned role `removeUnlisted` would delete, above the
  caller, is `ROLE_HAS_ASSIGNMENTS`.
- **Failing first**: before the fix the first test failed at "new role" (200, `created: 2`).
- **Mutants** (each killed by the first test): a deleted role's permissions not counted; a new
  role's not counted; removals from an existing role ignored; an empty list counted as removing
  everything.

## 2. Unknown role names in the application principal sync (`40e75703`)

**Found**: `principal/operations/sync.rs` refused a name prefixed `platform:` or with an existing
application's code (403 `ROLE_APP_FORBIDDEN`) whether or not it was a role, and stored every other
name as an `SDK_SYNC` assignment whether or not it was a role (`iam_principal_roles` has no FK on
the name), which becomes authority the moment a role of that name is created.

**Rule** (as Go, F15 ruling 2), `POST /api/applications/{appCode}/principals/sync` only, per
entry, after the entry's target-reach check (`SYNC_TARGET_FORBIDDEN`, unchanged):

1. each role name is lower-cased and matched exactly against role names; no `app:` prefix is
   added (a bare `admin` is not `app:admin`);
2. a role whose owning application (`AuthRole::owning_application_code`) is not `appCode`, a
   platform role included: 403 `ROLE_APP_FORBIDDEN`, "role '`<name>`' does not belong to
   application '`<appCode>`'", the whole sync refused, nothing written;
3. a name that is not a role, whatever its prefix: not assigned, not refused, nothing created; the
   entry syncs with its other roles. An entry whose every name is skipped is still created or
   updated, and its existing user's `SDK_SYNC` set for this application becomes empty (the merge
   replaces it with the empty set);
4. unchanged: the merge and sweep (`is_own_sdk_role`), the role ceiling (`ROLE_ABOVE_CALLER`; a
   skipped name is on neither side of it), `passwordHashIgnored`, and the platform route's
   anchor-only tier (owner: leave it).

The skipped names are logged (`warn`, with the application) and returned as **`skippedRoles`** on
the shared `SyncResultResponse`: lower-cased, each name once across the request, in payload order,
omitted when empty (so every other sync's response is unchanged). They ride
`PrincipalsSynced.skipped_roles`, `#[serde(skip)]`, so the event's data and the persistence
snapshot are unchanged (`split_tests::event_persistence_snapshot::principals_synced` now sets the
field and still matches). The role lookup is one `find_by_codes` query for every distinct name in
the payload (replacing the prefix lookup in `find_ids_by_codes`).

**Order**: the reach check moved from "every listed existing user first" into the entry loop,
before the entry's roles, as Go. Only the first refusal of a request with two faults changes.

**Platform route** (`POST /api/principals/sync`, `sync_users.rs`): unchanged; an anchor's unknown
name is still stored, and its response has no `skippedRoles`.

**OpenAPI**: `SyncResultResponse.skippedRoles` (optional `string[]`) and the corrected
`SyncPrincipalInputRequest.roles` description, both with Go's text; the served document's two
properties equal Go's `api/openapi.lock.json` (compared as parsed JSON from `OPENAPI_DUMP`). The
vendored Go lock `frontend/openapi/openapi.json` and `frontend/src/api/generated/types.gen.ts` take
Go `3aeb08c`'s hunks (the description, and `skippedRoles` before `syncedCodes`); see Divergences
for `passwordHashIgnored`. The route snapshot takes the new document hashes (nothing else
changed).

**Tests**:

- `principal_sync_unknown_roles_test::the_application_sync_skips_names_that_are_not_roles` (Go
  `TestSyncPrincipals_AppRouteSkipsUnknownRoles`, plus Rust's old refusals): own-prefixed
  (`unkrolesdk:not-yet-synced`, and the same mixed-case in another entry), bare (`Unprefixed`,
  `unprefixed`), unknown-application (`ghostapp:admin`), platform-prefixed (`Platform:Nope`) and
  existing-application-prefixed (`unkroleother:ghost`) unknown names skipped; the known role
  assigned; the all-unknown entry still created with no role; `skippedRoles` exactly
  `["unkrolesdk:not-yet-synced","unprefixed","ghostapp:admin","platform:nope","unkroleother:ghost"]`;
  once the role is synced the same sync assigns it and the field is absent; an existing user whose
  every name is skipped loses this application's `SDK_SYNC` role and keeps another application's
  and a non-SDK role; another application's role and a platform role (`Platform:Viewer`) beside an
  unknown name refuse the whole sync with the exact code and message, an earlier valid entry not
  written.
- `principal_sync_unknown_roles_test::the_platform_sync_is_unchanged`.
- `iam_authority_test::application_sync_keeps_to_its_own_roles`: the roles it refuses and assigns
  are now created (`platform:super-admin`, `rfp:buyer`, `hr:manager`); `employee` and
  `legacy:thing` are skipped and reported instead of stored.
- `iam_authority_test::application_sync_stays_within_the_callers_reach`: creates `hr:manager`;
  gains the per-entry order (an earlier entry's platform role is `ROLE_APP_FORBIDDEN` before a
  later entry's out-of-reach target; reversed, `SYNC_TARGET_FORBIDDEN`).
- **Failing first**: before the fix the new test failed at its first request, 403
  `ROLE_APP_FORBIDDEN` "role 'platform:nope' does not belong to application 'unkrolesdk'".
- **Mutants** (each killed): `skippedRoles` not deduplicated; a known foreign role not refused; an
  unknown name still assigned; the field not wired into the response; the reach check over every
  listed user first.

## Divergences from Go

- **Stored permission order** (1): a role's permissions are a set here, so the removed and
  deleted permissions are named in sorted order; Go names them in stored order. Added
  permissions follow the payload, as Go.
- **Confinement on the SDK role sync** (1, unchanged from the fourth batch): Rust checks every
  permission a role is sent, for every role in the payload, before anything else (as Java); Go
  checks only what an SDK role gains, and skips a same-named CODE or DATABASE role. A payload that
  sends a CODE role's name with a foreign permission is 400 here and skipped in Go.
- **Platform role sync** (`/bff/roles/sync-platform` and the boot sync): still no permission
  ceiling here (Go has one since F10). Not part of this brief.
- **Vendored lock** (2): `frontend/openapi/openapi.json` (and the SPA types) predate Go's
  `passwordHashIgnored`, so `SyncResultResponse` there still lacks that property (pre-existing);
  only F15's hunks were taken.
- **Platform principal sync**: anchor-only here (owner: leave it); Go and Java admit a non-anchor.
- **Rust SDK** (`crates/fc-sdk`): not changed; Go `a1d6322` added `skipped_roles` to the Rust SDK
  in `flowcatalyst-go/clients/rust-sdk`.

## Wire impact

- `POST /api/applications/{appCode}/roles/sync`: a change to a platform permission the caller
  does not hold is now 403 `PERMISSION_ABOVE_CALLER` where it succeeded.
- `POST /api/applications/{appCode}/principals/sync`: a name that is not a role is no longer
  assigned; one prefixed `platform:` or with an existing application's code is no longer refused
  unless it is a role; the response may carry `skippedRoles`. No status code added.

## Verified

All database tests ran on the `FC_TEST_PG_BIN` harness (embedded PostgreSQL 18.3, a private
cluster); no dev database was touched.

- Failing first and mutants: as recorded per item.
- `cargo fmt --all -- --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`:
  clean.
- Lib tests: fc-platform-core 134, fc-platform-iam 452, fc-platform 77, all passed.
- `route_table_snapshot_test`: passes after `UPDATE_ROUTE_SNAPSHOT=1` (document hashes only).
- `FC_TEST_PG_BIN=… scripts/sqlx-prepare.sh --check`: passes (no query added; `find_by_codes`
  exists).
- The whole `it` suite with `--include-ignored`, at `40e75703`'s tree: **502 passed, 0 failed**
  (the Docker-only modules that failed in the fifth batch passed this time).
- Builds ran with `CARGO_INCREMENTAL=0`, outside the command sandbox; none stalled this time.

## Not verified

- Embedded databases only, through the route handlers; no deployed platform, no running SDK
  against it, no browser (the SPA only gained a generated type; not type-checked here).
- The `warn` log line was not observed.

# Go platform bug fixes, fourth batch, mirrored (2026-10-13)

Go's F12, role confinement and principal-sync reach (`flowcatalyst-go/docs/platform-bug-fixes.md`
F12 and decision 19; Go `7f87172` rule 1, `2ab9a97` rule 2, `55909de` rule 3; owner approval
2026-10-12), checked here where Rust differs. Go took most of F12 from Java and Rust, so most of it
was already here. Go's F12 ledger lists two Rust gaps it found by grep only, and two Go findings to
check here; each was read against the Rust code and Java (`flowcatalyst-javalin`, read-only).

The earlier batches are `2026-10-10-platform-bug-fixes.md`, `2026-10-11-platform-bug-fixes-2.md`,
`2026-10-11-event-catalogue.md` and `2026-10-12-platform-bug-fixes-3.md`.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`FC_TEST_PG_BIN=<postgres bin> cargo
test -p fc-platform --test it -- --include-ignored <module>::`).

## Summary

| Item | Go | Verdict here | Rust commit |
|---|---|---|---|
| 1. Platform role sync not confined | `7f87172` | **real gap, fixed** | `576ba349` |
| 2. Platform principal sync has no assignable-role check | `55909de` | not a gap: the route is anchor-only here | none |
| 3. Principal sync overwrites an existing user's password hash | Go finding | not here (decision #22, both routes) | none |
| 4. Application principal sync replaces other applications' `SDK_SYNC` roles | Go finding | not here (`is_own_sdk_role`, merge and sweep) | none |

## 1. Confinement on the platform role sync

- **Found**: `RoleSyncService::sync_code_defined_roles` (the boot sync in `fc-server` and `fc-dev`,
  and `POST /bff/roles/sync-platform`) wrote each catalogue role's permissions as given. Create,
  update, grant and the SDK role sync already call `role::operations::require_confined`; this sync
  did not. Java confines it (`Role.syncedFromCatalogue` and `withPermissions` call `confined`), Go
  since `7f87172`. Confirmed by the new test before the fix: a catalogue role given
  `other:thing:view` synced (`RoleSyncCounts { created: 1, updated: 19, .. }`).
- **Fix**: `sync_catalogue(&[AuthRole])` (the catalogue as a parameter, as Go's
  `SyncPlatformRoles`; `sync_code_defined_roles` passes `roles::all()`) checks every catalogue
  role before its first write: a new role's permissions against its application, an existing
  `CODE` role's additions against its owning application (`owning_application_code`). No
  super-admin exception (the catalogue sync never crosses, as Go and Java). Refusal: 400
  `PERMISSION_OUTSIDE_APPLICATION`, "Permission '<p>' does not belong to application '<app>'; a
  role may only hold its own application's permissions", the first offender in sorted order (the
  permission set is unordered here; Go names the first in catalogue order). The writes are not one
  transaction here (they never were), so checking all roles first is what keeps a refused sync
  from writing anything. A role colliding with a non-`CODE` row is skipped as before and not
  checked (it is not written).
- **Change-only, as Go**: an existing `CODE` row that already holds a foreign permission re-syncs
  with the same list and can lose it. Java checks the whole list (decision 19 keeps Go's and Rust's
  rule).
- **BFF**: the handler wrapped every service error in a 500 "Role sync failed"; a 4xx `Coded`
  error now passes through (`sync_platform_roles_error`), so the refusal reaches the caller as
  Go's and Java's 400. Other errors stay 500. Boot logs the error, as before.
- **Practice**: the built-in catalogue is all `platform:` roles holding `platform:` permissions
  (lib test `the_built_in_catalogue_is_confined`), so the boot sync and the BFF route change
  nothing today; the rule stops a future catalogue edit.
- **Tests**: `platform_role_sync_confinement_test` (the built-in catalogue syncs; an own
  permission passes; an existing role gaining `other:thing:view` refused with the exact code,
  status and message, its permissions unchanged and a new role planned before it not created; a
  new role holding one refused and not created; a legacy `CODE` row holding a foreign permission
  re-syncs and loses it; the BFF route still answers 200); lib
  `role::bff::tests::a_sync_refusal_keeps_its_status`,
  `shared::role_sync_service::tests::the_built_in_catalogue_is_confined`.

## 2. Assignable roles on the platform principal sync

- **Go's rule** (Java's `requireSyncableRoles`): on `POST /api/principals/sync` a **non-anchor**
  may name only roles it could assign through `PUT /api/principals/{id}/roles` (400
  `UNKNOWN_ROLE`, 403 `PLATFORM_ROLE_FORBIDDEN`, 403 `ROLE_APP_FORBIDDEN`), so it can give a user
  the sync creates no role; an anchor may name any role.
- **Here**: the route is anchor-only. The handler calls `checks::require_anchor` before
  `can_sync_principals` and before the body, and `SyncUsersUseCase::authorize` repeats it
  (`db4711bd`, 2026-09-25: "it creates and updates client-less users"). A non-anchor gets 403
  "Anchor access required" and nothing is written
  (`iam_authority_test::platform_user_sync_needs_anchor`). Every caller that reaches the role
  names is an anchor, for whom Java and Go check nothing, so the check would be dead code. Not
  added.
- **Divergence for the owner**: Java and Go admit a non-anchor holding a user permission (bounded
  by target reach and the assignable-role rule); Rust refuses it outright. Rust is the stricter of
  the three. To align, the anchor gate would be dropped and Java's assignable-role check added
  (Rust has it for `PUT /api/principals/{id}/roles` in `principal/api.rs`).

## 3. Password hash on an existing user

Not present. Both syncs (`principal/operations/sync_users.rs`, `sync.rs`) apply a `passwordHash`
only to a user they create; on an existing user it is logged and ignored, and both routes report
the email in `passwordHashIgnored` (decision #22, Java's ruling 2026-09-25). Covered by
`app_compat_test::a_sync_never_replaces_an_existing_users_password_hash` (platform route: a
re-sync with another hash keeps the stored one and lists the email) and
`app_compat_test::the_application_sync_carries_the_hash_for_new_users` (application route: the
re-sync's hash is ignored and reported, the first password still signs in).

## 4. Another application's `SDK_SYNC` roles

Not present. The application sync's merge and its `removeUnlisted` sweep both use
`is_own_sdk_role` (`SDK_SYNC` and `{appCode}:`-prefixed), Java's `belongsToApplicationSync`, so
application A's sync keeps application B's synced roles and non-`SDK_SYNC` roles. Covered by
`iam_authority_test::application_sync_keeps_to_its_own_roles` (a listed user keeps `rfp:buyer`
and `platform:viewer` through an `hr` sync; the sweep strips only `hr:employee` from an unlisted
user). The platform route has no application and replaces every `SDK_SYNC` role, as Java's
null-application case and Go's.

## Wire impact

- No shape change, no OpenAPI change. New 400 `PERMISSION_OUTSIDE_APPLICATION` on
  `POST /bff/roles/sync-platform`, reachable only through a catalogue holding a foreign
  permission (none does); the route's other failures stay 500.

## Still different from Go (not changed, for the owner)

- **SDK role sync, whole list**: Rust's `role/operations/sync.rs` checks every permission an SDK
  sync supplies (as Java), Go only what it adds (decision 19). A role holding a foreign permission
  cannot be re-synced here with the same list. Go's F12 ledger item (1).
- **Unknown role names on the application principal sync**: Rust refuses a name prefixed
  `platform` or an existing application's code; Go any foreign prefix; Java every name that is not
  a role (400 `UNKNOWN_ROLE`). Go's F12 ledger item (3).
- **F10 permission ceiling on the two role syncs**: Go bounds the SDK and platform role syncs by
  the caller's permissions (`PERMISSION_ABOVE_CALLER`); Java and Rust do not. Go's F10 lists it as
  "to mirror if the owner keeps it"; out of F12's scope, not done.

## Verified

All database tests ran on the `FC_TEST_PG_BIN` harness (embedded PostgreSQL 18 from the Go repo's
archive, a private cluster); no dev database was touched.

- **Before the fix** (the `sync_catalogue` seam alone, no check):
  `platform_role_sync_confinement_test` failed at "another application's permission" (the sync
  returned `Ok`).
- **After**: `platform_role_sync_confinement_test`, `bff_roles_gate_test`,
  `bff_sync_platform_test`, `iam_authority_test` and `app_compat_test` (the tests items 2 to 4
  cite included): 29 passed, 0 failed. Lib tests: `fc-platform-iam` 452 passed (the two new ones
  included), `fc-platform` 76 passed.
- `cargo fmt --all -- --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`:
  clean. `FC_TEST_PG_BIN=… scripts/sqlx-prepare.sh --check`: passes (no checked query added).
- Not run: the full `it` suite (only the modules above).

## Not verified

- Builds stalled for long stretches with rustc idle while loading proc-macro dylibs (macOS
  `syspolicyd`); runs were restarted until they completed, with `CARGO_INCREMENTAL=0`, outside the
  command sandbox.
- The BFF passthrough of the 400 is covered by the lib test only: the route syncs the built-in
  catalogue, which holds no foreign permission, so no route-level request can trigger it.

## Owner decisions

1. **Platform principal sync tier** (item 2): keep Rust's anchor-only gate, or admit a non-anchor
   holding a user permission as Java and Go do (then add Java's assignable-role check)?
2. The three "still different" items above, each already listed in Go's F12/F10 ledgers.

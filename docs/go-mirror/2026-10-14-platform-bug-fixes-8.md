# Go platform bug fixes, eighth batch, mirrored (2026-10-14)

The permission ceiling on the platform role sync: Go's F10 (`flowcatalyst-go/docs/platform-bug-fixes.md`
F10, the `SyncPlatformRoles` row; `c56a68c`), left out of the earlier batches ("to mirror if the
owner keeps it": fourth batch, "Divergences"; sixth batch, "Platform role sync"). Owner approval
2026-10-14: bring it into `POST /bff/roles/sync-platform` and the boot sync. Every commit is on
`main`, not pushed.

The earlier batches are `2026-10-10-platform-bug-fixes.md`, `2026-10-11-platform-bug-fixes-2.md`,
`2026-10-11-event-catalogue.md`, `2026-10-12-platform-bug-fixes-3.md`,
`2026-10-13-platform-bug-fixes-4.md`, `2026-10-14-platform-bug-fixes-5.md`,
`2026-10-14-platform-bug-fixes-6.md` and `2026-10-14-platform-bug-fixes-7.md`.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`FC_TEST_PG_BIN=<postgres bin> cargo
test -p fc-platform --test it -- --include-ignored <module>::`).

## Summary

| Item | Go | Rust commit |
|---|---|---|
| 1. Permission ceiling on the platform role sync (BFF route and boot sync) | `c56a68c` (F10) | `f5b3ee62` |
| Ledger | | (this file) |

## Go's behaviour, as found

Read in `flowcatalyst-go` (`internal/platform/role/operations/sync.go` `SyncPlatformRoles`,
`internal/platform/role/ceiling.go`, `internal/platform/shared/bff/roles.go` `syncPlatform`,
`internal/platform/seed/seed.go` `seedRoles`; test `role/operations/ops_pg_test.go`
`TestSyncPlatformRoles_BoundedByCallersPermissions`):

- **Caller**: the BFF handler (`RequireAnchor`) runs the operation with the request's
  `AuthContext`; the ceiling reads it (`auth.FromContext(ctx)`). `platform:*:*:*` holds
  everything (`IsSuperAdmin`); a nil caller holds nothing.
- **Boot**: Go has no boot-time run of `SyncPlatformRoles`. The seeder (`seedRoles`) inserts a
  missing built-in role by direct SQL, leaves an existing one alone and sweeps nothing; it acts for
  no caller and is outside the rule (`ceiling.go`: "The system's own role writers (seeding, …) do
  not run through this rule").
- **What is collected**, in this order: per catalogue role, an existing `CODE` row's
  `ChangedPermissions(stored, catalogue)` (added, then removed); a new role's permissions; then
  each swept stale `CODE` row's permissions. A same-named row of another source is skipped
  (logged) and not checked; a stale row still assigned is kept (logged, not refused: the platform
  sync has no `ROLE_HAS_ASSIGNMENTS`) and not checked.
- **Order**: confinement (F12, 400 `PERMISSION_OUTSIDE_APPLICATION`) per role as it is planned,
  then the stale scan, then the ceiling once over everything collected, then the writes
  (`usecaseop.Sync`, one transaction).
- **Rule and refusal**: `role.RequirePermissionsWithinCaller`: platform permissions only, matched
  by `auth.Grants`, each named once in collection order. 403 `PERMISSION_ABOVE_CALLER`, "You may
  only add or remove permissions you hold yourself: <p1>, <p2>". All-or-nothing: the whole sync is
  refused before any write.

## 1. The ceiling on the platform role sync (`f5b3ee62`)

`RoleSyncService::sync_catalogue` and `sync_code_defined_roles`
(`crates/fc-platform-iam/src/shared/role_sync_service.rs`) take the `Caller` the sync runs for:

- `POST /bff/roles/sync-platform` (`role::bff::sync_platform_roles`, after its gate: anchor and
  the three role permissions) passes `Caller::from_auth` of the request.
- The boot syncs (fc-server, fc-dev, and fc-dev's functions e2e harness) pass `Caller::system()`,
  which holds every permission (`Authority for Caller`), so the boot sync is unbounded as before.

The plan pass, before any write, now also collects the change: per catalogue role an existing
`CODE` row's `ceiling::changed(stored, catalogue)` (added, then removed), a new role's permissions;
then the stale scan (`stale_code_roles`, which replaces `remove_stale_code_roles`: same
selection, `CODE` rows not in the catalogue and not assigned, computed before the writes instead
of after them) adds each stale row's permissions. Confinement still runs per role as it is planned,
so it refuses first; then `ceiling::require_permissions(Some(caller), changed)`, the same function,
code, status and message as the role-edit routes and the SDK role sync. A refused sync writes
nothing. The BFF route's error mapping already passes a client error through, so the refusal
reaches the caller as 403 with the standard envelope.

**Tests** (`platform_role_sync_ceiling_test`):
`the_bff_platform_role_sync_is_bounded_by_the_callers_permissions` (HTTP: a wildcard holder
seeds; a non-wildcard anchor holding the route's gate passes when nothing changes; restoring a
built-in role trimmed by hand is 403 with the exact envelope and leaves it trimmed; a holder of the
missing permission restores it; sweeping a stale `CODE` role holding a lacking permission is 403
and deletes nothing; the wildcard holder sweeps it) and
`the_platform_role_sync_is_bounded_by_the_callers_permissions` (the service: the boot sync and a
new role as the system caller; nothing changed passes; an addition refused with a new role planned
before it not created; a new role's permissions; confinement's 400 before the ceiling; the
super-admin passes; removal by trimming and by the sweep refused, nothing written; the message
names additions then removals, each once; a same-named `DATABASE` role and an assigned stale
`CODE` role are untouched and not checked; the system sweeps). The confinement test
(`platform_role_sync_confinement_test`) now runs as the system caller.

**Failing first**: the HTTP test failed before the fix (the non-wildcard anchor restored the
trimmed role: 200 `{"updated":18,…}` where 403 was expected). The service test needs the new
`caller` parameter, so it was not run before the fix; the mutants below cover it.

**Mutants** (11, each killed by at least one of the two tests): the ceiling call removed; an
existing role's change not collected; only its additions collected (removals dropped); a new
role's permissions not collected; the swept roles' not collected; the ceiling moved after the
writes; confinement deferred until after the ceiling; a same-named non-`CODE` role checked; an
assigned stale role swept (and so checked); the BFF route running as `Caller::system()`; a role's
removals named before its additions.

## Divergences from Go

- **Boot sync**: Rust's boot runs the full catalogue sync (updates, sweep) as `Caller::system()`;
  Go's boot seeder is a direct-SQL insert-if-missing that acts for no caller. Both leave the boot
  path unbounded.
- **Message order**: a role's permissions are a set here, so each role's additions, removals, new
  permissions and swept permissions are named in sorted order; Go names them in catalogue and
  stored order. The grouping (per role additions then removals, roles in catalogue order, then the
  swept roles) is Go's.
- **Atomicity** (pre-existing, unchanged): Go writes the whole sync and its events in one
  transaction (`usecaseop.Sync`); Rust writes row by row with no events. A refusal (confinement or
  the ceiling) still writes nothing here, because every check runs before the first write; a
  database failure mid-write can leave a partial sync.
- **Gate** (pre-existing): Rust's route also asks `platform:iam:role:create`, `:update` and
  `:delete`; Go's handler asks anchor only.

## Client-visible changes

- `POST /bff/roles/sync-platform`: a caller without `platform:*:*:*` gets 403
  `PERMISSION_ABOVE_CALLER` when the sync would add or remove a platform permission it does not
  hold (a fresh database, a built-in role trimmed or edited by hand, a stale `CODE` role to sweep,
  a catalogue change after an upgrade); a super-admin runs it as before.
- Boot sync: unchanged.
- No OpenAPI change (the route documents only its 200), no SQL change.

## Verified

All database tests ran on the `FC_TEST_PG_BIN` harness (embedded PostgreSQL 18.3, a private
cluster); no dev database and nothing on 8080 was touched.

- Failing first and mutants: as recorded above.
- `cargo fmt --all -- --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`:
  clean; `cargo clippy -p fc-dev --features web --all-targets -- -D warnings`: clean.
- Lib tests: fc-platform 99, fc-platform-auth 131, fc-platform-core 134, fc-platform-iam 452;
  fc-dev's binary tests 57; all passed.
- The whole `it` suite with `--include-ignored`: **512 passed, 1 failed**:
  `dispatch_claim_plan_test::plans_never_analysed_200k`, a timing bound (`backlog_count` 269 ms
  against 250 ms) under the full suite's load; it passed when run alone. Unrelated to this change.
- No SQL changed (`sqlx-prepare --check` not needed); no OpenAPI change.
- Builds ran with `CARGO_INCREMENTAL=0`; none stalled.

## Not verified

- No browser run: how the SPA's sync button shows the refusal was not checked.
- fc-server and fc-dev were built and linted, not started: the boot sync's `Caller::system()` is
  exercised through `RoleSyncService` in the tests, not through a binary's startup.
- The `warn` log lines (skipped non-`CODE` role, kept assigned stale role) were not observed.

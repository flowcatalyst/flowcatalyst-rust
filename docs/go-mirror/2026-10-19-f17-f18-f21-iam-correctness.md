# Go F17, F18 and F21 mirrored: service-account ids, role names, confined role reads (P2 batch A, 2026-10-19)

Go's F17 (`c74650d5`), F18 (`52c51e68`) and F21 (`bf81422f`), `flowcatalyst-go/docs/platform-bug-fixes.md`.
Nothing is pushed. The Go repo was only read. No migration: the data fixes were already mirrored
(Go 075/076/077 = Rust 087/088/089).

| Item | Go | Rust |
|---|---|---|
| F17: connection sync stores the `sac_` id; `serviceAccountId` on `PUT /api/connections/{id}` | `c74650d5` | `d704e283` |
| F18: shape and existence checks at every role write, `application_id` on role create and on the platform catalogue | `52c51e68` | `7ba569a6` |
| F21: the nine `/api/roles` reads confined for application service accounts; the `AppScope.ConfinedCodes` helper | `bf81422f` | `6d657e96` |

Tests: `connection_sync_service_account_test` (4), `role_name_validation_test` (6),
`role_read_confinement_test` (5) in `crates/fc-platform/tests/it/`, the unit tests of `role::name` and
`role_read_gates`; `FC_TEST_DATABASE_URL=<postgres 18> cargo test -p fc-platform --test it --
--include-ignored connection_sync_service_account_test:: role_name_validation_test::
role_read_confinement_test::`. Mutation-checked: each commit message lists what was broken.

## What changed

**F17.** `SyncConnectionsUseCase` resolves the application's principal id (`app_applications.service_account_id`,
a `prn_` id, FK to `iam_principals`) to the service account behind it (`iam_principals.service_account_id`) and
stores that. A principal with none fails the whole sync, 400 `APPLICATION_SERVICE_ACCOUNT_REQUIRED`, nothing
written. Every sync re-points existing API/CODE rows. `PUT /api/connections/{id}` takes an optional
`serviceAccountId` (set-if-provided; `SERVICE_ACCOUNT_REQUIRED` when blank, `SERVICE_ACCOUNT_NOT_FOUND` when not an
existing account). Create still does not validate the id (Go's `TODO(wave-3c)`).

**F18.** `role::name` (`fc-platform-iam/src/role/name.rs`): `validate_name` (two or more colon-separated segments
of ASCII letters, digits, `_`, `.`, `-`), `validate_permission` (one or more, `*` allowed), `resolve_names`
(shape then lookup). Applied at: role create, role update (permissions), grant-permission, the SDK role sync (up
front, whole sync refused), the service-account assignment (shape in validate, existence in execute), the user
assignment (shape in validate; existence was already checked), the IdP role mapping create. The principal syncs
skip a malformed or unknown name: the application route reports it in `skippedRoles` (as before for unknown
names), the anchor's platform route skips it and records it on the rollup event (Go's `SyncUsersResponse` has
no `skippedRoles`). Role create writes the owning application's `application_id` and refuses an unknown
application (400 `APPLICATION_NOT_FOUND`). The boot seed, the platform role sync and the SDK role sync set or
heal `application_id` (the SDK sync adopts an id-less role of its application). `AuthRole::is_platform()` (the
application code is `platform`) replaces "no application id" in the two client-administrator checks and the
bulk import, so the backfilled id does not open platform roles to a client administrator.

**F21.** `/api/roles` reads use `can_read_roles_confined` / `can_read_permission_catalog`; a caller admitted only
by the application-service permission is confined by `fc_platform_iam::shared::app_scope::confined_applications`
(the helper Go's F20 introduces as `AppScope.ConfinedCodes`: `None` for an all-applications caller, the bound
codes otherwise, 403 `FORBIDDEN` for a caller bound to none). `can_read_roles` now admits only the console
permission (it used to admit the application-service one too, so the BFF, the server-rendered UI and the
`/api/roles` reads showed an application service account every application's roles); the application-scoped SDK
list keeps the wider gate.

## Differences from Go that remain

- No `role.Name` type: `RoleAssignment.role` is still a `String`. The write boundary is the `role::name` functions
  at each path; a new write path that forgets them is not caught by the compiler.
- The IdP login sync (`OidcSyncService`) only shape-checks a mapped platform role (skip and warn). Go's
  `SyncIdpRoles` use case also requires the role to exist (400 `ROLE_NOT_FOUND`); Rust's login path has no such
  use case, and failing a login on a stale mapping would be a behaviour change this batch has no ruling for.
- Order of checks on `PUT /api/service-accounts/{id}/roles`: the shape check runs in `validate`, before the
  account is loaded, so a malformed name for a missing account is `INVALID_ROLE_NAME` here, `ServiceAccount_NOT_FOUND`
  in Go. Existence (`ROLE_NOT_FOUND`) is in `execute`, after the ceiling, as Go.
- OpenAPI: `UpdateConnectionRequest.serviceAccountId` is added (the Rust and Go schemas now have the same members);
  the ratchet baseline does not track property sets, so it is unchanged. No other schema changed.
- Not verified: that a connection the sync creates is now delivered signed end to end (the delivery harness, G4); the tests
  assert the stored id the signer looks up, not the signature.

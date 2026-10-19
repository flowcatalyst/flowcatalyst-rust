# Go F20 mirrored: application users, invitations and scoped reads (P2 batch C, 2026-10-19)

Go's F20 (`flowcatalyst-go/docs/platform-bug-fixes.md` F20, commit `8376f6e1`, framework M4e): an
application's own admin pages call the platform as the application's service account. Nothing
is pushed. The Go repo was only read.

| Item | Go | Rust |
|---|---|---|
| 1. The six `application-service` permissions: the grant to an existing deployment (Go 080 = Rust 092) | `8376f6e1` | `d6c9c45d` |
| 1b. The same six in the built-in role catalogue (`application_service::ALL`) | `8376f6e1` | `01ed215c` (batch B) |
| 2. Permission checks, confined event and dispatch-job reads | `8376f6e1` | `a98c17fa` |
| 3. `GET/POST /api/applications/{appCode}/users`, `GET .../users/{id}`, `GET .../invitations`, `POST .../invitations/{id}/resend`, `DELETE .../invitations/{id}`; ratchet baseline and route snapshot | `8376f6e1` | `9463c887` |

Tests: `crates/fc-platform/tests/it/app_admin_reads_test.rs` (6 tests) and `application_users_test.rs`
(13 tests), `FC_TEST_DATABASE_URL=<postgres 18> cargo test -p fc-platform --test it --
--include-ignored app_admin_reads_test:: application_users_test::`.

## What Go does (read, then ported)

**The confinement rule.** A caller admitted only by an application-service permission is confined
to the applications in its `applications` claim; a caller holding the console permission for the
same read (`platform:messaging:event:view`, `:dispatch-job:view`) is unchanged; a caller that
reaches every application is not narrowed. An application-scoped caller bound to none is
`403 FORBIDDEN` ("this credential is not bound to an application"), not an empty page. Client reach
stays on top. Ownership: an event is the application's when the first segment of its type is the
application's code (`msg_events_read.application`); a dispatch job when the first segment of its
`code` is (`msg_dispatch_jobs_read.application`, and `ownedBy` on by-id, attempts and by-event). By
id, a record outside the application answers the same 404 a missing id would.

**Users.** The application's users are CLIENT-scope users homed at a client the application is
enabled for (`app_client_configs.enabled`) who hold at least one `{app}:...` role. Invitations are
the active internal users of that set with no password and no external identity: `PENDING` while an
unexpired invite token exists, `EXPIRED` otherwise. Invite creates a CLIENT user with
`ADMIN_ASSIGNED` roles (the application's own only: 403 `ROLE_APP_FORBIDDEN`, unknown 400
`UNKNOWN_ROLE`, none 400 `ROLES_REQUIRED`; the F8 ceiling applies) and mints the invite after the
commit (`sendInvitation` default true, `returnInviteLink` suppresses the email, `inviteRedirectUri`
any absolute http(s) URL, validated before anything is written). Resend replaces the earlier
token; nothing to deliver is 400 `INVITATION_DELIVERY_REQUIRED`, a failed delivery 500
`INVITE_DELIVERY_FAILED`. Revoke deactivates the user, then deletes the invite tokens.

## What Rust does

- Checks (`fc-platform-core`, `shared/authorization_service.rs`, `checks`):
  `can_read_events_or_own`, `can_read_all_events`, `can_read_dispatch_jobs_or_own`,
  `can_read_all_dispatch_jobs`, `can_read_application_users`, `can_create_application_users`
  (Go `CanInviteApplicationUsers`), `can_read_application_invitations`,
  `can_update_application_invitations` (Go `CanManageApplicationInvitations`; the names carry a verb
  `permission_convention_test` recognises). They are Go's `requireAny` lists, so a denial is 403
  `PERMISSION_REQUIRED`, `one of: ...`. The console checks (`can_read_events`,
  `can_read_dispatch_jobs`) are unchanged and still guard the BFF screens, the server-rendered UI,
  `filter-options` and the raw reads: the application-service permissions open exactly Go's six
  routes (plus Rust's `/by-event/{eventId}` spelling of one of them).
- Confinement: `RequestAppScope::confined_codes` (`fc-platform-messaging/src/shared/app_scope.rs`)
  is Go's `AppScope.ConfinedCodes`, built on the application scope Rust already resolves
  (`ApplicationAccess::scope_for`). The codes reach SQL as `application = ANY($codes)`
  (`EventReadFilter.accessible_applications`, `DispatchJobReadFilter.accessible_applications`), so
  no caller filter can widen them. By id, `get_event`, `get_dispatch_job` and
  `get_dispatch_job_attempts` answer the same not-found as a missing id; `get_jobs_for_event` drops
  other applications' jobs.
- Users and invitations: `fc-platform-iam/src/appusers/` (`repository.rs` is the one visibility
  predicate shared by list, get, invitations, resend and revoke; `api.rs` the six handlers) and
  `principal/operations/invite_application_user.rs` (the use case). `fc-platform/src/appusers/`
  wires them under `/api/applications`. Invite, resend and revoke reuse the existing
  `PasswordResetEmailer` (`send_invite`, `invite_link`: minting replaces earlier tokens) and
  `DeactivateUserUseCase`.

## Differences from Go

1. **Out-of-scope `{appCode}`: 404, not 403.** Go answers another application's code, an unbound
   credential and (for a scoped caller) an unknown code with 403 `Not authorised for application
   '<code>'`. This platform keeps the owner's standing ruling for every `/{appCode}` route
   (`checks::require_application_access`: "the same 404 as a missing application (owner ruling; Go
   answers 403)"), so the six new routes answer 404 in all four cases (a caller reaching every
   application also gets the 404 for an unknown code, as in Go). Nothing is revealed either way.
   **Owner decision wanted** if the new routes should follow Go instead: one call site per handler
   (`require_application_access` in `appusers/api.rs`) plus the use case's `authorize`.
2. **The invite use case authorizes for itself.** Go's `InviteApplicationUser` has a public
   `Authorize` and trusts the route. Rust's `use_case_shape_convention_test` forbids an empty
   `authorize`, so the use case repeats the permission, the application scope and the client reach
   (the same answers the handler gives first). Observable behaviour is unchanged.
3. **Not-found bodies are Rust's.** A missing user or invitation is 404 `NOT_FOUND`, "Entity not
   found: User with id ..." (`PlatformError::not_found`), where Go says `User_NOT_FOUND` /
   `Invitation_NOT_FOUND`. Same status, same body for an id outside the application and for a
   missing one (tested).
4. **An id bound to an application that no longer exists** cannot arise here (the grant table has a
   foreign key), so Go's "dangling id" refusal has no Rust test; `confined_codes` still refuses a
   caller whose bound ids resolve to nothing.
5. **Resend body.** Go's spec marks the body required though every field is optional; Rust accepts
   an empty body as the defaults and still documents `ResendInvitationRequest`.
6. **Read projection rows.** Rust reads `msg_dispatch_jobs_read.retry_strategy`, `sequence` and
   `timeout_seconds` as required, where Go tolerates NULL; rows the projector writes always have
   them. (Found while porting the fixtures; not changed.)

## Wire shape

`OPENAPI_UPDATE_BASELINE=1 cargo test -p fc-platform --test it -- openapi_go_contract_test` closed
exactly 14 baseline lines: the 6 Go-only operations and the 8 missing schemas
(`ApplicationUser`, `ApplicationUserList`, `ApplicationInvitation`, `ApplicationInvitationList`,
`InviteApplicationUserRequest`, `InviteApplicationUserResponse`, `ResendInvitationRequest`,
`ResendInvitationResponse`). A script comparison against Go's lock found the same operation ids,
tag (`application-users`), parameters (name, location, required, type, format), success statuses,
request-body presence, and each schema's property set and required set. Not reproduced, not
compared by the contract test and without SDK impact: Go's `x-sdk: app` classification and the
`$schema` members. `route_table.snapshot` gained the six routes.

## Verified and inferred

Verified, on PostgreSQL 18, through the HTTP stack (and, for the use case, directly): every
scenario of Go's `appusers/api_pg_test.go`, `event/api/app_read_pg_test.go` and
`dispatchjob/api/app_read_pg_test.go`, and the end-to-end shape of
`server/app_admin_pages_pg_test.go` (events, jobs, invite with returned link, list, get,
invitations, resend, revoke, an empty `roles`, another application refused), except the Go cases
that need the 403 shape (difference 1) or a dangling application id (difference 4).

Mutation checks (each mutant failed at least one test; the named test is the first that did):
event list SQL clause dropped (`..._reads_only_its_own_applications_events`); event by-id
ownership off (`an_event_by_id_is_the_applications_or_not_found`); job `owned_by` always true
(`a_job_by_id_attempts_and_event_is_the_applications_or_not_found`); `confined_codes` never
confining (five tests); unbound caller let through (`an_unbound_credential_is_refused_...`); job
list SQL clause dropped; console view treated as confined; users: role predicate, enabled-client
predicate, CLIENT-scope predicate and caller client reach each dropped; invitations without the
invitee predicate; revoke without deactivation (`resend_replaces_the_link_and_revoke_deactivates`);
foreign roles accepted on invite; invite admitted by `user:view`; the invite use case's
`authorize` emptied (`the_invite_use_case_checks_permission_application_and_client_reach_itself`).

Inferred, not checked: that no deployed caller sends a request body Rust's stricter `roles`
(required, no default) rejects but Go's lenient `additionalProperties: true` body accepted; that
the F20 SPA pages (P5) need nothing beyond these shapes.

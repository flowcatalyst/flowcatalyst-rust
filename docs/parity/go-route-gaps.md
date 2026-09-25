# Go routes Rust lacked: what was ported, what remains

2026-09-25. Rust must be a drop-in replacement for the Go platform
(`../flowcatalyst-go`, read-only; `docs/owner-decisions-2026-09-25.md`). This
records the inventory of Go's HTTP surface against Rust's, what branch
`feat/go-routes` ported, and what is still open.

## Method

Every Go route under `/api`, `/bff`, `/auth`, `/oauth`, `/.well-known` and
`/portal` (plus the spec routes) was listed from the source: the
`apiroute.{Get,Post,Put,Delete}` and `huma.Register` calls in
`internal/platform/*/api`, the chi routes in `shared/{bff,sdk,me}`,
`auth/*`, `passwordreset`, `portalauth`, and the dynamically registered ones
(`registerBFF` for `/bff/events` and `/bff/dispatch-jobs`, `registerAt` for
`/api/processes` and `/bff/processes`, the `regenerate-*` loop in
`serviceaccount/api/api.go:73-81`). The router's own routes (`internal/router`),
the outbox processor's `/outbox/*`, `/metrics`, `/health`, `/ready` and `/mcp`
are out of scope. Each Go route was matched on METHOD + path (parameter names
normalised) against Rust's router (`crates/fc-platform/src/router.rs`, the
per-module routers, `function/*`), and every "missing" verdict was re-checked
by grepping the Rust source.

At `2561978b` (before this work): **385** Go routes in scope, **294** matched
exactly, **22** existed at another path or method, **69** had no Rust
equivalent. The parity harness's run 1 (`docs/parity/api-run-1.md`) found the
same set from the other side.

## Ported

"Go" is the Go file and line of the route's registration or handler.

### Two-factor authentication (`crates/fc-platform/src/mfa/`)

| Route | Go |
|---|---|
| `/auth/login` answers `mfa_required` / `enrollment_required` | `auth/login/endpoint.go:533`, `twofactor.go:77` |
| POST `/auth/2fa/verify` | `auth/login/twofactor.go:62` |
| POST `/auth/2fa/challenge/email` | `twofactor.go:63` |
| POST `/auth/2fa/enroll/totp/begin`, `/confirm` | `twofactor.go:64-65` |
| POST `/auth/2fa/enroll/email/begin`, `/confirm` | `twofactor.go:66-67` |
| GET `/auth/2fa/status` | `twofactor_selfservice.go:25` |
| POST `/auth/2fa/methods/totp/begin`, `/confirm` | `twofactor_selfservice.go:26-27` |
| POST `/auth/2fa/methods/email/begin`, `/confirm` | `twofactor_selfservice.go:28-29` |
| DELETE `/auth/2fa/methods/{method}` | `twofactor_selfservice.go:30` |
| POST `/auth/2fa/recovery-codes/regenerate` | `twofactor_selfservice.go:31` |
| GET `/auth/2fa/trusted-devices`, DELETE `/{id}` | `twofactor_selfservice.go:32-33` |
| POST `/api/principals/{id}/reset-2fa` | `principal/api/api.go:102` |
| Email-domain mapping 2FA policy (`require2fa`, `allowed2faMethods`, `rememberDeviceEnabled`, `rememberDeviceDays`) | `emaildomainmapping/api/dto.go:20-25` |

How the second factor meets each sign-in, as Go:

- **Password** (`/auth/login`): a user with a confirmed factor gets
  `mfa_required` (a 10-minute HS256 step token, the usable methods, narrowed to
  the domain's allow-list when it requires 2FA) unless a remembered-device
  cookie (`__Host-fc_td`, `fc_td` over plain HTTP) matches; a user of a domain
  requiring 2FA with no factor gets `enrollment_required` (a 30-minute token).
  A passkey does not exempt the password path. Evaluation errors fail closed
  (`MFA_EVAL_FAILED`).
- **Passkey** (`/auth/webauthn/authenticate/*`) and **OIDC**
  (`/auth/oidc/callback`): never challenged (Go skips 2FA on both).
- **Password reset**: a user with TOTP gets a factor-gated token
  (`requires_factor`) and must present a current code at confirm; a
  `reset_2fa` token clears the factors; an unenrolled user of a 2FA-requiring
  domain is handed `enrollment_required` after the reset.
- **Session-only routes**: Go gates the self-service `/auth/2fa/*`,
  `/auth/change-password*` and `/auth/login-history` on "authenticated", not on
  the cookie; a bearer token of an API principal passes. Rust does the same.
  The step-token routes are public (the token stands in for a session).

### Password and account flows

| Route | Go |
|---|---|
| POST `/auth/password-setup/request` | `passwordreset/api/api.go:447` |
| POST `/auth/check-domain` answers `passwordSetupRequired` | `auth/login/endpoint.go:230` |
| `/auth/password-reset/request`: eligibility, `redirectUri`, factor-gated tokens | `passwordreset/api/api.go:470-560` |
| `/auth/password-reset/validate`: `requiresFactor` | `api.go:733` |
| `/auth/password-reset/confirm`: factor gate, `reset_2fa`, enrolment hand-off, invite sign-in | `api.go:753-1000` |
| POST `/api/principals/{id}/send-password-reset` takes `{reset2fa}` | `principal/api/api.go:1442` |
| POST `/auth/change-password`, `/auth/change-password/send-email-code` | `auth/login/endpoint.go:143-144`, `change_password.go` |
| GET `/auth/login-history` | `endpoint.go:146`, `session_history.go` |
| `/auth/login` refuses an OIDC-mapped domain (`SSO_REQUIRED`) | `endpoint.go:481-498` |
| GET `/api/reset-approvals`, POST `/{id}/approve`, `/{id}/deny` | `resetapproval/api/api.go:42-44` |
| POST `/api/principals/users` and `/api/principals`: `sendInvitation`, `returnInviteLink` (answered as `inviteLink`), `inviteRedirectUri`, the "account created" welcome | `principal/api/api.go:365-391, 620-760` (`notifyNewUser`) |

### Developer credentials (`crates/fc-platform/src/developer_credential/`)

| Route | Go |
|---|---|
| GET `/api/principals/developer-users` | `principal/api/api.go:116` |
| POST, DELETE `/api/principals/{id}/developer-credential` | `api.go:117-118`, `operations/developer_credential.go` |
| `/oauth/token` `client_credentials` with a `prn_` client id | `auth/oauthapi/token.go:505-520, 595-632` |

### Portal identity plane (`crates/fc-platform/src/portal/`)

| Route | Go |
|---|---|
| GET, POST `/api/portal-users`; DELETE `/{id}`; POST `/{id}/activate`, `/deactivate`, `/apps`; DELETE `/{id}/apps/{portalAppCode}` | `portalidentity/api/api.go:74-80` |
| GET, POST `/api/portal-apps`; PUT, DELETE `/{id}`; POST `/{id}/assign-unassigned` | `api.go:83-87` |
| GET `/portal/authorize`; POST `/portal/auth/check-domain`, `/login`, `/password-reset` | `portalauth/endpoints.go:57-60` |
| GET `/portal/auth/oidc/login` and the portal branch of `/auth/oidc/callback` | `auth/bridge/login_endpoint.go:970, 1049` |
| Portal reset/invite tokens (`ptu_` subjects) at `/auth/password-reset/validate`, `/confirm`; portal code redemption at `/oauth/token`; `portalClientId` / `portalAppId` on OAuth clients | `passwordreset/api/api.go`, `oauthapi/portal_token.go` |

### Sync, router and the remaining aliases (`feat/go-routes-misc`)

| Route | Go |
|---|---|
| GET `/api/dispatch/router-config` | `dispatch/api.go:31`, `document.go`, `settings.go` |
| POST `/api/applications/{appCode}/connections/sync` | `sdksync/api.go:100` |
| POST `/api/processes/sync` (`applicationCode` in the body) | `sdksync/api.go:108` |
| POST `/api/applications/{appCode}/docs/sync`; GET `/api/docs`, `/api/docs/platform/{slug}`, `/api/docs/applications/{appCode}/{slug}` | `sdksync/docs_sync.go`, `docsapi/api.go:47-49` |
| GET, POST `/api/roles/{roleName}/permissions`; POST, DELETE `/{roleName}/permissions/{permission}`; DELETE `/api/roles/permissions/{permission}`; POST `/bff/roles/permissions` | `role/api/api.go:46-56`, `shared/bff/roles.go:51` |
| POST `/api/service-accounts/{id}/token`, `/deactivate`; `regenerate-token`, `regenerate-secret` aliases | `serviceaccount/api/api.go:66, 71-84` |
| POST `/api/clients/search` | `client/api/api.go:39` |
| GET, PUT, DELETE `/api/config/{app}/{section}/{property}`; GET `/api/platform-config/{app}`; GET, POST `/{app}/access`; DELETE `/api/platform-config/access/{id}` | `platformconfig/api/api.go:32-40` |
| GET `/api/email-domain-mappings/lookup?domain=`, `/by-domain/{domain}`; POST `/{id}/move-provider`; Go's create request | `emaildomainmapping/api/api.go:44-49` |
| POST `/api/principals/bulk-import`; GET `/{id}/version`; PUT `/{id}/client-association` | `principal/api/api.go:93, 96, 115` |
| POST `/api/applications/{id}/service-account`; GET `/{id}/clients/{clientId}` | `application/api/api.go:53, 60` |
| POST `/api/event-types/{id}/schemas`; PUT `/bff/event-types/{id}`; GET `/api|bff/events/list-raw`, `/api|bff/dispatch-jobs/list-raw`, `…/event/{eventId}` | `eventtype/api/api.go:44`, `bff/event_types.go:43`, `event/api/api.go:37, 68`, `dispatchjob/api/api.go:69-105` |
| GET `/api/openapi.json`, `/api/openapi.yaml` | `internal/server/wire_spec.go:16, 24` |
| POST `/api|bff/dispatch-jobs/requeue`, `/{id}/cancel`, `/{id}/complete`, `/{id}/sign` | `dispatchjob/api/api.go:75-78, 109-112` |

`/api/dispatch/settled` arrived with the scheduler work on `feat/functions`.

## Migrations

| Rust | Go | What |
|---|---|---|
| `041_mfa_tables` | 031 | `iam_user_mfa_methods`, `iam_user_mfa_recovery_codes`, `iam_mfa_email_pins`, `iam_mfa_trusted_devices`, the mapping's `require_2fa` / `remember_device_*` and `tnt_email_domain_mapping_2fa_methods` |
| `042_password_reset_token_purpose` | 031, 032, 033, 041, 051 | `iam_password_reset_tokens.{purpose, reset_2fa, requires_factor, factor_attempts, redirect_uri}` and the purpose CHECK |
| `052_developer_api_credentials` | 039 | `iam_principals.dev_client_secret_ref`, `dev_client_secret_updated_at` |
| `044_reset_approval_requests` | 032 | `iam_reset_approval_requests` |
| `046_portal_identities` | 041, 043 | `portal_identities`, `portal_login_flows`, portal flags on OAuth clients and OIDC login states |
| `047_portal_apps` | 053 | `portal_apps`, `portal_identity_apps`, `oauth_clients.portal_app_id`, invite columns |
| `050_connection_application_scope` | 056 | `msg_connections.application_code`, `source`; uniqueness on (application_code, client_id, code) for connections and subscriptions |
| `051_app_docs` | 044 | `app_docs` |

All are idempotent (`IF NOT EXISTS`, guarded CHECKs) and use Go's table and
column names, so each is a no-op on production's Go-migrated database; each is
registered in `core_migrations` with a probe for the pre-tracker backfill.

## Deliberate differences from Go

| Where | Rust | Go | Why |
|---|---|---|---|
| TOTP, email PIN, recovery code | spent by one guarded UPDATE/DELETE: a code signs in once however many requests race | `TouchMethodUsed` is an unconditional UPDATE after a separate read; two concurrent verifies of one code both pass | replay protection (task brief) |
| `/auth/2fa/verify` | enforces the domain's allowed methods | enrolment only | owner ruling I-Q12 (Java, 2026-09-05) |
| `/auth/2fa/challenge/email`, `/auth/password-setup/request` | budgeted per IP and per address, own buckets, the reset numbers; over budget: nothing sent, same answer | no budget | owner ruling 7; Java dbe3ad9c |
| Trusted-device notification | the User-Agent label is HTML-escaped | interpolated raw | an attacker-chosen header in an email body |
| `/auth/change-password` policy | Rust's password policy (`PASSWORD_POLICY`) | Go's NIST-style `passwordpolicy` (`PASSWORD_TOO_SHORT`, …) | the policy is platform-wide; porting Go's is a separate change (the portal port carries Go's policy for portal identities only) |
| Emails | plain HTML | Go's branded theme (logo, colours) for reset and invite | Rust has no branding store yet |
| Reset request of a user with no strong factor under the strict policy | always a link | a link (Go's `RequireStrongFactorForReset` is never set) | same behaviour; the approval queue is only filled by Go-written rows |
| Not-found codes, error envelope | Rust's (`NOT_FOUND`, `error`+`code`+`message`) | `Entity_NOT_FOUND`, huma's `{error, message, details}` | platform-wide, owner decision 5 |
| Router config | every tenant (platform, pools, active subscriptions, every client identifier) gets DEFAULT and HIGH_PRIORITY queues | pool and subscription tenants only, HIGH_PRIORITY only when used | Go never lists a client-scoped job's queue (the API never sets `client_identifier`), so such jobs sit QUEUED forever (delivery harness); the naming is the scheduler's own (`scheduler::destination`) |
| Catalogue writes, docs sync, admin token mint | through the unit of work, emitting `platform:admin:permission:defined|deleted`, `platform:admin:app-docs:synced`, `platform:iam:serviceaccount:token-minted` | direct writes, no event | CLAUDE.md: every control-plane write has an event and audit row |
| Connections sync | the rollup `connection:synced` event only | per-row events too | |
| Role, service-account, client-association, attach-service-account writes | anchor plus the permission; role ceiling on role grants and bulk-import roles | the permission (anchor for some) | owner decisions #19, #25, ruling 14, triage S3 |
| Docs, sync and config routes of a registered application | the application must be in the caller's scope (404) | not checked | an out-of-scope application answers 404 (owner ruling) |
| Config | secrets stay encrypted at rest (an anchor read decrypts); an absent `description` on PUT keeps the old one | Go clears it | |
| Raw-list aliases | need `view` as well as `view-raw` | `view-raw` | |

## Remaining

Routes: all 385 of Go's are served (`POST /api/principals` arrived with the
`feat/api-core` merge). Behaviour still differing or unbuilt:

- The principal response lacks Go's `hasDeveloperCredential`,
  `developerCredentialUpdatedAt` and (detail read) `twoFactorMethods`; the
  developer-users list carries the first two.
- Go's NIST password policy (`passwordpolicy`) for users, and Go's branded
  email theme.
- `GET /auth/oidc/login`'s error shapes (`DOMAIN_REQUIRED`,
  `OIDC_NOT_CONFIGURED`).
- Expired email PINs, trusted devices, reset tokens and portal login flows are
  never purged (the repositories have `purge_expired`; nothing schedules it —
  Java ruling I-Q17).
- Requeue, cancel and complete are gated on `dispatch-job:view`, as Go (a view
  permission for a write): worth a ruling.
- `fc-router`'s config sync against `/api/dispatch/router-config`: it maps an
  explicit `connections: 0` / `visibilityTimeout: 0` to 0 instead of its
  defaults, sends no bearer token (the route needs anchor plus
  `dispatch-pool:view`; the SDK-rulings router bearer may cover it), and
  `FLOWCATALYST_CONFIG_URL` must point at the route.
- CLAUDE.md lists no infrastructure exception for the 2FA rows (factors,
  codes, PINs, devices, approval decisions) or the self-service password
  change, which Go writes directly with audit rows and no events and which
  this port writes the same way. The owner should add them, as #16 did for
  the lazy secret rehash.

## Parity harness

The Go-vs-Rust harness (`harness/parity`), Rust at a debug build of this branch,
Go at `73a6918` (prebuilt), `--only
'{auth,auth-remainder,reset-approvals,portal,portal-apps,portal-users,portal-assign}/*'`:
`auth/mfa.json` — 21 OK; its remaining 11 DIFFs were cookie attributes (fixed
since: `Expires`, and `Secure`/`SameSite` on the clearing cookie), `$schema`
and `/auth/me`'s shape. `auth/session.json` found the login failure envelope
(fixed since: 401 `UNAUTHENTICATED` with the cookie challenge, 429 on
backoff). What is left in these groups is platform-wide, not specific to these
routes:

- `$schema` on every body, and Go's `{error, message, details}` error envelope
  vs Rust's `{error, code, message}` (owner decision 5 for codes; the envelope
  is `feat/api-core`'s).
- Scenarios that create a principal with `POST /api/principals` (405 on Rust
  here; `feat/api-core` adds it) cascade into undefined captures.
- `/auth/me`'s fields and the roleless profile-only 403 (`NO_PLATFORM_ROLE`)
  belong to the agents on `/auth/me` and `shared/middleware.rs`.
- The client IP for the login backoff: fixed since by `feat/api-core`
  (`ClientIp` falls back to the socket peer, as Go).
- `GET /auth/oidc/login` without `domain` answers axum's plain-text query error
  (Go: JSON `DOMAIN_REQUIRED`), and an internal domain answers a plain message
  (Go: `OIDC_NOT_CONFIGURED`).
- OAuth client responses lack Go's `apiAccess` and `applications`.

A full run after the `feat/go-routes-misc` merge was not possible: the only
prebuilt Go `fcdev` (needed to seed) had been removed, and Go may not be built
here. The Rust side of every ported route is covered by the Docker tests
(`two_factor_test`, `two_factor_admin_test`, `password_flows_test`,
`developer_credential_test`, `portal_identity_test`, `go_routes_test`).

## SPA

Go's SPA has the 2FA challenge and enrolment (`components/TwoFactorChallenge.vue`,
`TwoFactorSetup.vue`), the Profile page's 2FA section, change password and
sign-in history (`TwoFactorSection.vue`, `pages/ProfilePage.vue`), the
set-password framing and the "create your password" offer
(`pages/auth/LoginPage.vue`, `ResetPasswordPage.vue`), the lost-device approval
page (`pages/authentication/ResetApprovalsPage.vue`), the developer-credential
controls (`pages/users/UserDetailBody.vue`), the portal-user pages and the
mapping's 2FA fields. This branch ports the parts a production user needs
on day one into the Rust SPA (`frontend/`): the sign-in challenge and
enrolment (`components/TwoFactorChallenge.vue`, `TwoFactorSetup.vue`), the
set-password framing, factor code and enrolment hand-off on
`ResetPasswordPage.vue`, the "create your password" offer on the login page,
the Profile page's 2FA section, change password and sign-in activity, and the
mapping's 2FA fields. Not ported: the reset-approvals page, the
developer-credential controls, the portal-user pages, and the portal framing of
the set-password page. The SPA's `switchClient` still posts to
`/auth/client/{clientId}` where the route is `POST /auth/client/switch` (a
pre-existing SPA bug, not touched here).

## Appendix: every Go route in scope

Status: **had** — Rust served it at `2561978b`; **ported** — added on this branch (or its alias added); the one `feat/api-core` route is marked. Go file paths are under `internal/`.

| Status | Method | Path | Go |
|---|---|---|---|
| had | GET | `/.well-known/jwks.json` | `platform/auth/oauthapi/discovery.go:12` |
| had | GET | `/.well-known/openid-configuration` | `platform/auth/oauthapi/discovery.go:11` |
| had | GET | `/api/anchor-domains` | `platform/auth/api/api.go:138` |
| had | POST | `/api/anchor-domains` | `platform/auth/api/api.go:139` |
| had | DELETE | `/api/anchor-domains/{id}` | `platform/auth/api/api.go:141` |
| had | PUT | `/api/anchor-domains/{id}` | `platform/auth/api/api.go:140` |
| had | GET | `/api/applications` | `platform/application/api/api.go:45` |
| had | POST | `/api/applications` | `platform/application/api/api.go:46` |
| had | GET | `/api/applications/by-code/{code}` | `platform/application/api/api.go:47` |
| had | GET | `/api/applications/by-id/{id}/roles` | `platform/application/api/api.go:59` |
| ported | POST | `/api/applications/{appCode}/connections/sync` | `platform/sdksync/api.go:100` |
| had | POST | `/api/applications/{appCode}/dispatch-pools/sync` | `platform/sdksync/api.go:101` |
| ported | POST | `/api/applications/{appCode}/docs/sync` | `platform/sdksync/api.go:103` |
| had | POST | `/api/applications/{appCode}/event-types/sync` | `platform/sdksync/api.go:98` |
| had | POST | `/api/applications/{appCode}/openapi/sync` | `platform/sdksync/api.go:110` |
| had | POST | `/api/applications/{appCode}/principals/sync` | `platform/sdksync/api.go:102` |
| had | POST | `/api/applications/{appCode}/processes/sync` | `platform/sdksync/api.go:104` |
| had | POST | `/api/applications/{appCode}/roles/sync` | `platform/sdksync/api.go:97` |
| had | POST | `/api/applications/{appCode}/scheduled-jobs/sync` | `platform/sdksync/api.go:109` |
| had | POST | `/api/applications/{appCode}/subscriptions/sync` | `platform/sdksync/api.go:99` |
| had | DELETE | `/api/applications/{id}` | `platform/application/api/api.go:52` |
| had | GET | `/api/applications/{id}` | `platform/application/api/api.go:48` |
| had | PUT | `/api/applications/{id}` | `platform/application/api/api.go:49` |
| had | POST | `/api/applications/{id}/activate` | `platform/application/api/api.go:50` |
| had | GET | `/api/applications/{id}/clients` | `platform/application/api/api.go:54` |
| ported | GET | `/api/applications/{id}/clients/{clientId}` | `platform/application/api/api.go:60` |
| had | POST | `/api/applications/{id}/clients/{clientId}/disable` | `platform/application/api/api.go:56` |
| had | POST | `/api/applications/{id}/clients/{clientId}/enable` | `platform/application/api/api.go:55` |
| had | POST | `/api/applications/{id}/deactivate` | `platform/application/api/api.go:51` |
| had | POST | `/api/applications/{id}/provision-login-client` | `platform/application/api/api.go:58` |
| had | POST | `/api/applications/{id}/provision-service-account` | `platform/application/api/api.go:57` |
| ported | POST | `/api/applications/{id}/service-account` | `platform/application/api/api.go:53` |
| had | GET | `/api/audit-logs` | `platform/audit/api/api.go:31` |
| had | GET | `/api/audit-logs/application-ids` | `platform/audit/api/api.go:35` |
| had | POST | `/api/audit-logs/batch` | `platform/shared/sdk/audit_batch.go:50` |
| had | GET | `/api/audit-logs/client-ids` | `platform/audit/api/api.go:36` |
| had | GET | `/api/audit-logs/entity-types` | `platform/audit/api/api.go:33` |
| had | GET | `/api/audit-logs/entity/{entityType}/{entityId}` | `platform/audit/api/api.go:38` |
| had | GET | `/api/audit-logs/operations` | `platform/audit/api/api.go:34` |
| had | GET | `/api/audit-logs/principal/{principalId}` | `platform/audit/api/api.go:39` |
| had | GET | `/api/audit-logs/recent` | `platform/audit/api/api.go:32` |
| had | GET | `/api/audit-logs/{id}` | `platform/audit/api/api.go:37` |
| had | GET | `/api/auth-configs` | `platform/auth/api/api.go:144` |
| had | POST | `/api/auth-configs` | `platform/auth/api/api.go:145` |
| had | DELETE | `/api/auth-configs/{id}` | `platform/auth/api/api.go:147` |
| had | PUT | `/api/auth-configs/{id}` | `platform/auth/api/api.go:146` |
| had | GET | `/api/clients` | `platform/client/api/api.go:37` |
| had | POST | `/api/clients` | `platform/client/api/api.go:38` |
| had | GET | `/api/clients/by-identifier/{identifier}` | `platform/client/api/api.go:43` |
| had | GET | `/api/clients/search` | `platform/client/api/api.go:42` |
| ported | POST | `/api/clients/search` | `platform/client/api/api.go:39` |
| had | DELETE | `/api/clients/{id}` | `platform/client/api/api.go:49` |
| had | GET | `/api/clients/{id}` | `platform/client/api/api.go:44` |
| had | PUT | `/api/clients/{id}` | `platform/client/api/api.go:45` |
| had | POST | `/api/clients/{id}/activate` | `platform/client/api/api.go:46` |
| had | GET | `/api/clients/{id}/applications` | `platform/client/api/api.go:53` |
| had | PUT | `/api/clients/{id}/applications` | `platform/client/api/api.go:54` |
| had | POST | `/api/clients/{id}/applications/{applicationId}/disable` | `platform/client/api/api.go:56` |
| had | POST | `/api/clients/{id}/applications/{applicationId}/enable` | `platform/client/api/api.go:55` |
| had | POST | `/api/clients/{id}/deactivate` | `platform/client/api/api.go:52` |
| had | POST | `/api/clients/{id}/notes` | `platform/client/api/api.go:48` |
| had | POST | `/api/clients/{id}/suspend` | `platform/client/api/api.go:47` |
| had | GET | `/api/config/platform` | `platform/publicapi/endpoint.go:61` |
| had | DELETE | `/api/config/{app}/{section}/{property}` | `platform/platformconfig/api/api.go:37` |
| had | GET | `/api/config/{app}/{section}/{property}` | `platform/platformconfig/api/api.go:33` |
| had | PUT | `/api/config/{app}/{section}/{property}` | `platform/platformconfig/api/api.go:36` |
| had | GET | `/api/connections` | `platform/connection/api/api.go:34` |
| had | POST | `/api/connections` | `platform/connection/api/api.go:35` |
| had | DELETE | `/api/connections/{id}` | `platform/connection/api/api.go:38` |
| had | GET | `/api/connections/{id}` | `platform/connection/api/api.go:36` |
| had | PUT | `/api/connections/{id}` | `platform/connection/api/api.go:37` |
| had | POST | `/api/connections/{id}/activate` | `platform/connection/api/api.go:40` |
| had | POST | `/api/connections/{id}/pause` | `platform/connection/api/api.go:39` |
| had | GET | `/api/dispatch-jobs` | `platform/dispatchjob/api/api.go:68` |
| had | POST | `/api/dispatch-jobs` | `platform/shared/sdk/dispatch_jobs_batch.go:92` |
| had | POST | `/api/dispatch-jobs/batch` | `platform/shared/sdk/dispatch_jobs_batch.go:93` |
| had | GET | `/api/dispatch-jobs/by-event/{eventId}` | `platform/dispatchjob/api/api.go:84` |
| ported | GET | `/api/dispatch-jobs/event/{eventId}` | `platform/dispatchjob/api/api.go:71` |
| had | GET | `/api/dispatch-jobs/filter-options` | `platform/dispatchjob/api/api.go:70` |
| ported | GET | `/api/dispatch-jobs/list-raw` | `platform/dispatchjob/api/api.go:69` |
| had | GET | `/api/dispatch-jobs/raw` | `platform/dispatchjob/api/api.go:85` |
| ported | POST | `/api/dispatch-jobs/requeue` | `platform/dispatchjob/api/api.go:75` |
| had | GET | `/api/dispatch-jobs/{id}` | `platform/dispatchjob/api/api.go:72` |
| had | GET | `/api/dispatch-jobs/{id}/attempts` | `platform/dispatchjob/api/api.go:74` |
| ported | POST | `/api/dispatch-jobs/{id}/cancel` | `platform/dispatchjob/api/api.go:76` |
| ported | POST | `/api/dispatch-jobs/{id}/complete` | `platform/dispatchjob/api/api.go:77` |
| had | GET | `/api/dispatch-jobs/{id}/raw` | `platform/dispatchjob/api/api.go:73` |
| ported | POST | `/api/dispatch-jobs/{id}/sign` | `platform/dispatchjob/api/api.go:78` |
| had | GET | `/api/dispatch-pools` | `platform/dispatchpool/api/api.go:32` |
| had | POST | `/api/dispatch-pools` | `platform/dispatchpool/api/api.go:33` |
| had | DELETE | `/api/dispatch-pools/{id}` | `platform/dispatchpool/api/api.go:39` |
| had | GET | `/api/dispatch-pools/{id}` | `platform/dispatchpool/api/api.go:34` |
| had | PUT | `/api/dispatch-pools/{id}` | `platform/dispatchpool/api/api.go:35` |
| had | POST | `/api/dispatch-pools/{id}/activate` | `platform/dispatchpool/api/api.go:38` |
| had | POST | `/api/dispatch-pools/{id}/archive` | `platform/dispatchpool/api/api.go:36` |
| had | POST | `/api/dispatch-pools/{id}/suspend` | `platform/dispatchpool/api/api.go:37` |
| had | POST | `/api/dispatch/process` | `platform/dispatchjob/processing/processing.go:155` |
| ported | GET | `/api/dispatch/router-config` | `platform/dispatch/api.go:31` |
| ported | POST | `/api/dispatch/settled` | `platform/dispatchjob/settled/settled.go:75` |
| ported | GET | `/api/docs` | `platform/docsapi/api.go:47` |
| ported | GET | `/api/docs/applications/{appCode}/{slug}` | `platform/docsapi/api.go:49` |
| ported | GET | `/api/docs/platform/{slug}` | `platform/docsapi/api.go:48` |
| had | GET | `/api/email-domain-mappings` | `platform/emaildomainmapping/api/api.go:43` |
| had | POST | `/api/email-domain-mappings` | `platform/emaildomainmapping/api/api.go:44` |
| ported | GET | `/api/email-domain-mappings/by-domain/{domain}` | `platform/emaildomainmapping/api/api.go:46` |
| ported | GET | `/api/email-domain-mappings/lookup` | `platform/emaildomainmapping/api/api.go:45` |
| had | DELETE | `/api/email-domain-mappings/{id}` | `platform/emaildomainmapping/api/api.go:50` |
| had | GET | `/api/email-domain-mappings/{id}` | `platform/emaildomainmapping/api/api.go:47` |
| had | PUT | `/api/email-domain-mappings/{id}` | `platform/emaildomainmapping/api/api.go:48` |
| ported | POST | `/api/email-domain-mappings/{id}/move-provider` | `platform/emaildomainmapping/api/api.go:49` |
| had | GET | `/api/event-types` | `platform/eventtype/api/api.go:37` |
| had | POST | `/api/event-types` | `platform/eventtype/api/api.go:38` |
| had | GET | `/api/event-types/by-code/{code}` | `platform/eventtype/api/api.go:40` |
| had | DELETE | `/api/event-types/{id}` | `platform/eventtype/api/api.go:42` |
| had | GET | `/api/event-types/{id}` | `platform/eventtype/api/api.go:39` |
| had | PUT | `/api/event-types/{id}` | `platform/eventtype/api/api.go:41` |
| ported | POST | `/api/event-types/{id}/schemas` | `platform/eventtype/api/api.go:43` |
| had | POST | `/api/event-types/{id}/versions` | `platform/eventtype/api/api.go:46` |
| had | GET | `/api/events` | `platform/event/api/api.go:42` |
| had | POST | `/api/events` | `platform/event/api/api.go:34` |
| had | POST | `/api/events/batch` | `platform/event/api/api.go:35` |
| had | GET | `/api/events/filter-options` | `platform/event/api/api.go:36` |
| ported | GET | `/api/events/list-raw` | `platform/event/api/api.go:37` |
| had | GET | `/api/events/raw` | `platform/event/api/api.go:41` |
| had | GET | `/api/events/{id}` | `platform/event/api/api.go:43` |
| had | GET | `/api/identity-providers` | `platform/identityprovider/api/api.go:81` |
| had | POST | `/api/identity-providers` | `platform/identityprovider/api/api.go:82` |
| had | DELETE | `/api/identity-providers/{id}` | `platform/identityprovider/api/api.go:85` |
| had | GET | `/api/identity-providers/{id}` | `platform/identityprovider/api/api.go:83` |
| had | PUT | `/api/identity-providers/{id}` | `platform/identityprovider/api/api.go:84` |
| had | GET | `/api/idp-role-mappings` | `platform/auth/api/api.go:150` |
| had | POST | `/api/idp-role-mappings` | `platform/auth/api/api.go:151` |
| had | DELETE | `/api/idp-role-mappings/{id}` | `platform/auth/api/api.go:152` |
| had | GET | `/api/login-attempts` | `platform/loginattempt/api/api.go:31` |
| had | GET | `/api/me` | `platform/shared/me/me.go:33` |
| had | GET | `/api/me/applications` | `platform/shared/me/me.go:34` |
| had | GET | `/api/me/clients` | `platform/shared/me/me.go:35` |
| had | GET | `/api/me/clients/{clientId}` | `platform/shared/me/me.go:36` |
| had | GET | `/api/me/clients/{clientId}/applications` | `platform/shared/me/me.go:37` |
| had | GET | `/api/oauth-clients` | `platform/auth/api/api.go:122` |
| had | POST | `/api/oauth-clients` | `platform/auth/api/api.go:123` |
| had | GET | `/api/oauth-clients/by-client-id/{clientId}` | `platform/auth/api/api.go:134` |
| had | DELETE | `/api/oauth-clients/{id}` | `platform/auth/api/api.go:135` |
| had | GET | `/api/oauth-clients/{id}` | `platform/auth/api/api.go:124` |
| had | PUT | `/api/oauth-clients/{id}` | `platform/auth/api/api.go:125` |
| had | POST | `/api/oauth-clients/{id}/activate` | `platform/auth/api/api.go:126` |
| had | POST | `/api/oauth-clients/{id}/deactivate` | `platform/auth/api/api.go:127` |
| had | POST | `/api/oauth-clients/{id}/regenerate-secret` | `platform/auth/api/api.go:132` |
| had | POST | `/api/oauth-clients/{id}/revoke-previous-secret` | `platform/auth/api/api.go:133` |
| had | POST | `/api/oauth-clients/{id}/rotate-secret` | `platform/auth/api/api.go:128` |
| ported | GET | `/api/openapi.json` | `server/wire_spec.go:16` |
| ported | GET | `/api/openapi.yaml` | `server/wire_spec.go:25` |
| ported | DELETE | `/api/platform-config/access/{id}` | `platform/platformconfig/api/api.go:40` |
| ported | GET | `/api/platform-config/{app}` | `platform/platformconfig/api/api.go:32` |
| ported | GET | `/api/platform-config/{app}/access` | `platform/platformconfig/api/api.go:38` |
| ported | POST | `/api/platform-config/{app}/access` | `platform/platformconfig/api/api.go:39` |
| had | GET | `/api/platform/cors` | `platform/cors/api/api.go:31` |
| had | POST | `/api/platform/cors` | `platform/cors/api/api.go:32` |
| had | GET | `/api/platform/cors/allowed` | `platform/cors/api/api.go:30` |
| had | DELETE | `/api/platform/cors/{id}` | `platform/cors/api/api.go:34` |
| had | GET | `/api/platform/cors/{id}` | `platform/cors/api/api.go:33` |
| ported | GET | `/api/portal-apps` | `platform/portalidentity/api/api.go:83` |
| ported | POST | `/api/portal-apps` | `platform/portalidentity/api/api.go:84` |
| ported | DELETE | `/api/portal-apps/{id}` | `platform/portalidentity/api/api.go:86` |
| ported | PUT | `/api/portal-apps/{id}` | `platform/portalidentity/api/api.go:85` |
| ported | POST | `/api/portal-apps/{id}/assign-unassigned` | `platform/portalidentity/api/api.go:87` |
| ported | GET | `/api/portal-users` | `platform/portalidentity/api/api.go:75` |
| ported | POST | `/api/portal-users` | `platform/portalidentity/api/api.go:74` |
| ported | DELETE | `/api/portal-users/{id}` | `platform/portalidentity/api/api.go:78` |
| ported | POST | `/api/portal-users/{id}/activate` | `platform/portalidentity/api/api.go:76` |
| ported | POST | `/api/portal-users/{id}/apps` | `platform/portalidentity/api/api.go:79` |
| ported | DELETE | `/api/portal-users/{id}/apps/{portalAppCode}` | `platform/portalidentity/api/api.go:80` |
| ported | POST | `/api/portal-users/{id}/deactivate` | `platform/portalidentity/api/api.go:77` |
| had | GET | `/api/principals` | `platform/principal/api/api.go:90` |
| ported (`feat/api-core`) | POST | `/api/principals` | `platform/principal/api/api.go:91` |
| ported | POST | `/api/principals/bulk-import` | `platform/principal/api/api.go:93` |
| had | GET | `/api/principals/check-email-domain` | `platform/principal/api/api.go:103` |
| ported | GET | `/api/principals/developer-users` | `platform/principal/api/api.go:116` |
| had | POST | `/api/principals/sync` | `platform/principal/api/api.go:94` |
| had | POST | `/api/principals/users` | `platform/principal/api/api.go:92` |
| had | DELETE | `/api/principals/{id}` | `platform/principal/api/api.go:104` |
| had | GET | `/api/principals/{id}` | `platform/principal/api/api.go:95` |
| had | PUT | `/api/principals/{id}` | `platform/principal/api/api.go:97` |
| had | POST | `/api/principals/{id}/activate` | `platform/principal/api/api.go:98` |
| had | GET | `/api/principals/{id}/application-access` | `platform/principal/api/api.go:110` |
| had | PUT | `/api/principals/{id}/application-access` | `platform/principal/api/api.go:109` |
| had | GET | `/api/principals/{id}/available-applications` | `platform/principal/api/api.go:111` |
| had | GET | `/api/principals/{id}/client-access` | `platform/principal/api/api.go:112` |
| had | POST | `/api/principals/{id}/client-access` | `platform/principal/api/api.go:113` |
| had | DELETE | `/api/principals/{id}/client-access/{clientId}` | `platform/principal/api/api.go:114` |
| ported | PUT | `/api/principals/{id}/client-association` | `platform/principal/api/api.go:115` |
| had | POST | `/api/principals/{id}/deactivate` | `platform/principal/api/api.go:99` |
| ported | DELETE | `/api/principals/{id}/developer-credential` | `platform/principal/api/api.go:118` |
| ported | POST | `/api/principals/{id}/developer-credential` | `platform/principal/api/api.go:117` |
| ported | POST | `/api/principals/{id}/reset-2fa` | `platform/principal/api/api.go:102` |
| had | POST | `/api/principals/{id}/reset-password` | `platform/principal/api/api.go:100` |
| had | GET | `/api/principals/{id}/roles` | `platform/principal/api/api.go:106` |
| had | POST | `/api/principals/{id}/roles` | `platform/principal/api/api.go:107` |
| had | PUT | `/api/principals/{id}/roles` | `platform/principal/api/api.go:105` |
| had | DELETE | `/api/principals/{id}/roles/{role}` | `platform/principal/api/api.go:108` |
| had | POST | `/api/principals/{id}/send-password-reset` | `platform/principal/api/api.go:101` |
| ported | GET | `/api/principals/{id}/version` | `platform/principal/api/api.go:96` |
| had | GET | `/api/processes` | `platform/process/api/api.go:38` |
| had | POST | `/api/processes` | `platform/process/api/api.go:39` |
| had | GET | `/api/processes/by-code/{code}` | `platform/process/api/api.go:40` |
| ported | POST | `/api/processes/sync` | `platform/sdksync/api.go:108` |
| had | DELETE | `/api/processes/{id}` | `platform/process/api/api.go:44` |
| had | GET | `/api/processes/{id}` | `platform/process/api/api.go:41` |
| had | PUT | `/api/processes/{id}` | `platform/process/api/api.go:42` |
| had | POST | `/api/processes/{id}/archive` | `platform/process/api/api.go:43` |
| had | GET | `/api/public/login-theme` | `platform/publicapi/endpoint.go:58` |
| had | GET | `/api/public/platform` | `platform/publicapi/endpoint.go:57` |
| ported | GET | `/api/reset-approvals` | `platform/resetapproval/api/api.go:42` |
| ported | POST | `/api/reset-approvals/{id}/approve` | `platform/resetapproval/api/api.go:43` |
| ported | POST | `/api/reset-approvals/{id}/deny` | `platform/resetapproval/api/api.go:44` |
| had | GET | `/api/roles` | `platform/role/api/api.go:35` |
| had | POST | `/api/roles` | `platform/role/api/api.go:36` |
| had | GET | `/api/roles/by-application/{applicationId}` | `platform/role/api/api.go:43` |
| had | GET | `/api/roles/by-code/{code}` | `platform/role/api/api.go:41` |
| had | GET | `/api/roles/by-source/{source}` | `platform/role/api/api.go:42` |
| had | GET | `/api/roles/filters/applications` | `platform/role/api/api.go:44` |
| had | GET | `/api/roles/permissions` | `platform/role/api/api.go:54` |
| ported | DELETE | `/api/roles/permissions/{permission}` | `platform/role/api/api.go:56` |
| had | GET | `/api/roles/permissions/{permission}` | `platform/role/api/api.go:55` |
| had | DELETE | `/api/roles/{id}` | `platform/role/api/api.go:39` |
| had | GET | `/api/roles/{id}` | `platform/role/api/api.go:37` |
| had | PUT | `/api/roles/{id}` | `platform/role/api/api.go:38` |
| ported | GET | `/api/roles/{roleName}/permissions` | `platform/role/api/api.go:46` |
| had | POST | `/api/roles/{roleName}/permissions` | `platform/role/api/api.go:51` |
| had | DELETE | `/api/roles/{roleName}/permissions/{permission}` | `platform/role/api/api.go:52` |
| ported | POST | `/api/roles/{roleName}/permissions/{permission}` | `platform/role/api/api.go:47` |
| had | GET | `/api/scheduled-jobs` | `platform/scheduledjob/api/api.go:36` |
| had | POST | `/api/scheduled-jobs` | `platform/scheduledjob/api/api.go:37` |
| had | GET | `/api/scheduled-jobs/by-code/{code}` | `platform/scheduledjob/api/api.go:45` |
| had | GET | `/api/scheduled-jobs/instances/{instanceId}` | `platform/scheduledjob/api/api.go:47` |
| had | POST | `/api/scheduled-jobs/instances/{instanceId}/complete` | `platform/scheduledjob/api/api.go:50` |
| had | POST | `/api/scheduled-jobs/instances/{instanceId}/log` | `platform/scheduledjob/api/api.go:49` |
| had | GET | `/api/scheduled-jobs/instances/{instanceId}/logs` | `platform/scheduledjob/api/api.go:48` |
| had | DELETE | `/api/scheduled-jobs/{id}` | `platform/scheduledjob/api/api.go:44` |
| had | GET | `/api/scheduled-jobs/{id}` | `platform/scheduledjob/api/api.go:38` |
| had | PUT | `/api/scheduled-jobs/{id}` | `platform/scheduledjob/api/api.go:39` |
| had | POST | `/api/scheduled-jobs/{id}/archive` | `platform/scheduledjob/api/api.go:42` |
| had | POST | `/api/scheduled-jobs/{id}/fire` | `platform/scheduledjob/api/api.go:43` |
| had | GET | `/api/scheduled-jobs/{id}/instances` | `platform/scheduledjob/api/api.go:46` |
| had | POST | `/api/scheduled-jobs/{id}/pause` | `platform/scheduledjob/api/api.go:40` |
| had | POST | `/api/scheduled-jobs/{id}/resume` | `platform/scheduledjob/api/api.go:41` |
| had | GET | `/api/service-accounts` | `platform/serviceaccount/api/api.go:61` |
| had | POST | `/api/service-accounts` | `platform/serviceaccount/api/api.go:62` |
| had | GET | `/api/service-accounts/code/{code}` | `platform/serviceaccount/api/api.go:63` |
| had | DELETE | `/api/service-accounts/{id}` | `platform/serviceaccount/api/api.go:67` |
| had | GET | `/api/service-accounts/{id}` | `platform/serviceaccount/api/api.go:64` |
| had | PUT | `/api/service-accounts/{id}` | `platform/serviceaccount/api/api.go:65` |
| ported | POST | `/api/service-accounts/{id}/deactivate` | `platform/serviceaccount/api/api.go:66` |
| had | POST | `/api/service-accounts/{id}/regenerate-auth-token` | `platform/serviceaccount/api/api.go:75` |
| ported | POST | `/api/service-accounts/{id}/regenerate-secret` | `platform/serviceaccount/api/api.go:80` |
| had | POST | `/api/service-accounts/{id}/regenerate-signing-secret` | `platform/serviceaccount/api/api.go:80` |
| ported | POST | `/api/service-accounts/{id}/regenerate-token` | `platform/serviceaccount/api/api.go:75` |
| had | GET | `/api/service-accounts/{id}/roles` | `platform/serviceaccount/api/api.go:68` |
| had | PUT | `/api/service-accounts/{id}/roles` | `platform/serviceaccount/api/api.go:69` |
| ported | POST | `/api/service-accounts/{id}/token` | `platform/serviceaccount/api/api.go:84` |
| had | GET | `/api/subscriptions` | `platform/subscription/api/api.go:32` |
| had | POST | `/api/subscriptions` | `platform/subscription/api/api.go:33` |
| had | DELETE | `/api/subscriptions/{id}` | `platform/subscription/api/api.go:36` |
| had | GET | `/api/subscriptions/{id}` | `platform/subscription/api/api.go:34` |
| had | PUT | `/api/subscriptions/{id}` | `platform/subscription/api/api.go:35` |
| had | POST | `/api/subscriptions/{id}/pause` | `platform/subscription/api/api.go:37` |
| had | POST | `/api/subscriptions/{id}/resume` | `platform/subscription/api/api.go:38` |
| ported | POST | `/auth/2fa/challenge/email` | `platform/auth/login/twofactor.go:63` |
| ported | POST | `/auth/2fa/enroll/email/begin` | `platform/auth/login/twofactor.go:66` |
| ported | POST | `/auth/2fa/enroll/email/confirm` | `platform/auth/login/twofactor.go:67` |
| ported | POST | `/auth/2fa/enroll/totp/begin` | `platform/auth/login/twofactor.go:64` |
| ported | POST | `/auth/2fa/enroll/totp/confirm` | `platform/auth/login/twofactor.go:65` |
| ported | POST | `/auth/2fa/methods/email/begin` | `platform/auth/login/twofactor_selfservice.go:28` |
| ported | POST | `/auth/2fa/methods/email/confirm` | `platform/auth/login/twofactor_selfservice.go:29` |
| ported | POST | `/auth/2fa/methods/totp/begin` | `platform/auth/login/twofactor_selfservice.go:26` |
| ported | POST | `/auth/2fa/methods/totp/confirm` | `platform/auth/login/twofactor_selfservice.go:27` |
| ported | DELETE | `/auth/2fa/methods/{method}` | `platform/auth/login/twofactor_selfservice.go:30` |
| ported | POST | `/auth/2fa/recovery-codes/regenerate` | `platform/auth/login/twofactor_selfservice.go:31` |
| ported | GET | `/auth/2fa/status` | `platform/auth/login/twofactor_selfservice.go:25` |
| ported | GET | `/auth/2fa/trusted-devices` | `platform/auth/login/twofactor_selfservice.go:32` |
| ported | DELETE | `/auth/2fa/trusted-devices/{id}` | `platform/auth/login/twofactor_selfservice.go:33` |
| ported | POST | `/auth/2fa/verify` | `platform/auth/login/twofactor.go:62` |
| ported | POST | `/auth/change-password` | `platform/auth/login/endpoint.go:143` |
| ported | POST | `/auth/change-password/send-email-code` | `platform/auth/login/endpoint.go:144` |
| had | GET | `/auth/check-domain` | `platform/auth/login/endpoint.go:124` |
| had | POST | `/auth/check-domain` | `platform/auth/login/endpoint.go:121` |
| had | GET | `/auth/client/accessible` | `platform/auth/clientselection/clientselection.go:34` |
| had | GET | `/auth/client/current` | `platform/auth/clientselection/clientselection.go:36` |
| had | POST | `/auth/client/switch` | `platform/auth/clientselection/clientselection.go:35` |
| had | POST | `/auth/login` | `platform/auth/login/endpoint.go:125` |
| ported | GET | `/auth/login-history` | `platform/auth/login/endpoint.go:146` |
| had | POST | `/auth/logout` | `platform/auth/login/endpoint.go:126` |
| had | GET | `/auth/me` | `platform/auth/login/endpoint.go:140` |
| had | GET | `/auth/oidc/callback` | `platform/auth/bridge/login_endpoint.go:160` |
| had | GET | `/auth/oidc/login` | `platform/auth/bridge/login_endpoint.go:159` |
| had | GET | `/auth/oidc/session/end` | `platform/auth/bridge/login_endpoint.go:161` |
| had | POST | `/auth/password-reset/confirm` | `platform/passwordreset/api/api.go:441` |
| had | POST | `/auth/password-reset/request` | `platform/passwordreset/api/api.go:439` |
| had | GET | `/auth/password-reset/validate` | `platform/passwordreset/api/api.go:440` |
| ported | POST | `/auth/password-setup/request` | `platform/passwordreset/api/api.go:447` |
| had | POST | `/auth/refresh` | `platform/auth/login/endpoint.go:132` |
| had | POST | `/auth/webauthn/authenticate/begin` | `platform/webauthn/api/api.go:84` |
| had | POST | `/auth/webauthn/authenticate/complete` | `platform/webauthn/api/api.go:85` |
| had | GET | `/auth/webauthn/credentials` | `platform/webauthn/api/api.go:86` |
| had | DELETE | `/auth/webauthn/credentials/{id}` | `platform/webauthn/api/api.go:87` |
| had | POST | `/auth/webauthn/register/begin` | `platform/webauthn/api/api.go:82` |
| had | POST | `/auth/webauthn/register/complete` | `platform/webauthn/api/api.go:83` |
| had | GET | `/bff/dashboard/stats` | `platform/shared/bff/dashboard.go:47` |
| had | GET | `/bff/debug/dispatch-jobs` | `platform/dispatchjob/api/api.go:95` |
| had | GET | `/bff/debug/events` | `platform/event/api/api.go:55` |
| had | GET | `/bff/debug/events/{id}` | `platform/event/api/api.go:58` |
| had | GET | `/bff/developer/applications` | `platform/shared/bff/developer.go:58` |
| had | GET | `/bff/developer/applications/{appId}` | `platform/shared/bff/developer.go:59` |
| had | GET | `/bff/developer/applications/{appId}/event-types` | `platform/shared/bff/developer.go:63` |
| had | GET | `/bff/developer/applications/{appId}/openapi/current` | `platform/shared/bff/developer.go:60` |
| had | GET | `/bff/developer/applications/{appId}/openapi/versions` | `platform/shared/bff/developer.go:61` |
| had | GET | `/bff/developer/applications/{appId}/openapi/versions/{specId}` | `platform/shared/bff/developer.go:62` |
| had | POST | `/bff/developer/sync-platform-openapi` | `platform/shared/bff/developer.go:64` |
| had | GET | `/bff/dispatch-jobs` | `platform/dispatchjob/api/api.go:102` |
| ported | GET | `/bff/dispatch-jobs/event/{eventId}` | `platform/dispatchjob/api/api.go:105` |
| had | GET | `/bff/dispatch-jobs/filter-options` | `platform/dispatchjob/api/api.go:104` |
| ported | GET | `/bff/dispatch-jobs/list-raw` | `platform/dispatchjob/api/api.go:103` |
| ported | POST | `/bff/dispatch-jobs/requeue` | `platform/dispatchjob/api/api.go:109` |
| had | GET | `/bff/dispatch-jobs/{id}` | `platform/dispatchjob/api/api.go:106` |
| had | GET | `/bff/dispatch-jobs/{id}/attempts` | `platform/dispatchjob/api/api.go:108` |
| ported | POST | `/bff/dispatch-jobs/{id}/cancel` | `platform/dispatchjob/api/api.go:110` |
| ported | POST | `/bff/dispatch-jobs/{id}/complete` | `platform/dispatchjob/api/api.go:111` |
| had | GET | `/bff/dispatch-jobs/{id}/raw` | `platform/dispatchjob/api/api.go:107` |
| ported | POST | `/bff/dispatch-jobs/{id}/sign` | `platform/dispatchjob/api/api.go:112` |
| had | GET | `/bff/event-types` | `platform/shared/bff/event_types.go:37` |
| had | POST | `/bff/event-types` | `platform/shared/bff/event_types.go:38` |
| had | GET | `/bff/event-types/filters/aggregates` | `platform/shared/bff/event_types.go:41` |
| had | GET | `/bff/event-types/filters/applications` | `platform/shared/bff/filter_options.go:49` |
| had | GET | `/bff/event-types/filters/subdomains` | `platform/shared/bff/event_types.go:40` |
| had | POST | `/bff/event-types/sync-platform` | `platform/shared/bff/event_types.go:39` |
| had | DELETE | `/bff/event-types/{id}` | `platform/shared/bff/event_types.go:48` |
| had | GET | `/bff/event-types/{id}` | `platform/shared/bff/event_types.go:42` |
| had | PATCH | `/bff/event-types/{id}` | `platform/shared/bff/event_types.go:47` |
| ported | PUT | `/bff/event-types/{id}` | `platform/shared/bff/event_types.go:43` |
| had | POST | `/bff/event-types/{id}/archive` | `platform/shared/bff/event_types.go:49` |
| had | POST | `/bff/event-types/{id}/schemas` | `platform/shared/bff/event_types.go:50` |
| had | POST | `/bff/event-types/{id}/schemas/{version}/deprecate` | `platform/shared/bff/event_types.go:52` |
| had | POST | `/bff/event-types/{id}/schemas/{version}/finalise` | `platform/shared/bff/event_types.go:51` |
| had | GET | `/bff/events` | `platform/event/api/api.go:69` |
| had | POST | `/bff/events/batch` | `platform/event/api/api.go:66` |
| had | GET | `/bff/events/filter-options` | `platform/event/api/api.go:67` |
| ported | GET | `/bff/events/list-raw` | `platform/event/api/api.go:68` |
| had | GET | `/bff/events/{id}` | `platform/event/api/api.go:70` |
| had | GET | `/bff/filter-options/clients` | `platform/shared/bff/filter_options.go:48` |
| had | GET | `/bff/processes` | `platform/process/api/api.go:38` |
| had | POST | `/bff/processes` | `platform/process/api/api.go:39` |
| had | GET | `/bff/processes/by-code/{code}` | `platform/process/api/api.go:40` |
| had | DELETE | `/bff/processes/{id}` | `platform/process/api/api.go:44` |
| had | GET | `/bff/processes/{id}` | `platform/process/api/api.go:41` |
| had | PUT | `/bff/processes/{id}` | `platform/process/api/api.go:42` |
| had | POST | `/bff/processes/{id}/archive` | `platform/process/api/api.go:43` |
| had | GET | `/bff/roles` | `platform/shared/bff/roles.go:46` |
| had | POST | `/bff/roles` | `platform/shared/bff/roles.go:47` |
| had | GET | `/bff/roles/filters/applications` | `platform/shared/bff/roles.go:49` |
| had | GET | `/bff/roles/permissions` | `platform/shared/bff/roles.go:50` |
| ported | POST | `/bff/roles/permissions` | `platform/shared/bff/roles.go:51` |
| had | GET | `/bff/roles/permissions/{permission}` | `platform/shared/bff/roles.go:52` |
| had | POST | `/bff/roles/sync-platform` | `platform/shared/bff/roles.go:48` |
| had | DELETE | `/bff/roles/{roleName}` | `platform/shared/bff/roles.go:55` |
| had | GET | `/bff/roles/{roleName}` | `platform/shared/bff/roles.go:53` |
| had | PUT | `/bff/roles/{roleName}` | `platform/shared/bff/roles.go:54` |
| had | GET | `/bff/scheduled-jobs` | `platform/shared/bff/scheduled_jobs.go:47` |
| had | GET | `/bff/scheduled-jobs/filter-options` | `platform/shared/bff/scheduled_jobs.go:48` |
| had | GET | `/bff/scheduled-jobs/instances/{instanceId}` | `platform/shared/bff/scheduled_jobs.go:49` |
| had | GET | `/bff/scheduled-jobs/instances/{instanceId}/logs` | `platform/shared/bff/scheduled_jobs.go:50` |
| had | GET | `/bff/scheduled-jobs/{id}` | `platform/shared/bff/scheduled_jobs.go:51` |
| had | GET | `/bff/scheduled-jobs/{id}/instances` | `platform/shared/bff/scheduled_jobs.go:52` |
| had | GET | `/oauth/authorize` | `platform/auth/oauthapi/authorize.go:24` |
| had | POST | `/oauth/introspect` | `platform/auth/oauthapi/introspect_revoke.go:15` |
| had | POST | `/oauth/revoke` | `platform/auth/oauthapi/introspect_revoke.go:20` |
| had | POST | `/oauth/token` | `platform/auth/oauthapi/token.go:172` |
| had | GET | `/oauth/userinfo` | `platform/auth/oauthapi/userinfo.go:17` |
| had | POST | `/oauth/userinfo` | `platform/auth/oauthapi/userinfo.go:18` |
| ported | POST | `/portal/auth/check-domain` | `platform/portalauth/endpoints.go:58` |
| ported | POST | `/portal/auth/login` | `platform/portalauth/endpoints.go:59` |
| ported | GET | `/portal/auth/oidc/login` | `platform/auth/bridge/login_endpoint.go:970` |
| ported | POST | `/portal/auth/password-reset` | `platform/portalauth/endpoints.go:60` |
| ported | GET | `/portal/authorize` | `platform/portalauth/endpoints.go:57` |
| had | GET | `/q/openapi` | `server/wire_spec.go:38` |
| had | GET | `/swagger-ui` | `server/wire_spec.go:47` |

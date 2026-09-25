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

MISC_SECTION

## Migrations

| Rust | Go | What |
|---|---|---|
| `041_mfa_tables` | 031 | `iam_user_mfa_methods`, `iam_user_mfa_recovery_codes`, `iam_mfa_email_pins`, `iam_mfa_trusted_devices`, the mapping's `require_2fa` / `remember_device_*` and `tnt_email_domain_mapping_2fa_methods` |
| `042_password_reset_token_purpose` | 031, 032, 033, 041, 051 | `iam_password_reset_tokens.{purpose, reset_2fa, requires_factor, factor_attempts, redirect_uri}` and the purpose CHECK |
| `043_developer_api_credentials` | 039 | `iam_principals.dev_client_secret_ref`, `dev_client_secret_updated_at` |
| `044_reset_approval_requests` | 032 | `iam_reset_approval_requests` |
| `046_portal_identities` | 041, 043 | `portal_identities`, `portal_login_flows`, portal flags on OAuth clients and OIDC login states |
| `047_portal_apps` | 053 | `portal_apps`, `portal_identity_apps`, `oauth_clients.portal_app_id`, invite columns |
MISC_MIGRATIONS

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
MISC_DEVIATIONS

## Remaining

REMAINING_SECTION

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
- The login backoff keys on the client IP: Rust's `ClientIp` reads only a
  trusted `X-Forwarded-For`, never the socket peer, so a direct connection has
  no IP (no per-(email, IP) backoff, no `ipAddress` in the sign-in history).
  Go falls back to the peer address. `shared/middleware.rs` is owned
  elsewhere.
- `GET /auth/oidc/login` without `domain` answers axum's plain-text query error
  (Go: JSON `DOMAIN_REQUIRED`), and an internal domain answers a plain message
  (Go: `OIDC_NOT_CONFIGURED`).
- OAuth client responses lack Go's `apiAccess` and `applications`.

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

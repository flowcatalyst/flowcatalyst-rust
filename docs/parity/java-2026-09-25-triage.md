# Java commits 0118cdca..65988b51 (92 commits), triaged for Rust

Read-only review of `../flowcatalyst-javalin`, 2026-09-25. Nearly all of it is the
Java session's **security sweep (S0–S3)** and the **owner rulings of 2026-09-25**
(Java `docs/backlog.md` @ f6e10994, items 1–19). The function commits change nothing
in the management interface. Paths are relative to `crates/fc-platform/src` unless
noted.

## Already fixed here

- `c7f89290` (Java 4ac18182): the function host parses `clients`/`applications`
  pairs to ids (every non-anchor versioned call used to 404), and refuses
  `token_use=identity`.

## Adopt: security, no owner decision needed (Go parity, or a hole Go shares)

| # | Fix | Rust today | Go |
|---|---|---|---|
| S1 | `/api/events/batch` and `/api/dispatch-jobs/batch` need Go's `platform:messaging:batch:{events,dispatch-jobs}-write` | `shared/batch_api.rs:100` (`_auth`), `shared/sdk_dispatch_jobs_api.rs` (client check only) | checks it |
| S2 | `/auth/refresh` refuses a refresh token issued to an OAuth client (it is swapped for a full-authority token today) | `auth/auth_api.rs:560-595` | refuses |
| S3 | Service-account role assignment, token and secret regeneration need anchor **plus** `SERVICE_ACCOUNT_UPDATE`. An app's own ANCHOR-tier SA can grant itself super-admin today | `service_account/api.rs:727,565,605,645` | same hole |
| S4 | Principal writes: add a permission to the tier check (Go `RequireUserAdmin`); `update_principal` has none at all. About 40 anchor-only gates in client, IdP, email-domain, config and CORS want Go's `anchorWith(perm)` | `principal/api.rs:557,955,1087,…` | permission |
| S5 | Signing reach at ingest: a dispatch job's `subscriptionId`, or its code's app prefix, picks the SA whose token is sent to the caller's URL | `dispatch_job/delivery_credentials.rs:113-156`, `sdk_dispatch_jobs_api.rs:106` | same hole |
| S6 | Event ingest reach (ruling 17a): only an app, its anchor or a super-admin may ingest an app's event types | `event/api.rs:261,487` | same hole |
| S7 | Subscriptions and connections: `SigningReach` on create and update; port Go's sync `CONNECTION_SCOPE_MISMATCH` | `subscription/operations/{create,update,sync}.rs` | sync only |
| S8 | Multi-tenant OIDC skips the `aud` check | `auth/oidc_login_api.rs:1038-1043` | checks `aud` |
| S9 | `/oauth/authorize` session cookie only (ruling 6); check the principal is active at code redemption | `auth/oauth_api.rs:370-379,1219` | bearer fallback |
| S10 | `/auth/client/*` and passkey routes: session cookie only (S2.1, S2.4) | `shared/middleware.rs:131`, `webauthn/api.rs` | — |
| S11 | Audit-log API redacts on read, using `redact_stored_document` (Go-era rows are unredacted) | `audit/api.rs:80` | — |
| S12 | Refresh rotation: atomic consume, family and reuse detection, inherited expiry, 10 s sibling leeway | `auth/oauth_api.rs:1372,1418,1537` | family revoke |
| S13 | Bounded delivery response reads (64 KiB, as Go) | `shared/dispatch_process_api.rs:150,270`, scheduler dispatcher | capped |
| S14 | App sync strips only its own SDK roles (037e6f56) | `principal/operations/sync.rs:153-162,209-218` | named-user path buggy |
| S15 | `client_credentials` requires a SERVICE principal | `auth/oauth_api.rs:1708` | — |

## Adopt: correctness and availability

- **Cutover blocker:** app-scoped `/api/applications/{app}/principals/sync` drops
  `passwordHash` (`shared/sdk_sync_api.rs:653`, `principal/operations/sync.rs:25`).
  Go passes it through, and hr/rfp send it.
- `aud_logs.entity_id` VARCHAR(17) is too narrow for sync rollups keyed by app code
  (Java V18 → 100). This needs a new migration.
- A unique-key race at persist returns 500; it should be 409 `DUPLICATE_KEY` (9d71ffd4).
- The SPA shell has no `Cache-Control` (8fd35a8b); set `no-cache, no-store, must-revalidate`.
- Password-reset request: add the per-email bucket (defined but unused).
- Ruling 16: `/bff/event-types/sync-platform` always uses `platform`; any other
  application is 400 `PLATFORM_SYNC_ONLY`.
- Ruling 8: already 404; add the code `EMAIL_DOMAIN_NOT_MAPPED` and the INFO log.
- Service-account create: add Go's `allApplications` opt-in (403 without all-apps
  reach; 400 alongside `applicationId`).

## Adopt: owner rulings that go beyond Go

- **14 + 13 role ceiling:** assigning roles needs `USER_ASSIGN_ROLES` or
  `SERVICE_ACCOUNT_UPDATE`. A caller may add or remove only roles whose every
  permission it holds (403 `ROLE_ABOVE_CALLER`). The same applies to principal sync,
  IdP role mappings and email-domain `allowedRoles`. Seed admin, iam-admin,
  iam-readonly and viewer with the service-account permissions.
  `POST /api/principals/sync` has **no** anchor check (any `USER_CREATE` holder can
  sync `platform:super-admin`).
- **15:** a role may hold only its own app's permissions (400
  `PERMISSION_OUTSIDE_APPLICATION`); the exception is a super-admin via the admin API.
  Pre-deploy check:
  `SELECT r.name, p.permission FROM iam_roles r JOIN iam_role_permissions p ON p.role_id = r.id WHERE split_part(p.permission, ':', 1) <> r.application_code;`
- **3:** a multi-tenant IdP mapping must pin the tenant (400 `TENANT_PIN_REQUIRED` on
  save, 403 `TENANT_NOT_PINNED` at login). (c) allowed-tenant IDs is optional: Rust has
  no provider-direct login. Pre-deploy check:
  `SELECT m.email_domain, p.code FROM tnt_email_domain_mappings m JOIN oauth_identity_providers p ON p.id = m.identity_provider_id WHERE p.oidc_multi_tenant AND coalesce(m.required_oidc_tenant_id,'') = '';`
- **10 listener timeouts:** 75 s keep-alive idle and 30 s to read the request, on the
  platform and function-host listeners. Go has a 10 s `ReadHeaderTimeout` only.
- **2 router auth:** platform bearer tokens,
  `platform:messaging:router:{view,operate}`, the `platform:router-operator` role,
  mock and seed routes in dev only, and a PKCE dashboard. **Order matters:** first ship
  TS and Laravel SDK releases that send the bearer token (Java 714f3f2d) and get them
  into integral, hr and rfp, before any router enforces it.
- **5, 11 SDKs:** port e0a9fd13 (single-flight refresh, TS and Laravel) and f55afed6
  (webhook `check()`, TS and Laravel). Rust's copies are byte-identical to Java's base.

## Adopt: function host

- 503 `VERSION_NOT_READY` plus `Retry-After` for a version still preparing (ruling 12;
  `reconciler.rs:524`, `pipeline.rs:541`).
- `emit` returns the event id and the platform's `message`, and logs the transport
  cause. Host side is small; the WIT changes additively (`@0.1.1`, or a new function).
- Log a refused version only when the refusal is new or changed; today every lazy
  retry logs it (5afabe52).
- The stored-manifest reader reports the parts it drops (892c711b).
- `fn validate` CLI (POST `…/manifest/check`).
- Optional: 16 MiB outbound response cap for an identical author contract. Rust
  already streams, so the host is safe either way.
- W4 function DB access: Java's `DB_*` codes, caps and deadline timeouts over
  decision #7's shared pool. Add a per-function semaphore, a per-invocation cap, and a
  `ROLLBACK`/`DISCARD ALL` on release (Java's open mid-transaction bug).

## Needs an owner decision

1. **Ruling 4, password hash on sync.** Java never applies it to an existing
   principal. hr and rfp re-run `sync-test-principals` expecting it to rotate a
   shared test password. Today any `USER_CREATE` holder can overwrite an anchor
   admin's hash.
2. **App-sync role names:** refuse unknown names (Java), or only names prefixed
   `platform:` or with another app's code (safe for the unknown Laravel inputs).
3. **Ingest tenancy:** a null or unknown `clientCode` from a non-anchor caller (Go
   accepts, Java refuses). Honour a supplied outbox `id` with a 409 on duplicates (Go
   parity)?
4. **Anchor plus permission** on `/api/roles` and client access: stricter than Go, in
   the spirit of #19.
5. **Session cookie:** Rust's is a 24 h full-authority JWT. Go's holds only the
   subject and reloads the principal per request, so deactivation is immediate and Go
   cookies survive cutover.
6. **JS functions (#6):** the manifest runtime value (`js`?) and the artifact format;
   reuse Java's `@flowcatalyst/function` API surface.

## Cutover gaps found (Go routes Rust lacks)

`/auth/password-setup/request`, 2FA/TOTP, reset-2fa, and developer credentials.

## Not applicable or already safe

- Path normalisation: axum gates are route-attached.
- `SecureTokens`: every site uses a CSPRNG with at least 32 bytes.
- S1.4 OAuth-client `principalId`.
- S2.6 (latent: `return_url` isn't wired).
- Duplicate job ids: supplied ids are ignored.
- W6 Rust functions: the PDK is already better.
- W5 SPA: already done.
- JWKS and token-fetch logging: already done.
- fcdev packaging, CI and test fixtures.

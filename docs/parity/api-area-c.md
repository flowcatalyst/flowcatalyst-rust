# API parity, area C: IAM and tenancy

Date: 2026-09-26. Branch `feat/api-area-c`. Scope: the parity harness groups `principals/*`, `clients/*`,
`applications/*`, `service-accounts/*`, `auth-configs/*`, `anchor-domains/*`, `identity-providers/*`,
`idp-role-mappings/*`, `email-domain-mappings/*`, `oauth-clients/*`, `portal*/*`, `reset-approvals/*`,
`webauthn/*`, `login-attempts/*` (19 scenario files, 594 steps). Go is the reference (`flowcatalyst-go` @ `73a6918`);
owner rulings override it (`docs/owner-decisions-2026-09-25.md`).

## Before and after

"Before" is parity run 3 on `main` (`a15dbbf3`, a full run). "After" is this branch, run on the area's groups only
(`--only '{principals,clients,…,login-attempts}/*'`). The step count is the same; a filtered run leaves out other
areas' rows in shared lists, which affects only the list steps noted under "What remains".

| scenario file | before OK / ACC / DIFF / ERR | after OK / ACC / DIFF / ERR |
|---|---|---|
| `anchor-domains/crud.json` | 9 / 0 / 4 / 10 | 21 / 1 / 1 / 0 |
| `applications/crud.json` | 22 / 0 / 24 / 2 | 48 / 0 / 0 / 0 |
| `applications/sdk-sync.json` | 13 / 0 / 11 / 0 | 13 / 0 / 11 / 0 |
| `auth-configs/crud.json` | 9 / 0 / 9 / 8 | 26 / 0 / 0 / 0 |
| `clients/crud.json` | 26 / 0 / 17 / 0 | 42 / 1 / 0 / 0 |
| `email-domain-mappings/email-domain-mappings.json` | 21 / 0 / 10 / 1 | 31 / 0 / 1 / 0 |
| `identity-providers/identity-providers.json` | 14 / 0 / 9 / 5 | 28 / 0 / 0 / 0 |
| `idp-role-mappings/idp-role-mappings.json` | 9 / 0 / 3 / 3 | 13 / 1 / 1 / 0 |
| `login-attempts/login-attempts.json` | 10 / 0 / 7 / 0 | 17 / 0 / 0 / 0 |
| `oauth-clients/oauth-clients.json` | 22 / 0 / 10 / 0 | 32 / 0 / 0 / 0 |
| `portal-apps/portal-apps.json` | 36 / 0 / 8 / 0 | 44 / 0 / 0 / 0 |
| `portal-assign/portal-assign.json` | 19 / 0 / 0 / 0 | 19 / 0 / 0 / 0 |
| `portal-users/portal-users.json` | 45 / 0 / 3 / 0 | 48 / 0 / 0 / 0 |
| `portal/portal.json` | 43 / 0 / 4 / 0 | 45 / 0 / 2 / 0 |
| `principals/principals-access.json` | 23 / 0 / 14 / 1 | 38 / 0 / 0 / 0 |
| `principals/principals-core.json` | 28 / 0 / 15 / 0 | 39 / 1 / 3 / 0 |
| `reset-approvals/reset-approvals.json` | 14 / 0 / 1 / 0 | 15 / 0 / 0 / 0 |
| `service-accounts/service-accounts.json` | 16 / 0 / 16 / 0 | 31 / 0 / 1 / 0 |
| `webauthn/webauthn.json` | 12 / 0 / 8 / 0 | 15 / 0 / 5 / 0 |
| **total (594 steps)** | **391 / 0 / 173 / 30** | **565 / 4 / 25 / 0** |

## What changed

One commit per change; each message says what is now true.

| Commit | What |
|---|---|
| `807f6aee` | Anchor domains, auth configs and IdP role mappings: 201 creates, Go's `{items}` lists and members, Go's validation codes, the full auth-config member set (secret sealed like the IdP secret), and the granted-clients route writes the granted list (it wrote the additional one). |
| `d288e32d` | OAuth clients: `apiAccess` and `applications` refs on every response (the SPA's list reads `applications.length`), create keeps `defaultScopes`/`allowedOrigins`, Go's defaults (PKCE on, grant types as sent) and codes. |
| `41a54040` | Applications: `website`/`logo`/`logoMimeType` kept (they were dropped), Go's response members, INVALID_CODE_FORMAT/CODE_EXISTS, Go's sub-route shapes (roles as names, client configs as `{items}`, enable/disable 204, repeatable login-client provisioning). The Rust SDK follows. |
| `7ca90e10` | Clients: `notes` on responses, Go's identifier rule and codes, `platform` reserved. |
| `8c6fa4e7` | Principals: `hasDeveloperCredential`, `developerCredentialUpdatedAt`, `twoFactorMethods` (detail read), `serviceAccountId`, the stored `idpType`; 404 (not 403) out of reach; Go's available-applications, check-email-domain, revoke (204) and bulk-import shapes. |
| `75e4b791` | Service accounts: Go's two ids (the account's `sac_` id is the API `id`, the SERVICE principal's `prn_` id is `principalId`); either id addresses the routes; `oauthClientId`; Go's list; INVALID_AUTH_TYPE; `lastUsedAt` stamped on authentication and mint. |
| `c0dcf9b0` | `/oauth/token` accepts `client_secret_basic` for every grant (client_credentials refused a Basic-only caller) and records the caller's address on service-account and developer token attempts. |
| `ef0e2622` | Login attempts: a bad cursor reads from the start and an out-of-range size reads 50, as Go. |
| `e2153f4b` | Portal: bodies checked as Go's huma schema (required members, the clientType enum); Go's OIDC-resolve message. |
| `d3633466` | Passkeys: EMAIL_REQUIRED on an empty email, go-webauthn's parse message, `lastUsedAt` omitted until used. |
| `e369ce45` | A login with no permissions answers `permissions: null`. |
| `be97da4d` | Identity providers and email-domain mappings on Go's model: routed domains are the mappings (create/update map, claim and release them in one transaction), delete guards, role sync on the provider (migration 053), mapping writes through the unit of work. |
| `2f731fbb` | User passwords checked against Go's policy (create, reset, change-password) with Go's codes. |
| `ac95872e` | Allow-list entries for the differences owner decisions make deliberate. |

## Findings worth knowing at cutover

- **Identity providers and mappings disagreed on a Go database.** Go's 040 moved domain routing to the mappings and
  role sync to the provider; Rust still read a dead domain junction and the mapping's role-sync columns. After
  cutover a Rust platform would have shown providers without their domains, refused portal SSO through a
  multi-tenant provider (its "allowed domains" looked empty), ignored a provider's role-sync switch, and compared
  the mapping's role ids against role names (rejecting every role when an allow-list existed). Fixed in `be97da4d`.
  Migration **053** (`053_identity_provider_role_sync`) adds Go's column and junction to a database Rust created; on a
  Go database its probe marks it applied. A Rust-created database keeps the provider domains it stored only in the
  old junction out of view until they are mapped (dev and test only; production is a Go database).
- **Passwords.** Go's NIST-shaped policy is what production users meet. Rust's composition rules refused passwords
  Go accepts, including through the SDK create path hr and rfp use. Fixed in `2f731fbb`.
- **Service-account ids.** Go-created accounts have distinct account and principal ids; Rust treated the account id
  as the principal id, so a Go-created account's `/api/service-accounts/{id}` routes did not resolve. Fixed in
  `75e4b791`; both ids are accepted.
- **`client_credentials` with HTTP Basic** failed with "Missing client_id". Fixed in `c0dcf9b0`.
- **The client list's page parameter**: Go's `GET /api/clients` takes no parameters and returns every client; Rust
  does the same, so there is nothing to fix.

## What remains, and why

Twenty-five steps still differ. None has an owner decision behind it; each is Go behaving in a way Rust should not
copy, a library difference, or a knock-on of one.

| Steps | Difference | Why it stays |
|---|---|---|
| applications/sdk-sync: 11 sync steps | Go answers 500 `AUDIT_WRITE` ("rollup audit write failed"); Rust syncs | Go's own schema defect: its `aud_logs.entity_id` is too narrow for a sync rollup keyed by the application code (Java V18 / Rust 038 widened it). Nothing to match. |
| principals-core: `list-all`, `list-by-type`, `list-sorted-desc` | one extra (synced) principal in Rust's rows | Knock-on of the sync row above: Go's principal sync rolled back, Rust's did not. |
| anchor-domains: `update-into-existing-domain`; idp-role-mappings: `create-duplicate-idp-role-name` | Go 500 `PERSIST`; Rust 409 (`DOMAIN_EXISTS`, `MAPPING_EXISTS`) | Go lets the unique index fail the write. A 409 is the correct answer (the Java triage adopts "a unique-key race returns 409"); matching a 500 would be a regression. |
| email-domain-mappings: `get-updated` | Go clears `primaryClientId` on an update that omits it | Go replaces the pointer unconditionally, so editing a mapping's 2FA in the SPA erases a PARTNER mapping's client. Rust leaves an absent member alone and clears on an explicit `null` (what the SPA sends for ANCHOR). |
| service-accounts: `mint-token` (`claims/name`) | Go's token carries the account's name from before the update | Go's account update does not rename the SERVICE principal; Rust keeps the two in step. (The rest of that step, the function permissions in `scope`, is allow-listed.) |
| portal: `redeem-code-a`, `redeem-code-b2` | a portal access token's `tier` is `""` in Go, `CLIENT` in Rust | Go's synthetic principal has no scope. Matching needs the access-token claims to allow an empty tier, which the platform's own token validation (auth core, shared) refuses today; left for the auth owner. |
| webauthn: `register-begin` (×2), `authenticate-begin-*` (×3) | ceremony options: Go advertises ten algorithms, UV `preferred`, no extensions; Rust (webauthn-rs) two algorithms, UV `required`, credProps/credProtect | Library defaults. Advertising algorithms or a weaker UV requirement than webauthn-rs verifies would break registration or weaken the ceremony; the browsers accept both shapes. |

## Allow-listed (owner decisions)

| Scenario / step | Pointer | Decision |
|---|---|---|
| anchor-domains, clients, idp-role-mappings / `confinement-login` | `/permissions/**` | #21 (Java ruling 13): admin and iam-admin carry the service-account permissions |
| principals-core / `invite-confirm-session-authenticates-me` | `/scope` | Follow-up: `/auth/me` carries the caller's scope |
| service-accounts / `mint-token` | `/scope`, `/accessToken/«jwt»/claims/scope` | Direction: messaging-admin carries the function-runner permissions (Java is the reference for functions) |

## App impact

- **SPA (`frontend/`, Go's SPA):** every change moves a response to the shape Go's SPA was written against. Pages that
  were broken or degraded on Rust: the OAuth-client list (`applications` missing), the identity-provider pages
  (create/update answered `{id}`/204 where the SPA reads the provider; domains were not routed), the create-user form
  (check-email-domain's shape), the service-account detail (no `principalId` for its application-access panel), the
  applications page (website/logo lost).
- **Laravel SDK (integral, hr, rfp):** generated from Go's OpenAPI, so Go-shaped responses are what it models.
  Creating users with passwords now follows Go's policy; token requests with Basic credentials work.
- **Rust SDK (`crates/fc-sdk`):** `applications().list_roles()` now returns role names,
  `list_clients()` reads `items` (still accepts `clientConfigs`), `enable_for_client`/`disable_for_client` return
  unit. A source change for any caller of those three.
- **Service-account callers** that stored an account `id` from Rust keep working (the principal id is accepted).
  New Rust accounts get Go's `sac_` account id and `prn_` principal id.

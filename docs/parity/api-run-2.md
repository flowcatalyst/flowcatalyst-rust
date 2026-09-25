# API parity run 2: Go vs Rust

Date: 2026-09-25. Second run of the Go-vs-Rust API parity harness (`harness/parity`, owner decision #29), after
the cross-cutting root causes of run 1 (`docs/parity/api-run-1.md`, root causes 1, 2, 3 and 7, plus 8 and the
scheduled-job 415 of 9) were fixed on `feat/api-core`.

| | |
|---|---|
| Go | `flowcatalyst-go` @ `73a6918`, built with `go build -mod=readonly` (Go tree unchanged) |
| Rust | `feat/api-core` @ `0db6b94a` (`fc-server` release build) |
| Scenarios | 45 files / 1363 steps, unchanged since run 1 |
| Seed | Go `fcdev init`, first attempt |
| Allow-list | 35 entries: the 33 function-route entries of run 1, and 2 refresh-leeway steps (decision #21) |
| Normalisation | Java's rules 1–7, plus rule 0: a body's top-level `$schema` is dropped on both sides (decision #30, provisional) |
| Command | `cargo run -p fc-parity --release -- --go-bin-dir target/go-bin --rust-bin-dir target/release` |

## Totals

| status | run 1 | run 2 |
|---|---:|---:|
| OK | 89 | **587** |
| ACCEPTED (allow-listed) | 33 | 35 |
| DIFF | 870 | 534 |
| ERROR | 371 | 207 |

Coverage is unchanged (Go lockfile 252 / 256, outside-lockfile surface 131 / 135). The run still exits 1 on DIFFs,
ERRORs and false `covers` claims; there are no stale allow-list entries.

### Classification of the 741 non-OK steps

The same classes as run 1, assigned by script over `report.json` (a step goes to the first class that fits, in this
order):

| class | run 1 | run 2 | what is left |
|---|---:|---:|---|
| Go defect (Go 5xx or misses its own `expect`, Rust sensible) | 16 | 18 | the app-scoped sync `AUDIT_WRITE` 500s, duplicate IdP-role mapping, unmapped OIDC domain, two code-first sync steps, a portal OIDC 500 |
| Cascade (capture never produced / request under the wrong session) | 295 | 128 | almost all behind missing routes: portal (34+19+11+6), 2FA / change-password (24), email-domain-mapping create (11) |
| Route missing on Rust (404/405, empty body) | 197 | 146 | portal-apps / portal-users / portal / portal-assign (64), `/auth/2fa/*`, `/auth/change-password`, `/auth/login-history`, `/auth/password-setup/request` (26), `principals/{id}/reset-2fa`, `version`, `bulk-import`, dispatch-job `requeue`/`cancel`/`complete`/`list-raw`, docs, role permission paths, service-account token routes, router-config |
| Different HTTP status | 137 | 109 | read-permission gates, per-area write statuses and validation (see below) |
| Error envelope / code only | 127 | 71 | per-handler codes and messages only; no `code` member and no generic `NOT_FOUND`/`DUPLICATE`/`VALIDATION_ERROR` remain |
| `$schema` only | 133 | 0 | decision #30 |
| Response shape | 324 (+12 capture) | 269 | members missing, extra or `null` vs absent; OpenAPI documents (about 7300 diff entries between `/q/openapi` and the developer spec) |

## What changed, by item

1. **`POST /api/principals`** exists (Go's `createPrincipal`: 201 `{id}`, scope/client as given, Go's authorization
   and validation codes). Every scenario's confinement block now runs on Rust: 47 of the 48 scenario steps that call
   it pass (the other captures the invite link Rust does not mint), and the confinement assertions behind them are now real comparisons (many of the remaining 403-vs-200
   status diffs are these assertions finding missing read-permission gates, which another workstream owns).
   `POST /api/principals/users` is unchanged.
2. **Error envelope.** Platform errors are Go's `{error, message, details?}` with no `code` member. Not-found codes are
   respelt centrally to Go's `<Resource>_NOT_FOUND` / `<Resource> not found: <id>` from one table of Go's resource
   names (covering `PlatformError::NotFound`, the UPPER_SNAKE use-case codes and free-text `NOT_FOUND` messages); a
   duplicate is `<FIELD>_EXISTS`, a validation `VALIDATION`, an internal error `INTERNAL`. Codes Go spells verbatim
   are kept (`UseCaseError::not_found_verbatim`, e.g. the subscription sync's `CONNECTION_NOT_FOUND`). Function
   routes keep their contract (decision #5): a response mapper on their routers re-renders their errors with `code`.
3. **Extractor rejections** answer Go's 400 `VALIDATION` with huma's details (`expected required property <name> to
   be present` at `body` with the body as `value`, `invalid integer` at `query.<name>`); under `/oauth` an
   `invalid_request` with no-store. `POST /api/scheduled-jobs/{id}/fire` accepts an empty body and answers Go's
   `{id, scheduledJobId, instanceId}`, which un-cascades the scheduled-job instance steps. Of the 40 run-1 steps
   where Rust answered an axum `text/plain` rejection, 35 now match Go; the other 5 need per-handler work: a code
   (`INVALID_RATE_LIMIT`, `EMAIL_REQUIRED` for a missing query parameter), a message (`ARCHIVED`,
   `SCOPE_FORBIDDEN` on a fire), or an order (`complete` of a missing instance checks the instance before the
   body in Go: 404).
4. **Unauthenticated / forbidden.** No credential, or a stale session: 403 `UNAUTHENTICATED` "authentication
   required". A bearer that does not validate: 401 `invalid_token` with `WWW-Authenticate: Bearer
   error="invalid_token"`. The session endpoints (`/auth/login`, `/auth/refresh`, a signed-out `/auth/me`) answer Go's
   401 `{code: UNAUTHENTICATED, message}` with `WWW-Authenticate: Cookie realm="fc_session"`. The session cookie
   carries `Expires`; logout clears it with `Secure; SameSite` and needs no session. OAuth errors carry
   `Cache-Control: no-store` / `Pragma: no-cache`. `GET /api/platform/cors/allowed` is public, as in Go.
5. **`$schema`**: not emitted by Rust; decision #30 (provisional) records it, and harness rule 0 drops it.
6. **Auth and tokens.**
   - `/oauth/authorize` with an unknown or inactive client: 400 `unauthorized_client`, never a redirect.
   - `client_credentials` for a client without a service account: 400 `unauthorized_client`.
   - Discovery advertises only `code`.
   - Login backoff works: the client IP falls back to the connection's peer address (Go's `RemoteAddr` fallback), so
     `backoff-5`…`backoff-7` now answer 429 with `Retry-After`, and login attempts record `127.0.0.1`.
   - Refresh rejections are 400 `invalid_grant` (was 401). The two leeway steps are allow-listed (below).
   - Token shapes: an interactive login's access token (authorization_code and its refresh) is Go's identity-only
     token unless the OAuth client is flagged `api_access` (Go's column, added by Rust migration `043` where
     missing). Introspection and userinfo now match. The session cookie was already Go's subject-only shape.
     `client_credentials` tokens were already Go's `token_use: api` shape. The function host (`bearer.rs`) refuses
     identity tokens, which is intended; the platform's own bearer auth refuses them with 401 `invalid_token`, as Go.
   - `/auth/oidc/login`: `DOMAIN_REQUIRED`, the legacy `email` parameter, `OIDC_NOT_CONFIGURED`.
7. **`/auth/login`** answers `{status: "ok", principalId, name, email, roles, permissions, clientId, ssoManaged}`
   and refuses an OIDC domain with 403 `SSO_REQUIRED`. **`/api/me`** answers Go's whoami with `permissions`,
   `accessibleClientIds` and `allApplications`, taking the reach from the credential (a bearer's claims, a session's
   reloaded row). `/auth/me`'s body was left to the go-authz workstream; only its signed-out answer changed.
8. **Passkeys** are refused only for domains mapped to an OIDC provider; the anchor domain `fcdev init` maps to an
   INTERNAL provider keeps them. The completion routes read the credential after the ceremony, with Go's codes.

## Deliberate differences

| step(s) | difference | ruling |
|---|---|---|
| `auth/oauth-code-flow` `refresh-reuse-rotated-token`, `refresh-after-family-revoked` | Go revokes the family on the immediate replay (400); Rust rotates into a sibling within 10 s (200), so the next step differs too | #21 (Java ruling 5), allow-listed |
| every JSON body | no top-level `$schema` | #30 (provisional), normalised |
| function routes | Java's contract, `code` beside `error` | Direction + #5, allow-listed as in run 1 |

## Remaining root causes, by class (owners are the other workstreams unless noted)

1. **Routes Rust lacks** (146 direct, most of the 128 cascades): portal (apps, users, assign, `/portal/*`), 2FA and
   the MFA login gate, change-password, login-history, password-setup (and with it the invite link that
   `returnInviteLink` asks for), `principals/{id}/version`, `reset-2fa`, `bulk-import`, dispatch-job actions,
   `list-raw`, docs and docs sync, role permission paths, service-account token routes, router-config,
   `applications/{id}/service-account`, `clients/{clientId}` config, email-domain-mapping create/lookup, the
   `openapi.json|yaml` aliases.
2. **Read-permission and scope gates** (a large part of the 109 status diffs): Go answers 403
   `PERMISSION_REQUIRED` / `ANCHOR_REQUIRED` / `NO_PLATFORM_ROLE` where Rust answers 200, and 404 where Rust answers
   403 for an out-of-scope resource (Go's PR-3(b)). Rust's check helpers still answer `FORBIDDEN "Anchor access
   required"` where Go names the permission.
3. **Write statuses and per-area validation**: creates answering 200 instead of 201, 200 instead of 204 (pool
   archive/activate, subscription pause/resume, client enable/disable), idempotent repeats answering 409, and
   validation Rust does not do (code formats, domain syntax, OIDC issuer/client id, concurrency, endpoints).
4. **Per-handler codes and messages** (71): Go's specific codes where Rust has another (`NAME_REQUIRED` vs
   `INVALID_NAME`, `ALREADY_PROVISIONED`, `REDIRECT_URIS_REQUIRED`, `CURSOR`, `INVALID_CONFIG_TYPE`,
   `DOMAIN_ALREADY_CONFIGURED`, `CODE_EXISTS` vs `<ENTITY>_CODE_EXISTS`, `CODE_ROLE_IMMUTABLE`, `SCOPE_FORBIDDEN`,
   …) and message wording.
5. **Response shapes** (269): `hasDeveloperCredential`, `isAnchorUser`, `apiAccess` and `applications` on OAuth
   clients, `null` versus absent members, list envelopes, `/auth/me`'s body, `passwordSetupRequired` on
   check-domain (Rust stores a random password for a passwordless create, Go stores none), the OpenAPI documents.
6. **Go defects** (18): unchanged from run 1 plus two code-first sync steps and one portal OIDC step; still not
   allow-listed.

## Harness notes

- The allow-list's `scenario` matches the scenario's name (not its file), so the leeway entries name
  `S2 auth: OAuth authorization code*`.
- No harness problem was found. Rule 0 drops only the top-level `$schema`; the `$schema` properties inside Go's
  OpenAPI component schemas are still compared.

## Per-group counts

| group | files | steps | OK | ACCEPTED | DIFF | ERROR |
|---|---:|---:|---:|---:|---:|---:|
| anchor-domains | 1 | 23 | 9 | 0 | 4 | 10 |
| applications | 2 | 72 | 29 | 0 | 40 | 3 |
| audit-logs | 1 | 34 | 17 | 0 | 17 | 0 |
| auth | 3 | 99 | 57 | 2 | 20 | 20 |
| auth-configs | 1 | 26 | 9 | 0 | 9 | 8 |
| auth-remainder | 1 | 36 | 17 | 0 | 5 | 14 |
| authz | 1 | 15 | 7 | 0 | 8 | 0 |
| bff | 1 | 66 | 34 | 0 | 30 | 2 |
| clients | 1 | 43 | 24 | 0 | 19 | 0 |
| code-first-connections | 1 | 10 | 4 | 0 | 3 | 3 |
| config | 1 | 24 | 10 | 0 | 11 | 3 |
| connections | 1 | 34 | 14 | 0 | 19 | 1 |
| dispatch-jobs | 1 | 28 | 10 | 0 | 17 | 1 |
| dispatch-pools | 1 | 36 | 17 | 0 | 19 | 0 |
| docs | 1 | 21 | 9 | 0 | 11 | 1 |
| email-domain-mappings | 1 | 32 | 5 | 0 | 12 | 15 |
| event-types | 1 | 30 | 15 | 0 | 14 | 1 |
| events | 1 | 17 | 8 | 0 | 7 | 2 |
| functions | 1 | 41 | 5 | 33 | 3 | 0 |
| identity-providers | 1 | 28 | 13 | 0 | 10 | 5 |
| idp-role-mappings | 1 | 15 | 9 | 0 | 3 | 3 |
| login-attempts | 1 | 17 | 8 | 0 | 9 | 0 |
| me-public-config | 1 | 25 | 14 | 0 | 11 | 0 |
| oauth-clients | 1 | 32 | 19 | 0 | 13 | 0 |
| platform | 2 | 48 | 35 | 0 | 13 | 0 |
| platform-config | 1 | 10 | 5 | 0 | 3 | 2 |
| portal | 1 | 47 | 2 | 0 | 5 | 40 |
| portal-apps | 1 | 44 | 6 | 0 | 16 | 22 |
| portal-assign | 1 | 19 | 2 | 0 | 0 | 17 |
| portal-users | 1 | 48 | 6 | 0 | 22 | 20 |
| principals | 2 | 81 | 37 | 0 | 35 | 9 |
| processes | 1 | 31 | 25 | 0 | 6 | 0 |
| reset-approvals | 1 | 15 | 6 | 0 | 9 | 0 |
| roles | 1 | 46 | 17 | 0 | 29 | 0 |
| router-config | 1 | 13 | 6 | 0 | 7 | 0 |
| scheduled-jobs | 1 | 52 | 25 | 0 | 27 | 0 |
| service-accounts | 1 | 32 | 11 | 0 | 16 | 5 |
| smoke | 1 | 18 | 12 | 0 | 6 | 0 |
| subscriptions | 1 | 35 | 17 | 0 | 18 | 0 |
| webauthn | 1 | 20 | 12 | 0 | 8 | 0 |
| **total** | **45** | **1363** | **587** | **35** | **534** | **207** |

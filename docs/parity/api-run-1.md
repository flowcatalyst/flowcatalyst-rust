# API parity run 1: Go vs Rust

Date: 2026-09-25. First run of the Go-vs-Rust API parity harness (`harness/parity`, owner decision #29).

| | |
|---|---|
| Go | `flowcatalyst-go` @ `73a6918`, built with `go build -mod=readonly` (Go tree unchanged) |
| Rust | this branch; platform code as of `4d39bf78` (`fc-server` release build) |
| Scenarios | 45 files / 1363 steps from `flowcatalyst-javalin` @ `65988b51` |
| Seed | Go `fcdev init`, first attempt (Go HEAD no longer has the `schema_type = 'JSON'` defect; the trigger workaround stayed dormant) |
| Allow-list | 33 entries, all for the function routes (Direction: Go lacks the function runner; Java is the reference) |
| Command | `cargo run -p fc-parity --release -- --go-bin-dir target/go-bin --rust-bin-dir target/release` |
| Wall clock | about 2 minutes with prebuilt binaries (Postgres container, seed, both boots, 2 × 1363 requests) |

Rust booted on a database Go created without trouble. Its migration runner saw a pre-tracker schema,
backfilled 33 migrations, and applied `034_functions`, `036` (cron dialect: 0 rows rewritten), `037` and `038`. Its
startup seeders then ran. The Go-to-Rust handover works at the schema level. What follows are differences
in behaviour.

## Totals

| status | steps |
|---|---:|
| OK | 89 |
| ACCEPTED (allow-listed) | 33 |
| DIFF | 870 |
| ERROR (a step's `expect.status` or capture failed on a side) | 371 (368 Rust only, 3 Go) |

Coverage: Go lockfile 252 / 256 operations hit (missing `POST /api/dispatch-jobs/{id}/sign` and the three
`platform-config/access` routes), and 131 / 135 of the outside-lockfile surface (missing
`DELETE /api/functions/{address}/aliases/{alias}`, `GET /api/function-domains/{hostname}`,
`GET /api/function-policies`, `GET /api/openapi-functions.json`). The run exits 1: there are DIFFs, ERRORs, and false
`covers` claims. Every false claim is a route the Rust side never reached because an earlier step failed.
Most of them are `assignPrincipalRoles`.

### Classification

Every non-OK step was classified. A script did the first pass, then the result was checked by hand against the
raw records in `report.json`.

| class | steps | verdict |
|---|---:|---|
| Cascade: a capture Rust never produced (`undefined substitution`), a login as a principal Rust never created, or a request that therefore ran under the admin's session (the "403 → 200" confinement steps) | 295 | likely-Rust-bug (downstream of the root causes below; re-judge after they are fixed) |
| Route missing on Rust (404/405 with an empty body) | 197 | likely-Rust-bug |
| Response shape: fields missing, extra, renamed or `null` vs absent; lists, facets, OpenAPI documents | 324 | likely-Rust-bug |
| Only `$schema` differs | 133 | likely-Rust-bug (no Rust ruling; Java left it as an owner question) |
| Error envelope only (`error`/`code`/`message`) | 127 | likely-Rust-bug |
| Different HTTP status (DIFF) or Rust missing an `expect.status` (ERROR) | 137 | likely-Rust-bug |
| Rust response lacks a member a later step captures | 12 | likely-Rust-bug |
| Go defect (Go 500s or misses its own `expect`; Rust answers sensibly) | 16 | Go defect, not a Rust bug |
| Function routes (Go serves its SPA for every `/api/function*`) | 33 | ruling-covered (ACCEPTED) |
| Harness problem producing a false diff | 0 | none found; see "Harness notes" |

## Root causes, by reach

1. **`POST /api/principals` does not exist on Rust.** Rust has `POST /api/principals/users`. This produces 48 direct
   405s and most of the 295 cascaded steps. Every scenario's tenant-confinement block creates a second principal
   first, so on Rust no confinement behaviour was actually tested. Fixing this route is the prerequisite for a
   meaningful run 2.
2. **`$schema` is missing on every Rust JSON body** (733 diff entries; 133 steps differ in nothing else). Go's huma
   emits `"$schema": "<base>/<Model>.json"`. No owner decision covers dropping it.
3. **Error envelope and codes.** Go sends `{$schema, error, message}` with codes like `Application_NOT_FOUND`,
   `CODE_EXISTS`, `NAME_REQUIRED`. Rust sends `{error, code, message}` with UPPER_SNAKE codes that often differ
   (`APPLICATION_NOT_FOUND`, `NOT_FOUND`, `DUPLICATE`, `VALIDATION_ERROR`), and different message text. When the
   body extractor rejects a request (missing required field, bad query type), Rust answers **422 `text/plain`**
   where Go answers **400 JSON `VALIDATION`**: 26 steps plus several ERRORs.
4. **Routes Rust lacks** (197 steps): the whole `/auth/2fa/*` surface; `/auth/change-password`,
   `/auth/login-history`, `/auth/password-setup/request`; `/api/portal-apps`, `/api/portal-users`, `/portal/authorize`,
   `/portal/auth/*`; `/api/docs*` and `…/docs/sync`; dispatch-job `requeue`/`cancel`/`complete`/`event/{id}`/`list-raw`;
   `events/list-raw`; role permission grant/revoke by path (`/api/roles/{name}/permissions[/{perm}]`,
   `POST /bff/roles/permissions`); service-account `token`/`regenerate-*`/`deactivate`; `reset-approvals`;
   `clients/search`; the config-property routes (`GET|PUT|DELETE /api/config/{app}/{section}/{prop}` answer 404);
   email-domain-mapping create/lookup/by-domain; `event-types/{id}/schemas`; `processes/sync`;
   `connections/sync`; `dispatch/router-config`; `principals/bulk-import`; `principals/{id}/reset-2fa`;
   `applications/{id}/service-account`; `applications/{id}/clients/{clientId}`; `openapi.json|yaml`; `PUT /bff/event-types/{id}`.
   The owner-decisions follow-ups already list several of these (connections/sync, docs/sync, processes/sync, router-config).
5. **Status codes on writes.** Creates answer 200 where Go answers 201 (anchor-domains, auth-configs,
   idp-role-mappings, provision-login-client). Pool archive/activate, subscription pause/resume and
   application-client enable/disable answer 200 with a body where Go answers 204; IdP update answers 204 where Go
   answers 200. Repeats that Go treats as no-ops (pool archive again, subscription resume again, process archive
   again, revoke of an absent permission) answer 409 or 400 on Rust.
6. **Validation Rust does not do.** Rust accepts what Go rejects with 400/409: application, pool and scheduled-job
   code format; anchor-domain and auth-config domain syntax; OIDC auth-config without issuer or client id; IdP OIDC
   without issuer; pool concurrency; subscription endpoint; service-account webhook auth type; oauth-client
   portal/api-access conflict; an event without `data`; a duplicate application code; a duplicate event-type
   version; deleting an IdP that is still mapped (Go 409, Rust 204).
7. **Auth and tokens.**
   - Login backoff is missing: after four failures Go answers 429 with `Retry-After`, Rust keeps answering 401.
   - Refresh-token reuse is not detected: replaying a rotated refresh token, or refreshing after the family was
     revoked, succeeds on Rust (Go: 400 `invalid_grant`). This is a security gap.
   - `/oauth/authorize` with an unknown client redirects to the supplied `redirect_uri` with an error (307). Go
     answers 400 JSON; RFC 6749 §4.1.2.1 forbids redirecting to an unverified URI.
   - `client_credentials` for a client with no service account: Rust 500 `server_error`, Go 400 `unauthorized_client`.
   - The access token's shape differs from Go's, against decisions #3/#20: `token_use` is `api` (Go: `identity`) and
     it carries `scope`/`roles`/`clients`/`applications` claims Go's does not. `all_applications` also differs.
     Introspection and userinfo echo the extra `scope`.
   - Response shapes differ for `/auth/login` (Go `{status:"ok", permissions, clientId, ssoManaged, …}`), `/auth/me`
     and `/api/me` (`permissions`, `allApplications`, `accessibleClientIds`), and the discovery document (Rust lists
     implicit/hybrid `response_types`).
   - Unauthenticated calls answer 401 `UNAUTHORIZED` on Rust, 403 `UNAUTHENTICATED` on Go. Go's 401s carry
     `WWW-Authenticate: Cookie realm="fc_session"`, Rust's do not.
   - The session cookie has no `Expires` on Rust, and Rust's logout clear-cookie drops `Secure; SameSite`.
   - Several OAuth error responses lack `Cache-Control: no-store`.
   - The MFA gate on `/auth/login` could not be tested: TOTP enrolment routes are missing (item 4), so no
     `mfaToken` was ever issued, and every later MFA step is a cascade.
8. **Passkeys refused for the anchor admin.** Go's `fcdev init` creates an INTERNAL identity provider and an ANCHOR
   email-domain mapping for the admin's domain. Rust's passkey gate (`webauthn/gate.rs`) treats *any* mapping as
   federated and answers 400 "passkeys are not available for this domain". After cutover, every Go-initialised
   installation loses passkeys for its domain. Go only gates on OIDC providers.
9. **Scheduled jobs:** `POST /api/scheduled-jobs/{id}/fire` with no body answers 415 (Rust requires a JSON
   `Content-Type`; Go accepts an empty body), which cascades through the instance steps. For a missing instance,
   `complete` answers 422 (Go 404) and `logs` answers 404 (Go 200 `[]`).
10. **Reads.**
    - Login attempts record `ipAddress = null` and `failureReason` as a code (`INVALID_CREDENTIALS`) where Go stores
      `127.0.0.1` and the message.
    - `GET /api/events/{id}` returns an event straight after ingest (Rust reads the write table). Go answers 404
      until the projector has run.
    - Audit-log facets list command names differently (`CreateApplicationCommand` vs `CreateCommand`).
    - List envelopes differ (`{items,total}` vs `{domains|configs|clientConfigs,total}`).
    - Rust emits `null` members where Go omits them (and the reverse for `website`, `hasLoginClient`, `notes`, …).
    - The BFF dashboard counts differ: `activeUsers` 9 vs 1 is a cascade; `rolesDefined` 16 vs 18 is unexplained
      (it may be the built-in role catalogue, decision #12, or rows only one side accepted earlier in the run).
11. **OpenAPI documents** (`/q/openapi`, the developer platform spec) differ wholesale: about 2400 diff entries each.
    Rust serves its own utoipa document, Go its huma one.
12. **Go defects seen on the way** (Rust answers sensibly): Go's app-scoped sync routes 500 `AUDIT_WRITE`
    (13 steps), a duplicate IdP-role mapping 500 `PERSIST`, and an OIDC login for an unmapped domain 500. Go also
    404s one connection-code subscription sync. None of these is allow-listed. Rust's behaviour is not the
    difference to fix. When the rest converges, record them with `!go-expect` entries under a ruling (Java has
    owner ruling 2026-09-25 "allow-list until cutover"; the Rust decisions file does not yet say this).

## Harness notes

- **No false diffs traced to the harness.** Go met every `expect.status` except its three known sync-route
  defects, so the scenarios, seed, cookie jar, PKCE, TOTP and substitution machinery work against Go as they
  did in Java. Both sides minted tokens under the same `kid`, so the key plumbing is right.
- **Inherited reporting quirk:** the pointer `/status` names both the HTTP status and a body member called
  `status` (Java has the same ambiguity). The "(`ok`, «absent»)" rows under `/status` are the login body's
  `status` member, not the HTTP status.
- **Inherited masking:** the auto-capture pattern (`.*Id`) captures `operationId` values in OpenAPI documents
  as `«auto:operationId»`. This is harmless, but it is why those diffs show labels.
- **Cross-scenario state is real state:** scenarios share each side's database across the run, so a Rust write
  that Go refused (for example an accepted invalid domain) shows up in later lists. Where a list differs only by
  extra or missing rows, check the earlier scenario first.
- The classifier's "cascade" bucket is conservative. Some "response shape" steps after a failed Rust login (for
  example `dashboard-stats` `activeUsers` 9 vs 1) are cascades too.

## Per-group counts

| group | files | steps | OK | ACCEPTED | DIFF | ERROR |
|---|---:|---:|---:|---:|---:|---:|
| anchor-domains | 1 | 23 | 0 | 0 | 10 | 13 |
| applications | 2 | 72 | 2 | 0 | 64 | 6 |
| audit-logs | 1 | 34 | 7 | 0 | 22 | 5 |
| auth-configs | 1 | 26 | 0 | 0 | 15 | 11 |
| auth-remainder | 1 | 36 | 3 | 0 | 18 | 15 |
| auth | 3 | 99 | 14 | 0 | 61 | 24 |
| authz | 1 | 15 | 1 | 0 | 10 | 4 |
| bff | 1 | 66 | 16 | 0 | 44 | 6 |
| clients | 1 | 43 | 5 | 0 | 35 | 3 |
| code-first-connections | 1 | 10 | 0 | 0 | 7 | 3 |
| config | 1 | 24 | 0 | 0 | 18 | 6 |
| connections | 1 | 34 | 2 | 0 | 29 | 3 |
| dispatch-jobs | 1 | 28 | 5 | 0 | 19 | 4 |
| dispatch-pools | 1 | 36 | 2 | 0 | 31 | 3 |
| docs | 1 | 21 | 0 | 0 | 15 | 6 |
| email-domain-mappings | 1 | 32 | 0 | 0 | 14 | 18 |
| event-types | 1 | 30 | 0 | 0 | 26 | 4 |
| events | 1 | 17 | 2 | 0 | 13 | 2 |
| functions | 1 | 41 | 0 | 33 | 8 | 0 |
| identity-providers | 1 | 28 | 2 | 0 | 18 | 8 |
| idp-role-mappings | 1 | 15 | 0 | 0 | 9 | 6 |
| login-attempts | 1 | 17 | 0 | 0 | 15 | 2 |
| me-public-config | 1 | 25 | 3 | 0 | 19 | 3 |
| oauth-clients | 1 | 32 | 3 | 0 | 27 | 2 |
| platform-config | 1 | 10 | 0 | 0 | 5 | 5 |
| platform | 2 | 48 | 2 | 0 | 39 | 7 |
| portal-apps | 1 | 44 | 0 | 0 | 19 | 25 |
| portal-assign | 1 | 19 | 0 | 0 | 2 | 17 |
| portal-users | 1 | 48 | 0 | 0 | 25 | 23 |
| portal | 1 | 47 | 0 | 0 | 7 | 40 |
| principals | 2 | 81 | 0 | 0 | 29 | 52 |
| processes | 1 | 31 | 4 | 0 | 24 | 3 |
| reset-approvals | 1 | 15 | 0 | 0 | 11 | 4 |
| roles | 1 | 46 | 4 | 0 | 39 | 3 |
| router-config | 1 | 13 | 1 | 0 | 12 | 0 |
| scheduled-jobs | 1 | 52 | 5 | 0 | 31 | 16 |
| service-accounts | 1 | 32 | 2 | 0 | 23 | 7 |
| smoke | 1 | 18 | 2 | 0 | 13 | 3 |
| subscriptions | 1 | 35 | 2 | 0 | 30 | 3 |
| webauthn | 1 | 20 | 0 | 0 | 14 | 6 |
| **total** | **45** | **1363** | **89** | **33** | **870** | **371** |

## Diffs by area

Every non-OK step, grouped by scenario file. Each line is labelled with its class: `likely-Rust-bug`,
`likely-Rust-bug (cascade)`, `Go defect` or `ruling-covered`. Pointers and values are normalised (`«name»` is a
capture, `«absent»` means missing on that side); values are cut to about 30 characters. The full diffs and raw
records are in the run's `report.md` / `report.json`, which are not committed. Re-run the harness to get them.

### anchor-domains

**`anchor-domains/crud.json`** (false `covers`: deleteAnchorDomain, assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `list-contains-normalised-domain`: `/domains` «absent» → [{"id":"«auto:id»","domain":…; `/items` [{"id":"«domainId»","domain"… → «absent»; `/total` «absent» → 1
- [likely-Rust-bug] `list-after-update`: `/domains` «absent» → [{"id":"«auto:id»","domain":…; `/items` [{"id":"«domainId»","domain"… → «absent»; `/total` «absent» → 1
- [likely-Rust-bug] `create` `POST /api/anchor-domains`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [likely-Rust-bug] `create-for-duplicate` `POST /api/anchor-domains`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [likely-Rust-bug] `create-collision-target` `POST /api/anchor-domains`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [likely-Rust-bug] `create-doomed` `POST /api/anchor-domains`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (10): `update`, `update-invalid-domain`, `update-into-existing-domain`, `delete-doomed`, `delete-doomed-again`, `confinement-assign-role`, `confinement-login`, `confinement-list-forbidden`, `confinement-create-forbidden`, `confinement-delete-forbidden`
- [likely-Rust-bug] `create-missing-domain` `POST /api/anchor-domains`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-invalid-domain` `POST /api/anchor-domains`: Go 400 `error=INVALID_DOMAIN` / Rust 200
- [likely-Rust-bug] `duplicate-domain` error body: Go `error=DOMAIN_EXISTS` / Rust `error=DOMAIN_EXISTS code=DOMAIN_EXISTS` (message text differs too)
- [likely-Rust-bug] `update-unknown` error body: Go `error=AnchorDomain_NOT_FOUND` / Rust `error=ANCHOR_DOMAIN_NOT_FOUND code=ANCHOR_DOMAIN_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `confinement-create-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`

### applications

**`applications/crud.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-by-id`: `/defaultBaseUrl` «absent» → null; `/iconUrl` «absent» → null; `/serviceAccountId` «absent» → null (+1 more)
- [likely-Rust-bug] `get-by-code`: `/defaultBaseUrl` «absent» → null; `/hasLoginClient` false → «absent»; `/iconUrl` «absent» → null (+2 more)
- [likely-Rust-bug] `list-by-type`: `/applications/0/defaultBaseUrl` «absent» → null; `/applications/0/hasLoginClient` false → «absent»; `/applications/0/iconUrl` «absent» → null (+2 more)
- [likely-Rust-bug] `list-active-true`: `/applications/0/defaultBaseUrl` «absent» → null; `/applications/0/hasLoginClient` false → «absent»; `/applications/0/iconUrl` «absent» → null (+10 more)
- [likely-Rust-bug] `get-after-update`: `/defaultBaseUrl` «absent» → null; `/serviceAccountId` «absent» → null; `/website` https://example.test/app → «absent»
- [likely-Rust-bug] `deactivate`: `/defaultBaseUrl` «absent» → null; `/hasLoginClient` false → «absent»; `/serviceAccountId` «absent» → null (+1 more)
- [likely-Rust-bug] `list-active-false-after-deactivate`: `/applications/0/defaultBaseUrl` «absent» → null; `/applications/0/hasLoginClient` false → «absent»; `/applications/0/serviceAccountId` «absent» → null (+1 more)
- [likely-Rust-bug] `list-active-true-after-deactivate`: `/applications/0/defaultBaseUrl` «absent» → null; `/applications/0/description` «absent» → null; `/applications/0/hasLoginClient` false → «absent» (+5 more)
- [likely-Rust-bug] `activate`: `/defaultBaseUrl` «absent» → null; `/hasLoginClient` false → «absent»; `/serviceAccountId` «absent» → null (+1 more)
- [likely-Rust-bug] `roles-listing-empty`: `/` {"$schema":"«base»/Applicati… → []
- [likely-Rust-bug] `clients-empty`: `/clientConfigs` «absent» → []; `/items` [] → «absent»; `/total` «absent» → 0
- [likely-Rust-bug] `clients-after-disable`: `/clientConfigs` «absent» → [{"id":"«auto:id»","applicat…; `/items` [{"id":"«clientConfigId»","a… → «absent»; `/total` «absent» → 1
- [likely-Rust-bug] `get-after-provision`: `/defaultBaseUrl` «absent» → null; `/website` https://example.test/app → «absent»
- [likely-Rust-bug] `get-after-login-client`: `/defaultBaseUrl` «absent» → null; `/website` https://example.test/app → «absent»
- [likely-Rust-bug] `confinement-read-succeeds`: `/defaultBaseUrl` «absent» → null; `/website` https://example.test/app → «absent»
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (6): `create`, `create-config-client`, `provision-service-account`, `create-for-duplicate`, `create-doomed`, `confinement-create-client`
- [likely-Rust-bug] `update-blank-name` error body: Go `error=NAME_REQUIRED` / Rust `error=INVALID_NAME code=INVALID_NAME` (message text differs too)
- [likely-Rust-bug] `activate-unknown` error body: Go `error=Application_NOT_FOUND` / Rust `error=APPLICATION_NOT_FOUND code=APPLICATION_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `enable-unknown-client` error body: Go `error=Client_NOT_FOUND` / Rust `error=CLIENT_NOT_FOUND code=CLIENT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `provision-service-account-again` error body: Go `error=ALREADY_PROVISIONED` / Rust `error=DUPLICATE code=DUPLICATE` (message text differs too)
- [likely-Rust-bug] `provision-login-client-validation` error body: Go `error=REDIRECT_URIS_REQUIRED` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] `provision-login-client-unknown-app` error body: Go `error=Application_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-doomed-after-delete` error body: Go `error=Application_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `delete-doomed-again` error body: Go `error=Application_NOT_FOUND` / Rust `error=APPLICATION_NOT_FOUND code=APPLICATION_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] Rust has no `GET /api/applications/{x}/clients/{x}` (answers 405): `client-config-missing`
- [likely-Rust-bug] Rust has no `POST /api/applications/{x}/service-account` (answers 405): `attach-service-account-before-provision`, `confinement-anchor-only-forbidden`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug] `enable-for-client` `POST /api/applications/{x}/clients/{x}/enable`: Go 204 / Rust 200
- [likely-Rust-bug] `disable-for-client` `POST /api/applications/{x}/clients/{x}/disable`: Go 204 / Rust 200
- [likely-Rust-bug] `clients-of-unknown-application` `GET /api/applications/app_doesnotexist1/clients`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `create-invalid-code` `POST /api/applications`: Go 400 `error=INVALID_CODE_FORMAT` / Rust 201
- [likely-Rust-bug] `create-missing-name` `POST /api/applications`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-duplicate-code` `POST /api/applications`: Go 409 `error=CODE_EXISTS` / Rust 201
- [likely-Rust-bug] `client-config-after-enable` `GET /api/applications/{x}/clients/{x}`: Go 200 / Rust (no record) — rust: capture 'clientConfigId': pointer /id not present in the response body
- [likely-Rust-bug] `provision-login-client-public` `POST /api/applications/{x}/provision-login-client`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [likely-Rust-bug] `provision-login-client-confidential` `POST /api/applications/{x}/provision-login-client`: Go 201 / Rust 409 `error=DUPLICATE code=DUPLICATE` — rust: expected status 201 but got 409
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (3): `confinement-assign-readonly-role`, `confinement-login`, `confinement-write-forbidden`

**`applications/sdk-sync.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (8): `create-app`, `sync-roles-create`, `sync-roles-update`, `sync-scheduled-jobs-create`, `sync-scheduled-jobs-update`, `sync-openapi-create`, `sync-openapi-unchanged`, `sync-openapi-new-version`
- [Go defect] `sync-event-types-create` `POST /api/applications/parity-sync-{x}/event-types/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-event-types-update` `POST /api/applications/parity-sync-{x}/event-types/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-event-types-remove-unlisted` `POST /api/applications/parity-sync-{x}/event-types/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-dispatch-pools-create` `POST /api/applications/parity-sync-{x}/dispatch-pools/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-dispatch-pools-update` `POST /api/applications/parity-sync-{x}/dispatch-pools/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-subscriptions-create` `POST /api/applications/parity-sync-{x}/subscriptions/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-subscriptions-update` `POST /api/applications/parity-sync-{x}/subscriptions/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-principals-create` `POST /api/applications/parity-sync-{x}/principals/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-principals-update` `POST /api/applications/parity-sync-{x}/principals/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-processes-create` `POST /api/applications/parity-sync-{x}/processes/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [Go defect] `sync-processes-update` `POST /api/applications/parity-sync-{x}/processes/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [likely-Rust-bug] Rust has no `POST /api/applications/parity-sync-{x}/docs/sync` (answers 404): `sync-docs-create`, `sync-docs-replace`
- [likely-Rust-bug] `sync-invalid-openapi` error body: Go `error=INVALID_OPENAPI_SPEC` / Rust `error=INVALID_OPENAPI_SPEC code=INVALID_OPENAPI_SPEC` (message text differs too)
- [likely-Rust-bug] `sync-unknown-application` error body: Go `error=Application_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)

### audit-logs

**`audit-logs/audit-logs.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-by-id`: `/applicationId` «absent» → null; `/clientId` «absent» → null; `/operationJson` «absent» → null
- [likely-Rust-bug] `list-filtered-by-entity-type-and-id`: `/auditLogs/0/applicationId` «absent» → null; `/auditLogs/0/clientId` «absent» → null
- [likely-Rust-bug] `list-filtered-by-operation`: `/auditLogs/0/applicationId` «absent» → null; `/auditLogs/0/clientId` «absent» → null
- [likely-Rust-bug] `recent-alias-same-filter`: `/` {"$schema":"«base»/AuditLogL… → [{"id":"«pageThreeId»","oper…
- [likely-Rust-bug] `by-entity`: `/auditLogs/0/applicationId` «absent» → null; `/auditLogs/0/clientId` «absent» → null; `/entityId` «absent» → «auto:entityId» (+3 more)
- [likely-Rust-bug] `by-entity-unknown-is-empty-not-404`: `/entityId` «absent» → «auto:entityId»; `/entityType` «absent» → S1cAuditEntityf0f6bce23a38; `/hasMore` false → «absent» (+1 more)
- [likely-Rust-bug] `by-principal`: `/` {"$schema":"«base»/AuditLogL… → [{"id":"«pageThreeId»","oper…
- [likely-Rust-bug] `entity-types-facet-contains-our-entity-type`: `/entityTypes/5` Principal → S1cAuditEntityf0f6bce23a38; `/entityTypes/6` Role → s1c-audit-page-f0f6bce23a38; `/entityTypes/7` Roles → Scheduledjobs (+5 more)
- [likely-Rust-bug] `operations-facet-contains-create-and-update`: `/operations/0` ActivateCommand → ActivateApplicationCommand; `/operations/1` AssignRolesCommand → AttachServiceAccountToApplic…; `/operations/4` CreateCommand → CreateApplicationCommand (+17 more)
- [likely-Rust-bug] `page-1-of-2`: `/auditLogs/0/applicationId` «absent» → null; `/auditLogs/0/clientId` «absent» → null; `/auditLogs/1/applicationId` «absent» → null (+1 more)
- [likely-Rust-bug] `page-2-with-after-cursor`: `/auditLogs/0/applicationId` «absent» → null; `/auditLogs/0/clientId` «absent» → null
- [likely-Rust-bug] `page-size-non-integer-is-validation-error`: `/headers/Content-Type` application/json → text/plain; charset=utf-8; `/` {"$schema":"«base»/ErrorMode… → Failed to deserialize query …
- [likely-Rust-bug] `login-as-anchor-to-grant-role`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `b-with-viewer-role-can-now-read-anchors-audit-log`: `/applicationId` «absent» → null; `/clientId` «absent» → null; `/operationJson` «absent» → null
- [likely-Rust-bug] `get-by-id-missing` error body: Go `error=AuditLog_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `malformed-cursor-is-400-cursor` error body: Go `error=CURSOR` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (3): `application-ids-facet`, `client-ids-facet`, `create-second-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (6): `assign-role-without-audit-log-view`, `login-as-b`, `b-without-audit-log-view-is-permission-required`, `b-cannot-get-by-id-either`, `grant-audit-log-view-via-viewer-role`, `login-as-b-again`

### auth-configs

**`auth-configs/crud.json`** (false `covers`: deleteAuthConfig, assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `list-contains-created`: `/configs` «absent» → [{"id":"«auto:id»","emailDom…; `/items` [{"id":"«configId»","emailDo… → «absent»; `/total` «absent» → 1
- [likely-Rust-bug] `list-after-oidc-update`: `/configs` «absent» → [{"id":"«auto:id»","emailDom…; `/items` [{"id":"«configId»","emailDo… → «absent»; `/total` «absent» → 1
- [likely-Rust-bug] `get-list-after-untouched-update`: `/configs` «absent» → [{"id":"«auto:id»","emailDom…; `/items` [{"id":"«configId»","emailDo… → «absent»; `/total` «absent» → 1
- [likely-Rust-bug] `create-internal` `POST /api/auth-configs`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [likely-Rust-bug] `create-doomed` `POST /api/auth-configs`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (10): `update-add-oidc`, `update-untouched-fields`, `update-invalid-auth-provider`, `delete-doomed`, `delete-doomed-again`, `confinement-assign-role`, `confinement-login`, `confinement-list-forbidden`, `confinement-create-forbidden`, `confinement-delete-forbidden`
- [likely-Rust-bug] `create-missing-email-domain` `POST /api/auth-configs`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-invalid-email-domain` `POST /api/auth-configs`: Go 400 `error=INVALID_EMAIL_DOMAIN` / Rust 200
- [likely-Rust-bug] `create-oidc-missing-issuer` `POST /api/auth-configs`: Go 400 `error=OIDC_ISSUER_REQUIRED` / Rust 200
- [likely-Rust-bug] `create-oidc-missing-client-id` `POST /api/auth-configs`: Go 400 `error=OIDC_CLIENT_ID_REQUIRED` / Rust 200
- [likely-Rust-bug] `create-invalid-config-type` error body: Go `error=INVALID_CONFIG_TYPE` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] `create-invalid-auth-provider` error body: Go `error=INVALID_AUTH_PROVIDER` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] `duplicate-domain` error body: Go `error=DOMAIN_ALREADY_CONFIGURED` / Rust `error=EMAIL_DOMAIN_EXISTS code=EMAIL_DOMAIN_EXISTS` (message text differs too)
- [likely-Rust-bug] `update-unknown` error body: Go `error=AuthConfig_NOT_FOUND` / Rust `error=AUTH_CONFIG_NOT_FOUND code=AUTH_CONFIG_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `confinement-create-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`

### auth-remainder

**`auth-remainder/auth-remainder.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-internal-idp`: `/allowedEmailDomains` [] → «absent»; `/allowedRoleIds` [] → «absent»; `/code` parity-s3-ar-idp-f0f6bce23a3… → «absent» (+7 more)
- [likely-Rust-bug] `logout-admin`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] `login-admin-again`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `oidc-login-missing-domain-and-provider`: `/headers/Content-Type` application/json → text/plain; charset=utf-8; `/` {"error":"DOMAIN_REQUIRED","… → Failed to deserialize query …
- [likely-Rust-bug] `oidc-login-legacy-email-param`: `/headers/Content-Type` application/json → text/plain; charset=utf-8; `/` {"error":"OIDC_NOT_CONFIGURE… → Failed to deserialize query …
- [likely-Rust-bug] `oidc-session-end-plain`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] `oidc-session-end-missing-client`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] `oidc-session-end-unknown-client`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `create-require2fa-mapping`, `create-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-gate-principal`
- [likely-Rust-bug] Rust has no `POST /auth/2fa/methods/email/begin` (answers 404): `self-email-begin`
- [likely-Rust-bug] Rust has no `POST /auth/2fa/methods/email/confirm` (answers 404): `self-email-confirm-wrong-code`
- [likely-Rust-bug] Rust has no `POST /auth/2fa/challenge/email` (answers 404): `challenge-email-invalid-token`
- [likely-Rust-bug] Rust has no `GET /auth/2fa/trusted-devices` (answers 404): `list-trusted-devices-before-revoke`
- [likely-Rust-bug] Rust has no `POST /auth/change-password/send-email-code` (answers 404): `change-password-send-email-code-no-mfa`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (12): `login-gate-user-first-time`, `enroll-totp-begin`, `enroll-totp-confirm-wrong-code`, `enroll-totp-confirm`, `enroll-email-begin`, `enroll-email-confirm-wrong-code`, `login-gate-user-second-time`, `challenge-email-with-pending-token`, `verify-email-pin-wrong-code`, `finish-login-with-totp`, `revoke-trusted-device`, `revoke-trusted-device-again`
- [likely-Rust-bug] `logout-gate-user-1` `POST /auth/logout`: Go 204 / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `logout-gate-user-2` `POST /auth/logout`: Go 204 / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `oidc-login-mapped-domain-not-oidc` error body: Go `error=OIDC_NOT_CONFIGURED` / Rust `error=Domain parity-s3-ar-f0f6bce23a38.example.test uses internal authentication, not OIDC` (message text differs too)
- [Go defect] `oidc-login-unmapped-domain` `GET /auth/oidc/login`: Go 500 `error=OIDC_RESOLVE_FAILED`, Rust 404 `error=No authentication configuration found for domain: parity-s3-ar-nomapping-f0f6bce23a38.example.test code=EMAIL_DOMAIN_NOT_MAPPED`

### auth

**`auth/mfa.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `logout`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] `auth-me-after-verify`: `/clientId` null → «absent»; `/clients` «absent» → ["*"]; `/id` «absent» → «auto:principalId» (+3 more)
- [likely-Rust-bug] `logout-2`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] `old-recovery-code-dead`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] `logout-3`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] `login-plain-again`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] Rust has no `GET /auth/2fa/status` (answers 404): `status-before`, `status-enrolled`, `status-after-recovery`, `status-after-removal`, `status-after-admin-reset`
- [likely-Rust-bug] Rust has no `POST /auth/2fa/methods/totp/begin` (answers 404): `totp-begin`, `totp-begin-2`
- [likely-Rust-bug] Rust has no `POST /auth/2fa/methods/totp/confirm` (answers 404): `totp-confirm-wrong-code`
- [likely-Rust-bug] Rust has no `GET /auth/2fa/trusted-devices` (answers 404): `trusted-devices`
- [likely-Rust-bug] Rust has no `POST /auth/2fa/recovery-codes/regenerate` (answers 404): `regenerate-recovery-codes`
- [likely-Rust-bug] Rust has no `DELETE /auth/2fa/methods/TOTP` (answers 404): `remove-totp`, `remove-totp-again`
- [likely-Rust-bug] Rust has no `POST /api/principals/{x}/reset-2fa` (answers 404): `admin-reset-2fa`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (9): `totp-confirm`, `verify-wrong-code`, `verify-unknown-method`, `verify`, `verify-replayed-code`, `verify-with-recovery-code`, `verify-old-recovery-code`, `verify-new-recovery-code`, `totp-confirm-2`
- [likely-Rust-bug] `login-gated` `POST /auth/login`: Go 200 / Rust (no record) — rust: capture 'mfaToken': pointer /mfaToken not present in the response body
- [likely-Rust-bug] `login-after-remember` `POST /auth/login`: Go 200 / Rust (no record) — rust: capture 'mfaToken2': pointer /mfaToken not present in the response body
- [likely-Rust-bug] `login-gated-3` `POST /auth/login`: Go 200 / Rust (no record) — rust: capture 'mfaToken3': pointer /mfaToken not present in the response body

**`auth/oauth-code-flow.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-client`: `/client/apiAccess` false → «absent»; `/client/applications` [] → «absent»; `/client/defaultScopes/0` openid → «absent» (+2 more)
- [likely-Rust-bug] `authorize-bad-redirect`: `/headers/Cache-Control` no-store → «absent»
- [likely-Rust-bug] `token`: `/access_token/«jwt»/claims/all_applications` false → true; `/access_token/«jwt»/claims/applications/0` «absent» → *; `/access_token/«jwt»/claims/clients/0` «absent» → * (+3 more)
- [likely-Rust-bug] `token-replayed-code`: `/headers/Cache-Control` no-store → «absent»
- [likely-Rust-bug] `token-wrong-secret`: `/headers/Cache-Control` no-store → «absent»
- [likely-Rust-bug] `introspect-active`: `/client_id` «absent» → *; `/scope` «absent» → platform:*:*:*
- [likely-Rust-bug] `userinfo`: `/scope`  → platform:*:*:*
- [likely-Rust-bug] `userinfo-no-bearer`: `/headers/Cache-Control` no-store → «absent»
- [likely-Rust-bug] `refresh`: `/access_token/«jwt»/claims/all_applications` false → true; `/access_token/«jwt»/claims/applications/0` «absent» → *; `/access_token/«jwt»/claims/clients/0` «absent» → * (+3 more)
- [likely-Rust-bug] `api-me-with-client-credentials-token`: `/allApplications` false → «absent»; `/email` «absent» → null; `/permissions` ["platform:application-servi… → «absent»
- [likely-Rust-bug] `token-again`: `/access_token/«jwt»/claims/all_applications` false → true; `/access_token/«jwt»/claims/applications/0` «absent» → *; `/access_token/«jwt»/claims/clients/0` «absent» → * (+3 more)
- [likely-Rust-bug] `discovery`: `/response_types_supported/1` «absent» → token; `/response_types_supported/2` «absent» → id_token; `/response_types_supported/3` «absent» → code token (+3 more)
- [likely-Rust-bug] `authorize-unknown-client` `GET /oauth/authorize`: Go 400 `error=unauthorized_client` / Rust 307
- [likely-Rust-bug] `refresh-reuse-rotated-token` `POST /oauth/token`: Go 400 `error=invalid_grant` / Rust 200
- [likely-Rust-bug] `refresh-after-family-revoked` `POST /oauth/token`: Go 400 `error=invalid_grant` / Rust 200
- [likely-Rust-bug] `client-credentials-unbound-client` `POST /oauth/token`: Go 400 `error=unauthorized_client` / Rust 500 `error=server_error`
- [likely-Rust-bug] `refresh-revoked` `POST /oauth/token`: Go 400 `error=invalid_grant` / Rust 401 `error=invalid_grant`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `create-application`, `provision-service-account`
- [likely-Rust-bug] `auth-refresh-refuses-a-client-bound-token` error body: Go `code=UNAUTHENTICATED` / Rust `error=INVALID_TOKEN code=INVALID_TOKEN` (message text differs too)

**`auth/session.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login-wrong-password` error body: Go `code=UNAUTHENTICATED` / Rust `error=UNAUTHORIZED code=UNAUTHORIZED` (message text differs too)
- [likely-Rust-bug] `client-switch-unknown` error body: Go `error=Client_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `auth-me-after-logout` error body: Go `code=UNAUTHENTICATED` / Rust `error=UNAUTHORIZED code=UNAUTHORIZED` (message text differs too)
- [likely-Rust-bug] `backoff-1` error body: Go `code=UNAUTHENTICATED` / Rust `error=UNAUTHORIZED code=UNAUTHORIZED` (message text differs too)
- [likely-Rust-bug] `backoff-2` error body: Go `code=UNAUTHENTICATED` / Rust `error=UNAUTHORIZED code=UNAUTHORIZED` (message text differs too)
- [likely-Rust-bug] `backoff-3` error body: Go `code=UNAUTHENTICATED` / Rust `error=UNAUTHORIZED code=UNAUTHORIZED` (message text differs too)
- [likely-Rust-bug] `backoff-4` error body: Go `code=UNAUTHENTICATED` / Rust `error=UNAUTHORIZED code=UNAUTHORIZED` (message text differs too)
- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `check-domain-passwordless-awaiting-setup`: `/passwordSetupRequired` true → «absent»
- [likely-Rust-bug] `auth-me`: `/clientId` null → «absent»; `/clients` «absent» → ["*"]; `/id` «absent» → «auto:principalId» (+3 more)
- [likely-Rust-bug] `api-me`: `/accessibleClientIds/0` «absent» → *; `/allApplications` true → «absent»; `/permissions` ["platform:*:*:*"] → «absent»
- [likely-Rust-bug] `client-accessible`: `/currentClientId` «absent» → null
- [likely-Rust-bug] `api-me-with-switched-token`: `/allApplications` true → «absent»; `/permissions` ["platform:*:*:*"] → «absent»
- [likely-Rust-bug] `logout`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-passwordless-check-domain-target`, `create-with-password-check-domain-target`, `create-backoff-victim`
- [likely-Rust-bug] Rust has no `POST /auth/password-setup/request` (answers 404): `password-setup-request-eligible`, `password-setup-request-ineligible`, `password-setup-request-bad-body`
- [likely-Rust-bug] Rust has no `GET /auth/login-history` (answers 404): `login-history`
- [likely-Rust-bug] Rust has no `POST /auth/change-password` (answers 404): `change-password-wrong-current`, `change-password-weak`, `change-password`, `change-password-back`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (3): `login-old-password`, `login-new-password`, `assign-victim-role`
- [likely-Rust-bug] `backoff-5` `POST /auth/login`: Go 429 `code=TOO_MANY_REQUESTS` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `backoff-6` `POST /auth/login`: Go 429 `code=TOO_MANY_REQUESTS` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `backoff-7` `POST /auth/login`: Go 429 `code=TOO_MANY_REQUESTS` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `backoff-right-password-while-backed-off` `POST /auth/login`: Go 429 `code=TOO_MANY_REQUESTS` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`

### authz

**`authz/permissions-from-roles.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `provision-service-account`: `/serviceAccount/principalId` «auto:principalId» → «saId»
- [likely-Rust-bug] `service-account-token`: `/access_token/«jwt»/claims/sub` «auto:principalId» → «saId»
- [likely-Rust-bug] `anchor-viewer-reads-every-client`: `/clients/0/notes` [] → «absent»; `/clients/0/statusChangedAt` «absent» → null; `/clients/0/statusReason` «absent» → null (+18 more)
- [likely-Rust-bug] `restore-admin-session`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `cleanup-service-account-lookup`: `/lastUsedAt` «time» → null
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `create-application`
- [likely-Rust-bug] `service-account-reads-principals` `GET /api/principals`: Go 403 `error=PERMISSION_REQUIRED` / Rust 200
- [likely-Rust-bug] `anchor-viewer-creates-an-event-type` `POST /api/event-types`: Go 403 `error=PERMISSION_REQUIRED` / Rust 400 `error=CODE_REQUIRED code=CODE_REQUIRED`
- [likely-Rust-bug] `cleanup-application` `DELETE /api/applications/{x}`: Go 204 / Rust 409 `error=APPLICATION_HAS_REFERENCES code=APPLICATION_HAS_REFERENCES`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-anchor-viewer`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (3): `assign-viewer-role`, `login-as-anchor-viewer`, `cleanup-anchor-viewer`

### bff

**`bff/bff.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `dashboard-stats`: `/activeUsers` 9 → 1; `/rolesDefined` 16 → 18
- [likely-Rust-bug] `developer-list-applications`: `/items/0/currentSpecId` «absent» → null; `/items/0/currentSyncedAt` «absent» → null; `/items/0/currentVersion` «absent» → null (+49 more)
- [likely-Rust-bug] `developer-get-application`: `/currentSpecId` «absent» → null; `/currentSyncedAt` «absent» → null; `/currentVersion` «absent» → null (+1 more)
- [likely-Rust-bug] `developer-sync-platform-openapi`: `/archivedPriorVersion` «absent» → null; `/version` dev → 0.1.0
- [likely-Rust-bug] `get-platform-application-id`: `/defaultBaseUrl` «absent» → null; `/hasLoginClient` false → «absent»; `/iconUrl` «absent» → null (+1 more)
- [likely-Rust-bug] `developer-get-platform-current-spec`: `/changeNotes` «absent» → null; `/changeNotesText` «absent» → null; `/spec/components/schemas/AccessListResponse` {"type":"object","required":… → «absent» (+2330 more)
- [likely-Rust-bug] `developer-list-platform-versions`: `/items/0/changeNotesText` «absent» → null; `/items/0/version` dev → 0.1.0
- [likely-Rust-bug] `developer-get-platform-version`: `/changeNotes` «absent» → null; `/changeNotesText` «absent» → null; `/spec/components/schemas/AccessListResponse` {"type":"object","required":… → «absent» (+2330 more)
- [likely-Rust-bug] `bff-create-event-type`: `/aggregate` order → «absent»; `/application` s3bfff0f6bce23a38 → «absent»; `/clientScoped` false → «absent» (+9 more)
- [likely-Rust-bug] `bff-add-schema`: `/specVersions/0/schema` {"type": "object"} → {"type":"object"}
- [likely-Rust-bug] `bff-finalise-schema`: `/specVersions/0/schema` {"type": "object"} → {"type":"object"}
- [likely-Rust-bug] `bff-deprecate-schema`: `/specVersions/0/schema` {"type": "object"} → {"type":"object"}
- [likely-Rust-bug] `bff-archive-event-type`: `/specVersions/0/schema` {"type": "object"} → {"type":"object"}
- [likely-Rust-bug] `bff-sync-platform-event-types`: `/schemas/unchanged` 0 → 71; `/schemas/updated` 0 → 1; `/total` 73 → 72 (+1 more)
- [likely-Rust-bug] `bff-roles-filter-applications`: `/options/1/code` parity-s3-bff-f0f6bce23a38 → parity-authz-f0f6bce23a38; `/options/1/name` Parity S3 BFF App → Parity Authz f0f6bce23a38; `/options/2/code` parity → parity-s3-bff-f0f6bce23a38 (+16 more)
- [likely-Rust-bug] `bff-list-permissions`: `/items/0/action` view → create; `/items/0/aggregate` widget → application; `/items/0/application` parity-s3-bff-f0f6bce23a38 → platform (+44 more)
- [likely-Rust-bug] `bff-sync-platform-roles`: `/total` 15 → 17; `/updated` 15 → 17
- [likely-Rust-bug] `bff-list-scheduled-jobs`: `/data/0/description` «absent» → null; `/data/0/lastFiredAt` «absent» → null; `/data/0/payload` «absent» → null (+6 more)
- [likely-Rust-bug] `bff-scheduled-jobs-filter-options`: `/applications` [{"value":"«auto:id»","label… → «absent»; `/clients/4/label` Parity Auth-Config Confineme… → Parity Confinement Client; `/clients/5/label` Parity Confinement Client → S1C AuditLogs Second Client (+5 more)
- [likely-Rust-bug] `bff-get-scheduled-job`: `/description` «absent» → null; `/lastFiredAt` «absent» → null; `/payload` «absent» → null (+2 more)
- [likely-Rust-bug] `bff-list-job-instances-empty`: `/totalPages` 0 → «absent»; `/total_pages` «absent» → 0
- [likely-Rust-bug] `confinement-roles-read-allowed`: `/items/0/applicationCode` parity-sync-f0f6bce23a38 → parity-s3-bff-f0f6bce23a38; `/items/0/clientManaged` true → false; `/items/0/description` d2 → null (+265 more)
- [likely-Rust-bug] `confinement-scheduled-job-list-scoped-to-own-client`: `/data/0` «absent» → {"id":"«jobId»","clientId":"…; `/data/1` «absent» → {"id":"«syncedJobId»","clien…; `/total` 0 → 2 (+2 more)
- [likely-Rust-bug] `confinement-event-type-list-unfiltered-by-scope`: `/items/0/description` «absent» → null; `/items/0/specVersions/0/schema` {"type": "object", "$schema"… → {"type":"object","$schema":"…; `/items/1/description` «absent» → null (+143 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (3): `create-application`, `create-scheduled-job`, `confinement-create-client`
- [likely-Rust-bug] `developer-get-application-unknown` `GET /bff/developer/applications/app_doesnotexist{x}`: Go 404 `error=Application_NOT_FOUND` / Rust 403 `error=FORBIDDEN code=FORBIDDEN`
- [likely-Rust-bug] `bff-get-permission` `GET /bff/roles/permissions/parity-s3-bff-{x}:admin:widget:manage`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `confinement-scheduled-job-out-of-scope-is-404-not-403` `GET /bff/scheduled-jobs/{x}`: Go 404 `error=ScheduledJob_NOT_FOUND` / Rust 200 `code=s3bff-sj-f0f6bce23a38`
- [likely-Rust-bug] `developer-current-spec-none-yet` error body: Go `error=OpenApiSpec_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `developer-get-version-unknown` error body: Go `error=OpenApiSpec_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `developer-get-version-belongs-to-different-app` error body: Go `error=OpenApiSpec_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `bff-get-event-type-unknown` error body: Go `error=EventType_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `bff-get-event-type-after-delete` error body: Go `error=EventType_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `bff-get-role-unknown` error body: Go `error=Role_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `bff-get-permission-unknown` error body: Go `error=Permission_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `bff-get-scheduled-job-unknown` error body: Go `error=ScheduledJob_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `bff-get-instance-unknown` error body: Go `error=ScheduledJobInstance_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `bff-get-instance-logs-unknown` error body: Go `error=ScheduledJobInstance_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] Rust has no `PUT /bff/event-types/{x}` (answers 405): `bff-update-event-type`
- [likely-Rust-bug] Rust has no `POST /bff/roles/permissions` (answers 405): `bff-create-permission`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (6): `confinement-assign-roles`, `confinement-assign-roles-viewer`, `confinement-login`, `confinement-dashboard-forbidden`, `confinement-developer-list-forbidden`, `confinement-roles-write-forbidden`

### clients

**`clients/crud.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-by-id`: `/identifier` «auto:oidcClientId»-f0f6bce2… → parity-client-f0f6bce23a38; `/notes` [] → «absent»; `/statusChangedAt` «absent» → null (+1 more)
- [likely-Rust-bug] `get-by-identifier`: `/identifier` «auto:oidcClientId»-f0f6bce2… → parity-client-f0f6bce23a38; `/notes` [] → «absent»; `/statusChangedAt` «absent» → null (+1 more)
- [likely-Rust-bug] `list`: `/clients/0/notes` [] → «absent»; `/clients/0/statusChangedAt` «absent» → null; `/clients/0/statusReason` «absent» → null (+25 more)
- [likely-Rust-bug] `search-get`: `/clients/0/identifier` «auto:oidcClientId»-f0f6bce2… → parity-client-f0f6bce23a38; `/clients/0/notes` [] → «absent»; `/clients/0/statusChangedAt` «absent» → null (+1 more)
- [likely-Rust-bug] `get-after-update`: `/identifier` «auto:oidcClientId»-f0f6bce2… → parity-client-f0f6bce23a38; `/notes` [] → «absent»; `/statusChangedAt` «absent» → null (+1 more)
- [likely-Rust-bug] `get-after-suspend`: `/identifier` «auto:oidcClientId»-f0f6bce2… → parity-client-f0f6bce23a38; `/notes` [] → «absent»
- [likely-Rust-bug] `get-after-activate`: `/identifier` «auto:oidcClientId»-f0f6bce2… → parity-client-f0f6bce23a38; `/notes` [] → «absent»; `/statusReason` «absent» → null
- [likely-Rust-bug] `get-after-note`: `/identifier` «auto:oidcClientId»-f0f6bce2… → parity-client-f0f6bce23a38; `/notes` [{"category":"parity","text"… → «absent»; `/statusReason` «absent» → null
- [likely-Rust-bug] `client-applications-before`: `/applications/1/code` parity-s3-bff-f0f6bce23a38 → parity-authz-f0f6bce23a38; `/applications/1/description` parity harness → «absent»; `/applications/1/name` Parity S3 BFF App → Parity Authz f0f6bce23a38 (+23 more)
- [likely-Rust-bug] `client-applications-after-enable`: `/applications/1/code` parity-s3-bff-f0f6bce23a38 → parity-authz-f0f6bce23a38; `/applications/1/description` parity harness → «absent»; `/applications/1/name` Parity S3 BFF App → Parity Authz f0f6bce23a38 (+24 more)
- [likely-Rust-bug] `client-applications-after-bulk-update`: `/applications/1/code` parity-s3-bff-f0f6bce23a38 → parity-authz-f0f6bce23a38; `/applications/1/description` parity harness → «absent»; `/applications/1/name` Parity S3 BFF App → Parity Authz f0f6bce23a38 (+24 more)
- [likely-Rust-bug] `confinement-applications-of-own-client-allowed`: `/applications/1/code` parity-s3-bff-f0f6bce23a38 → parity-authz-f0f6bce23a38; `/applications/1/description` parity harness → «absent»; `/applications/1/name` Parity S3 BFF App → Parity Authz f0f6bce23a38 (+24 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (8): `create`, `suspend`, `activate`, `create-linked-application`, `create-for-duplicate`, `create-doomed`, `create-deactivate-target`, `deactivate-is-a-hard-delete`
- [likely-Rust-bug] Rust has no `POST /api/clients/search` (answers 405): `search-post`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug] `add-note` error body: Go / Rust (message text differs too)
- [likely-Rust-bug] `validation-bad-identifier` error body: Go `error=INVALID_IDENTIFIER` / Rust `error=INVALID_IDENTIFIER_FORMAT code=INVALID_IDENTIFIER_FORMAT` (message text differs too)
- [likely-Rust-bug] `duplicate-identifier` error body: Go `error=IDENTIFIER_EXISTS` / Rust `error=IDENTIFIER_EXISTS code=IDENTIFIER_EXISTS` (message text differs too)
- [likely-Rust-bug] `get-unknown` error body: Go `error=Client_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `suspend-unknown` error body: Go `error=Client_NOT_FOUND` / Rust `error=CLIENT_NOT_FOUND code=CLIENT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-doomed-after-delete` error body: Go `error=Client_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `delete-doomed-again` error body: Go `error=Client_NOT_FOUND` / Rust `error=CLIENT_NOT_FOUND code=CLIENT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-deactivate` error body: Go `error=Client_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `validation-missing-name` `POST /api/clients`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `validation-missing-identifier` `POST /api/clients`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (5): `confinement-assign-role`, `confinement-login`, `confinement-list-forbidden`, `confinement-read-own-client-forbidden`, `confinement-write-forbidden`

### code-first-connections

**`code-first-connections/code-first-connections.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-shared-signer`: `/principalId` «auto:principalId» → «sharedSaId»; `/serviceAccount/applicationId` «absent» → null; `/serviceAccount/description` «absent» → null (+1 more)
- [likely-Rust-bug] `create-shared-connection`: `/code` cfc-shared-f0f6bce23a38 → «absent»; `/createdAt` «time» → «absent»; `/name` CFC Shared → «absent» (+4 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (3): `create-app`, `provision-service-account`, `create-client`
- [Go defect] `sync-connections-create` `POST /api/applications/cfc-app-{x}/connections/sync`: Go 500 `error=AUDIT_WRITE`, Rust 404
- [Go defect] `sync-subscriptions-by-connection-code` `POST /api/applications/cfc-app-{x}/subscriptions/sync`: Go 404 `error=CONNECTION_NOT_FOUND`, Rust 200
- [Go defect] `sync-subscriptions-shared-connection-found-with-flag` `POST /api/applications/cfc-app-{x}/subscriptions/sync`: Go 500 `error=AUDIT_WRITE`, Rust 200
- [likely-Rust-bug] `sync-subscriptions-shared-connection-not-found-without-flag` `POST /api/applications/cfc-app-{x}/subscriptions/sync`: Go 404 `error=CONNECTION_NOT_FOUND` / Rust 200

### config

**`config/crud.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] Rust has no `PUT /api/config/parity-{x}/section-a/prop-one` (answers 404): `set-global`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug] `get-global` `GET /api/config/parity-{x}/section-a/prop-one`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `set-global-again-updates-in-place` `PUT /api/config/parity-{x}/section-a/prop-one`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `get-global-after-update` `GET /api/config/parity-{x}/section-a/prop-one`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `get-secret-as-anchor-unmasked` `GET /api/config/parity-{x}/section-a/secret-one`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `get-client-scoped` `GET /api/config/parity-{x}/section-a/prop-one`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `get-global-still-unaffected-by-client-scoped-write` `GET /api/config/parity-{x}/section-a/prop-one`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `set-missing-value` `PUT /api/config/parity-{x}/section-a/prop-two`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `delete-global` `DELETE /api/config/parity-{x}/section-a/prop-one`: Go 204 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `delete-again-is-idempotent` `DELETE /api/config/parity-{x}/section-a/prop-one`: Go 204 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `confinement-read-secret-is-masked` `GET /api/config/parity-{x}/section-a/secret-one`: Go 403 `error=FORBIDDEN` / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `confinement-write-forbidden` `PUT /api/config/parity-{x}/section-a/secret-one`: Go 403 `error=FORBIDDEN` / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `confinement-delete-forbidden` `DELETE /api/config/parity-{x}/section-a/secret-one`: Go 403 `error=FORBIDDEN` / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `confinement-read-other-app-forbidden` `GET /api/config/parity-other-{x}/section-a/prop-one`: Go 403 `error=FORBIDDEN` / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `set-secret` `PUT /api/config/parity-{x}/section-a/secret-one`: Go 200 / Rust (no record) — rust: capture 'secretConfigId': pointer /id not present in the response body
- [likely-Rust-bug] `set-client-scoped` `PUT /api/config/parity-{x}/section-a/prop-one`: Go 200 / Rust (no record) — rust: capture 'clientScopedConfigId': pointer /id not present in the response body
- [likely-Rust-bug] `get-not-found` error body: Go `error=Config_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=Config_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `confinement-create-client`, `confinement-create-role`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (2): `confinement-assign-role`, `confinement-login`

### connections

**`connections/connections.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-service-account`: `/principalId` «auto:principalId» → «saId»; `/serviceAccount/applicationId` «absent» → null; `/serviceAccount/description` «absent» → null (+1 more)
- [likely-Rust-bug] `create`: `/code` s1ccon-f0f6bce23a38 → «absent»; `/createdAt` «time» → «absent»; `/description` parity harness connection → «absent» (+5 more)
- [likely-Rust-bug] `get-created`: `/clientId` «absent» → null; `/clientIdentifier` «absent» → null; `/externalId` «absent» → null (+1 more)
- [likely-Rust-bug] `create-client-scoped`: `/clientId` «auto:id» → «absent»; `/code` s1ccon-f0f6bce23a38-scoped → «absent»; `/createdAt` «time» → «absent» (+5 more)
- [likely-Rust-bug] `list-by-status`: `/connections/0/clientId` «absent» → null; `/connections/0/clientIdentifier` «absent» → null; `/connections/0/description` «absent» → null (+10 more)
- [likely-Rust-bug] `list-by-client-id`: `/connections/0/clientIdentifier` «absent» → null; `/connections/0/description` «absent» → null; `/connections/0/externalId` «absent» → null (+1 more)
- [likely-Rust-bug] `get-updated`: `/clientId` «absent» → null; `/clientIdentifier` «absent» → null; `/source` UI → «absent»
- [likely-Rust-bug] `pause`: `/clientId` «absent» → null; `/clientIdentifier` «absent» → null; `/source` UI → «absent»
- [likely-Rust-bug] `activate`: `/clientId` «absent» → null; `/clientIdentifier` «absent» → null; `/source` UI → «absent»
- [likely-Rust-bug] `activate-again-is-idempotent`: `/clientId` «absent» → null; `/clientIdentifier` «absent» → null; `/source` UI → «absent»
- [likely-Rust-bug] `create-duplicate-first`: `/code` s1ccon-f0f6bce23a38-dup → «absent»; `/createdAt` «time» → «absent»; `/name` Dup → «absent» (+4 more)
- [likely-Rust-bug] `create-service-account-b`: `/principalId` «auto:principalId» → «saBId»; `/serviceAccount/applicationId` «absent» → null; `/serviceAccount/description` «absent» → null (+1 more)
- [likely-Rust-bug] `b-can-create-own`: `/clientId` «clientBId» → «absent»; `/code` s1cconb-f0f6bce23a38-own → «absent»; `/createdAt` «time» → «absent» (+5 more)
- [likely-Rust-bug] `delete-again` error body: Go `error=Connection_NOT_FOUND` / Rust `error=CONNECTION_NOT_FOUND code=CONNECTION_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=Connection_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-invalid-code-format` error body: Go `error=INVALID_CODE_FORMAT` / Rust `error=INVALID_CODE_FORMAT code=INVALID_CODE_FORMAT` (message text differs too)
- [likely-Rust-bug] `create-duplicate-conflict` error body: Go `error=CODE_EXISTS` / Rust `error=CONNECTION_CODE_EXISTS code=CONNECTION_CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] `update-missing` error body: Go `error=Connection_NOT_FOUND` / Rust `error=CONNECTION_NOT_FOUND code=CONNECTION_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `pause-missing` error body: Go `error=Connection_NOT_FOUND` / Rust `error=CONNECTION_NOT_FOUND code=CONNECTION_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-code-required` `POST /api/connections`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-name-required` `POST /api/connections`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-service-account-required` `POST /api/connections`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `create-second-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (7): `assign-role`, `login-as-b`, `b-cannot-read-anchors-scoped-connection`, `b-cannot-update-anchors-scoped-connection`, `b-cannot-pause-anchors-scoped-connection`, `b-cannot-create-platform-wide`, `b-cannot-create-scoped-to-foreign-client`

### dispatch-jobs

**`dispatch-jobs/dispatch-jobs.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-by-id`: `/clientId` «absent» → null; `/completedAt` «absent» → null; `/correlationId` «absent» → null (+17 more)
- [likely-Rust-bug] `get-by-id-raw`: `/attempts` «absent» → []; `/metadata` «absent» → []; `/scheduledFor` «absent» → «time» (+1 more)
- [likely-Rust-bug] `list-raw-alias-filtered-by-code`: `/0` «absent» → {"id":"«clientAJobId»","exte…; `/1` «absent» → {"id":"«platformJobId»","ext…
- [likely-Rust-bug] `filter-options`: `/aggregates` «absent» → []; `/applications` «absent» → []; `/clientIds` [] → «absent» (+5 more)
- [likely-Rust-bug] `by-event-alias`: `/0` «absent» → {"id":"«platformJobId»","ext…
- [likely-Rust-bug] `cancel-missing-job-is-404`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `list-raw-filtered-by-code` `GET /api/dispatch-jobs/list-raw`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] Rust has no `GET /api/dispatch-jobs/event/p{x}` (answers 404): `by-event`, `b-by-event-for-the-platform-wide-job`
- [likely-Rust-bug] Rust has no `POST /api/dispatch-jobs/{x}/cancel` (answers 404): `cancel-pending-job-is-409-not-failed`, `b-cannot-cancel-client-a-job-answers-404-not-403-pr3`
- [likely-Rust-bug] Rust has no `POST /api/dispatch-jobs/{x}/complete` (answers 404): `complete-pending-job-is-409-not-failed`, `b-cannot-complete-client-a-job-answers-404-not-403-pr3`
- [likely-Rust-bug] Rust has no `POST /api/dispatch-jobs/requeue` (answers 405): `requeue-both-seeded-jobs`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug] `get-missing-job` error body: Go `error=DispatchJob_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `create-second-client`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (5): `assign-role`, `login-as-b`, `b-cannot-get-client-a-job-answers-404-not-403-pr3`, `b-cannot-get-client-a-job-raw-answers-404-not-403-pr3`, `b-cannot-get-client-a-job-attempts-answers-404-not-403-pr3`

### dispatch-pools

**`dispatch-pools/dispatch-pools.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-created`: `/clientId` «absent» → null
- [likely-Rust-bug] `list-by-status`: `/pools/0/clientId` «absent» → null; `/pools/0/code` s1cdp-f0f6bce23a38 → parity-pool-f0f6bce23a38; `/pools/0/concurrency` 5 → 2 (+13 more)
- [likely-Rust-bug] `list-by-client-id`: `/pools/0/clientId` «auto:id» → null; `/pools/0/code` s1cdp-f0f6bce23a38-scoped → parity-pool-f0f6bce23a38; `/pools/0/concurrency` 10 → 2 (+7 more)
- [likely-Rust-bug] `get-updated`: `/clientId` «absent» → null
- [likely-Rust-bug] `get-after-archive`: `/clientId` «absent» → null
- [likely-Rust-bug] `get-after-suspend`: `/clientId` «absent» → null; `/status` SUSPENDED → ARCHIVED
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (5): `create`, `create-client-scoped`, `create-duplicate-first`, `create-second-client`, `b-can-create-own`
- [likely-Rust-bug] `archive` `POST /api/dispatch-pools/{x}/archive`: Go 204 / Rust 200 `code=s1cdp-f0f6bce23a38`
- [likely-Rust-bug] `suspend-after-archive-is-unconditional` `POST /api/dispatch-pools/{x}/suspend`: Go 204 / Rust 409 `error=DISPATCH_POOL_ALREADY_ARCHIVED code=DISPATCH_POOL_ALREADY_ARCHIVED`
- [likely-Rust-bug] `activate` `POST /api/dispatch-pools/{x}/activate`: Go 204 / Rust 200 `code=s1cdp-f0f6bce23a38`
- [likely-Rust-bug] `archive-again-is-a-no-op-write` `POST /api/dispatch-pools/{x}/archive`: Go 204 / Rust 409 `error=DISPATCH_POOL_ALREADY_ARCHIVED code=DISPATCH_POOL_ALREADY_ARCHIVED`
- [likely-Rust-bug] `create-code-required` `POST /api/dispatch-pools`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-invalid-code-format` `POST /api/dispatch-pools`: Go 400 `error=INVALID_CODE_FORMAT` / Rust 201
- [likely-Rust-bug] `create-name-required` `POST /api/dispatch-pools`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-invalid-concurrency` `POST /api/dispatch-pools`: Go 400 `error=INVALID_CONCURRENCY` / Rust 201
- [likely-Rust-bug] `create-invalid-rate-limit` `POST /api/dispatch-pools`: Go 400 `error=INVALID_RATE_LIMIT` / Rust 422
- [likely-Rust-bug] `delete-again` error body: Go `error=DispatchPool_NOT_FOUND` / Rust `error=DISPATCH_POOL_NOT_FOUND code=DISPATCH_POOL_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=DispatchPool_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-duplicate-conflict` error body: Go `error=CODE_EXISTS` / Rust `error=DISPATCH_POOL_CODE_EXISTS code=DISPATCH_POOL_CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] `update-missing` error body: Go `error=DispatchPool_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `archive-missing` error body: Go `error=DispatchPool_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (7): `assign-role`, `login-as-b`, `b-cannot-read-anchors-scoped-pool`, `b-cannot-update-anchors-scoped-pool`, `b-cannot-archive-anchors-scoped-pool`, `b-cannot-create-platform-wide`, `b-cannot-create-scoped-to-foreign-client`

### docs

**`docs/docs.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-unknown-platform-page`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `get-application-doc-before-sync`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `get-doc-unknown-application`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `get-doc-unknown-slug-on-known-application`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `relogin-as-admin-to-regrant`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] Rust has no `GET /api/docs` (answers 404): `list-before-any-app-docs`, `list-after-sync-includes-application-group`, `confinement-list-forbidden`, `confinement-read-succeeds-with-docs-view`
- [likely-Rust-bug] Rust has no `GET /api/docs/platform/platform-overview` (answers 404): `get-known-platform-page`
- [likely-Rust-bug] Rust has no `GET /api/docs/platform/identity-and-access` (answers 404): `get-another-known-platform-page`
- [likely-Rust-bug] Rust has no `POST /api/applications/parity-docs-{x}/docs/sync` (answers 404): `sync-app-doc`
- [likely-Rust-bug] Rust has no `GET /api/docs/applications/parity-docs-{x}/getting-started` (answers 404): `get-application-doc-after-sync`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `create-application`, `confinement-create-client`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (4): `confinement-assign-role-without-docs-view`, `confinement-login`, `confinement-regrant-with-docs-view`, `confinement-relogin`

### email-domain-mappings

**`email-domain-mappings/email-domain-mappings.json`** (false `covers`: updateEmailDomainMapping, deleteEmailDomainMapping, assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-idp-x`: `/allowedEmailDomains` [] → «absent»; `/allowedRoleIds` [] → «absent»; `/code` parity-edm-idpx-f0f6bce23a38 → «absent» (+7 more)
- [likely-Rust-bug] `create-idp-y`: `/allowedEmailDomains` [] → «absent»; `/allowedRoleIds` [] → «absent»; `/code` parity-edm-idpy-f0f6bce23a38 → «absent» (+7 more)
- [likely-Rust-bug] `list`: `/mappings/0/allowed2faMethods` [] → «absent»; `/mappings/0/allowedRoleIds` «absent» → []; `/mappings/0/primaryClientId` «absent» → null (+15 more)
- [likely-Rust-bug] `move-provider-unknown-mapping`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] Rust has no `POST /api/email-domain-mappings` (answers 404): `create`, `create-for-duplicate`
- [likely-Rust-bug] Rust has no `GET /api/email-domain-mappings/by-domain/parity-edm-{x}.example.test` (answers 404): `get-by-domain`
- [likely-Rust-bug] Rust has no `GET /api/email-domain-mappings/lookup` (answers 404): `lookup-not-found`, `confinement-lookup-still-works`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (14): `get-created`, `update`, `get-updated`, `update-2fa-missing-method`, `update-invalid-2fa-method`, `move-provider`, `move-provider-already-on-provider`, `move-provider-missing-target`, `delete`, `delete-again`, `get-deleted`, `confinement-grant-auth-admin-role`, `confinement-login`, `confinement-list-refused`
- [likely-Rust-bug] `lookup-found` `GET /api/email-domain-mappings/lookup`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `lookup-missing-domain-param` `GET /api/email-domain-mappings/lookup`: Go 400 `error=DOMAIN_REQUIRED` / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `create-duplicate-domain` `POST /api/email-domain-mappings`: Go 409 `error=DOMAIN_ALREADY_MAPPED` / Rust 404 `error=IDENTITY_PROVIDER_NOT_FOUND code=IDENTITY_PROVIDER_NOT_FOUND`
- [likely-Rust-bug] `create-invalid-domain` `POST /api/email-domain-mappings`: Go 400 `error=INVALID_EMAIL_DOMAIN` / Rust 404 `error=IDENTITY_PROVIDER_NOT_FOUND code=IDENTITY_PROVIDER_NOT_FOUND`
- [likely-Rust-bug] `create-partner-missing-primary-client` `POST /api/email-domain-mappings`: Go 400 `error=PRIMARY_CLIENT_REQUIRED` / Rust 404 `error=IDENTITY_PROVIDER_NOT_FOUND code=IDENTITY_PROVIDER_NOT_FOUND`
- [likely-Rust-bug] `create-invalid-scope-type` error body: Go `error=INVALID_SCOPE_TYPE` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `confinement-second-client`

### event-types

**`event-types/event-types.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-by-code`: `/createdBy` «auto:principalId» → «absent»; `/event` «absent» → created; `/eventName` created → «absent» (+1 more)
- [likely-Rust-bug] `add-version-canonical`: `/createdBy` «auto:principalId» → «absent»; `/event` «absent» → created; `/eventName` created → «absent» (+4 more)
- [likely-Rust-bug] `list-by-subdomain`: `/items/0/createdBy` «auto:principalId» → «absent»; `/items/0/event` «absent» → created; `/items/0/eventName` created → «absent» (+6 more)
- [likely-Rust-bug] `list-by-aggregate`: `/items/0/createdBy` «auto:principalId» → «absent»; `/items/0/event` «absent» → created; `/items/0/eventName` created → «absent» (+6 more)
- [likely-Rust-bug] `list-by-status`: `/items/0/createdBy` «auto:principalId» → «absent»; `/items/0/event` «absent» → created; `/items/0/eventName` created → «absent» (+6 more)
- [likely-Rust-bug] `list-by-client-id-filters-nothing`: `/items/0` {"id":"«etId»","code":"s1cet… → «absent»
- [likely-Rust-bug] `b-can-read-anchors-event-type-because-client-id-never-persisted`: `/createdBy` «auto:principalId» → «absent»; `/event` «absent» → created; `/eventName` created → «absent» (+7 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (3): `create`, `create-second-client`, `b-can-create-scoped-to-own-client`
- [likely-Rust-bug] `get-by-code-missing` error body: Go `error=EventType_NOT_FOUND` / Rust `error=EVENT_TYPE_NOT_FOUND code=EVENT_TYPE_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `add-version-missing-event-type` error body: Go `error=EventType_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `update-missing-event-type` error body: Go `error=EventType_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-code-required` error body: Go `error=CODE_REQUIRED` / Rust `error=CODE_REQUIRED code=CODE_REQUIRED` (message text differs too)
- [likely-Rust-bug] `create-name-required` error body: Go `error=NAME_REQUIRED` / Rust `error=NAME_REQUIRED code=NAME_REQUIRED` (message text differs too)
- [likely-Rust-bug] `create-invalid-code-format-wrong-segment-count` error body: Go `error=INVALID_CODE_FORMAT` / Rust `error=INVALID_CODE_FORMAT code=INVALID_CODE_FORMAT` (message text differs too)
- [likely-Rust-bug] `create-invalid-code-format-blank-segment` error body: Go `error=INVALID_CODE_FORMAT` / Rust `error=INVALID_CODE_FORMAT code=INVALID_CODE_FORMAT` (message text differs too)
- [likely-Rust-bug] `create-duplicate-conflict` error body: Go `error=CODE_EXISTS` / Rust `error=CODE_EXISTS code=CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] Rust has no `POST /api/event-types/{x}/schemas` (answers 404): `add-schema-alias`, `b-cannot-add-schema-to-anchors-event-type`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug] `add-version-conflict` `POST /api/event-types/{x}/versions`: Go 409 `error=VERSION_EXISTS` / Rust 200 `code=s1cetf0f6bce23a38:orders:order:created`
- [likely-Rust-bug] `add-version-schema-required` `POST /api/event-types/{x}/versions`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `update-name-required` `PUT /api/event-types/{x}`: Go 400 `error=NAME_REQUIRED` / Rust 204
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (5): `assign-role`, `login-as-b`, `b-cannot-create-platform-wide`, `b-cannot-create-scoped-to-foreign-client`, `b-cannot-update-even-its-own-client-scoped-event-type`

### events

**`events/events.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `batch-one-bad-item-reports-it-in-its-slot`: `/results/1/error` data is required → «absent»; `/results/1/id`  → «auto:id»; `/results/1/status` BAD_REQUEST → SUCCESS
- [likely-Rust-bug] `list-raw-alias-empty-state`: `/0` «absent» → {"id":"«withClientEventId»",…; `/1` «absent» → {"id":"«unknownCodeResultId»…; `/2` «absent» → {"id":"«dupeResultId0»","spe… (+47 more)
- [likely-Rust-bug] `filter-options-empty-state`: `/aggregates` «absent» → []; `/eventTypes` [] → «absent»; `/types` «absent» → []
- [likely-Rust-bug] `create-singular` `POST /api/events`: Go 201 / Rust (no record) — rust: capture 'eventDedup': pointer /event/deduplicationId not present in the response bod…
- [likely-Rust-bug] `create-singular-with-own-client-id-succeeds-because-anchor-can-access-any-client` `POST /api/events`: Go 201 / Rust (no record) — rust: capture 'withClientDedup': pointer /event/deduplicationId not present in the respons…
- [likely-Rust-bug] `create-singular-missing-data-is-validation-error` `POST /api/events`: Go 400 `error=VALIDATION` / Rust 201
- [likely-Rust-bug] `get-by-id-of-a-freshly-ingested-event-is-404-because-the-projector-never-ran` `GET /api/events/{x}`: Go 404 `error=Event_NOT_FOUND` / Rust 200
- [likely-Rust-bug] `list-raw-empty-state` `GET /api/events/list-raw`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `list-non-integer-limit-is-validation-error` `GET /api/events`: Go 400 `error=VALIDATION` / Rust 200
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (4): `batch`, `batch-empty-is-201-not-200`, `batch-repeated-deduplication-id-still-both-report-success`, `batch-unknown-client-code-leaves-event-unscoped-not-rejected`
- [likely-Rust-bug] `get-by-id-unknown` error body: Go `error=Event_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)

### functions

**`functions/functions.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `provision-host-service-account`: `/serviceAccount/principalId` «auto:principalId» → «hostServiceAccountId»
- [likely-Rust-bug] `host-service-account`: `/lastUsedAt` «absent» → null
- [likely-Rust-bug] `host-token`: `/access_token/«jwt»/claims/scope` platform:application-service… → platform:application-service…; `/access_token/«jwt»/claims/sub` «auto:principalId» → «hostServiceAccountId»; `/scope` platform:application-service… → platform:application-service…
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (4): `create-application`, `create-host-application`, `assign-host-role`, `create-application-not-addressable`
- [ruling-covered] allow-listed (function routes; Go has none) (33): `create-function`, `get-function`, `list-by-pattern`, `update-function`, `get-policy`, `put-policy`, `publish`, `list-versions`, `get-version-not-found`, `heartbeat`, `desired-state`, `emit-events`, `promote-not-ready`, `status`, `list-aliases`, `get-config`, `put-config`, `get-secrets`, `put-secret`, `get-secrets-after-set`, `delete-secret`, `delete-secret-not-found`, `retire-not-found`, `pools`, `duplicate-create`, `immutable-field`, `two-part-address`, `create-function-application-not-addressable`, `delete-function`, `claim-domain`, `list-domains`, `list-routes-requires-filter`, `release-domain`

### identity-providers

**`identity-providers/identity-providers.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create`: `/allowedEmailDomains` ["parity-idp-f0f6bce23a38.ex… → «absent»; `/allowedRoleIds` [] → «absent»; `/code` parity-idp-f0f6bce23a38 → «absent» (+9 more)
- [likely-Rust-bug] `get-created`: `/allowedRoleIds` [] → «absent»; `/oidcIssuerPattern` «absent» → null; `/syncRolesFromIdp` false → «absent»
- [likely-Rust-bug] `list`: `/identityProviders/0/allowedEmailDomains/0` example.com → parity-idp-noscope-f0f6bce23…; `/identityProviders/0/allowedRoleIds` [] → «absent»; `/identityProviders/0/code` internal → parity-idp-noscope-f0f6bce23… (+45 more)
- [likely-Rust-bug] `get-updated`: `/allowedRoleIds` [] → «absent»; `/oidcIssuerPattern` «absent» → null; `/syncRolesFromIdp` false → «absent»
- [likely-Rust-bug] `create-for-delete-guard`: `/allowedEmailDomains` [] → «absent»; `/allowedRoleIds` [] → «absent»; `/code` parity-idp-guard-f0f6bce23a3… → «absent» (+9 more)
- [likely-Rust-bug] `create-for-duplicate`: `/allowedEmailDomains` [] → «absent»; `/allowedRoleIds` [] → «absent»; `/code` parity-idp-dup-f0f6bce23a38 → «absent» (+7 more)
- [likely-Rust-bug] `create-fresh-domain-without-mapping-scope` `POST /api/identity-providers`: Go 400 `error=MAPPING_SCOPE_REQUIRED` / Rust 201 — rust: expected status 400 but got 201
- [likely-Rust-bug] `update` `PUT /api/identity-providers/{x}`: Go 200 `code=parity-idp-f0f6bce23a38` / Rust 204 — rust: expected status 200 but got 204
- [likely-Rust-bug] `update-noop-every-field-optional` `PUT /api/identity-providers/{x}`: Go 200 `code=parity-idp-f0f6bce23a38` / Rust 204 — rust: expected status 200 but got 204
- [likely-Rust-bug] `release-domains-before-delete` `PUT /api/identity-providers/{x}`: Go 200 `code=parity-idp-f0f6bce23a38` / Rust 204 — rust: expected status 200 but got 204
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `map-domain-to-guard-provider`, `confinement-second-client`
- [likely-Rust-bug] `delete-blocked-while-mapped` `DELETE /api/identity-providers/{x}`: Go 409 `error=DOMAINS_STILL_MAPPED` / Rust 204
- [likely-Rust-bug] `create-missing-name` `POST /api/identity-providers`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-oidc-missing-issuer` `POST /api/identity-providers`: Go 400 `error=OIDC_ISSUER_REQUIRED` / Rust 201
- [likely-Rust-bug] Rust has no `DELETE /api/identity-providers/{x}` (answers 404): `delete-guard-provider-now-succeeds`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-second-principal`
- [likely-Rust-bug] `delete-again` error body: Go `error=IdentityProvider_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-deleted` error body: Go `error=IdentityProvider_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-duplicate-code` error body: Go `error=CODE_EXISTS` / Rust `error=IDENTITY_PROVIDER_CODE_EXISTS code=IDENTITY_PROVIDER_CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] `get-unknown` error body: Go `error=IdentityProvider_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (4): `confinement-grant-auth-admin-role`, `confinement-login`, `confinement-list-refused`, `confinement-create-refused`

### idp-role-mappings

**`idp-role-mappings/idp-role-mappings.json`** (false `covers`: deleteIdpRoleMapping, assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `list`: `/items` [{"id":"«mappingId»","idpTyp… → «absent»; `/mappings` «absent» → [{"id":"«auto:id»","idpType"…; `/total` «absent» → 1
- [likely-Rust-bug] `create` `POST /api/idp-role-mappings`: Go 201 / Rust 200 — rust: expected status 201 but got 200
- [Go defect] `create-duplicate-idp-role-name` `POST /api/idp-role-mappings`: Go 500 `error=PERSIST`, Rust 409 `error=MAPPING_EXISTS code=MAPPING_EXISTS`
- [likely-Rust-bug] `create-missing-idp-type` `POST /api/idp-role-mappings`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-missing-idp-role-name` `POST /api/idp-role-mappings`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-missing-platform-role-name` `POST /api/idp-role-mappings`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (6): `delete`, `delete-again`, `confinement-grant-iam-admin-role`, `confinement-login`, `confinement-list-refused`, `confinement-create-refused`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `confinement-second-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-second-principal`

### login-attempts

**`login-attempts/login-attempts.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `re-login-as-admin`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `list-default`: `/items/0/ipAddress` 127.0.0.1 → null; `/items/1/failureReason` Invalid credentials → INVALID_CREDENTIALS; `/items/1/ipAddress` 127.0.0.1 → null (+134 more)
- [likely-Rust-bug] `list-filter-attempt-type`: `/items/0/ipAddress` 127.0.0.1 → null; `/items/1/failureReason` Invalid credentials → INVALID_CREDENTIALS; `/items/1/ipAddress` 127.0.0.1 → null (+112 more)
- [likely-Rust-bug] `list-filter-outcome`: `/items/0/failureReason` Invalid credentials → INVALID_CREDENTIALS; `/items/0/ipAddress` 127.0.0.1 → null; `/items/0/principalId` null → «auto:principalId» (+61 more)
- [likely-Rust-bug] `list-filter-identifier`: `/items/0/ipAddress` 127.0.0.1 → null; `/items/1/failureReason` Invalid credentials → INVALID_CREDENTIALS; `/items/1/ipAddress` 127.0.0.1 → null (+53 more)
- [likely-Rust-bug] `list-filter-date-range`: `/items/0/ipAddress` 127.0.0.1 → null; `/items/1/failureReason` Invalid credentials → INVALID_CREDENTIALS; `/items/1/ipAddress` 127.0.0.1 → null (+134 more)
- [likely-Rust-bug] `list-page-size-1`: `/items/0/ipAddress` 127.0.0.1 → null
- [likely-Rust-bug] `list-next-page`: `/items/0/failureReason` Invalid credentials → INVALID_CREDENTIALS; `/items/0/ipAddress` 127.0.0.1 → null; `/items/0/principalId` null → «auto:principalId»
- [likely-Rust-bug] `list-page-size-out-of-range`: `/items/0/ipAddress` 127.0.0.1 → null; `/items/1` {"id":"«auto:id»","attemptTy… → «absent»; `/items/2` {"id":"«auto:id»","attemptTy… → «absent» (+48 more)
- [likely-Rust-bug] `list-page-size-not-an-integer`: `/headers/Content-Type` application/json → text/plain; charset=utf-8; `/` {"$schema":"«base»/ErrorMode… → Failed to deserialize query …
- [likely-Rust-bug] `seed-a-failed-attempt` error body: Go `code=UNAUTHENTICATED` / Rust `error=UNAUTHORIZED code=UNAUTHORIZED` (message text differs too)
- [likely-Rust-bug] `list-malformed-cursor` `GET /api/login-attempts`: Go 200 / Rust 400 `error=VALIDATION_ERROR code=VALIDATION_ERROR`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `create-second-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (2): `login-as-b`, `confinement-list`

### me-public-config

**`me-public-config/me-public-config.json`**

- [likely-Rust-bug] `health`: `/version` dev → 0.1.0
- [likely-Rust-bug] `public-platform`: `/platformName` FlowCatalyst → «absent»
- [likely-Rust-bug] `public-login-theme`: `/accentColor` «absent» → null; `/backgroundColor` «absent» → null; `/backgroundGradient` «absent» → null (+8 more)
- [likely-Rust-bug] `config-platform-legacy-alias`: `/platformName` FlowCatalyst → «absent»
- [likely-Rust-bug] `q-openapi-alias`: `/components/schemas/AccessListResponse` {"additionalProperties":fals… → «absent»; `/components/schemas/AccessResponse` {"additionalProperties":fals… → «absent»; `/components/schemas/AddNoteRequest/additional…` true → «absent» (+2327 more)
- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `me`: `/accessibleClientIds/0` «absent» → *; `/allApplications` true → «absent»; `/permissions` ["platform:*:*:*"] → «absent»
- [likely-Rust-bug] `me-applications`: `/applications/0/baseUrl` «absent» → null; `/applications/0/description` «absent» → null; `/applications/0/iconUrl` «absent» → null (+93 more)
- [likely-Rust-bug] `me-clients`: `/clients/1/identifier` «auto:oidcClientId»dup-f0f6b… → parity-clientdup-f0f6bce23a3…; `/clients/12/identifier` «auto:oidcClientId»-f0f6bce2… → parity-s3-ar-f0f6bce23a38; `/clients/12/name` Parity Client Renamed → Parity S3 AR Client (+2 more)
- [likely-Rust-bug] `confinement-me`: `/accessibleClientIds/0` «confClientId» → *; `/allApplications` true → «absent»; `/email` parity-s3-me-conf-f0f6bce23a… → parity-admin@example.com (+5 more)
- [likely-Rust-bug] `confinement-me-clients-list-narrowed`: `/clients/0/id` «confClientId» → «auto:id»; `/clients/0/identifier` parity-s3-me-conf-f0f6bce23a… → default; `/clients/0/name` Parity S3 Me Confinement Cli… → Default Client (+22 more)
- [likely-Rust-bug] `logout`: `/headers/Set-Cookie` fc_session=«cookie»; HttpOnl… → fc_session=«cookie»; HttpOnl…
- [likely-Rust-bug] Rust has no `GET /api/openapi.json` (answers 404): `openapi-json`
- [likely-Rust-bug] Rust has no `GET /api/openapi.yaml` (answers 404): `openapi-yaml`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug] `me-client-unknown` error body: Go `error=Client_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `me-client-applications-unknown` error body: Go `error=Client_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `confinement-create-client`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (3): `confinement-assign-role`, `confinement-login`, `confinement-me-client-of-anchor-forbidden`
- [likely-Rust-bug] `unauthenticated-me` `GET /api/me`: Go 403 `error=UNAUTHENTICATED` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`

### oauth-clients

**`oauth-clients/oauth-clients.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-public`: `/client/apiAccess` false → «absent»; `/client/applications` [] → «absent»; `/client/defaultScopes/0` openid → «absent»
- [likely-Rust-bug] `create-confidential`: `/client/apiAccess` false → «absent»; `/client/applications` [{"id":"«auto:id»","name":"P… → «absent»; `/client/pkceRequired` true → false
- [likely-Rust-bug] `get-by-id`: `/apiAccess` false → «absent»; `/applications` [] → «absent»; `/defaultScopes/0` openid → «absent»
- [likely-Rust-bug] `get-by-client-id`: `/apiAccess` false → «absent»; `/applications` [] → «absent»; `/defaultScopes/0` openid → «absent»
- [likely-Rust-bug] `list`: `/clients/0/apiAccess` false → «absent»; `/clients/0/applicationIds/0` «appId» → «absent»; `/clients/0/applications` [{"id":"«appId»","name":"CFC… → «absent» (+108 more)
- [likely-Rust-bug] `get-after-update`: `/apiAccess` false → «absent»; `/applications` [] → «absent»; `/defaultScopes/0` openid → «absent»
- [likely-Rust-bug] `get-after-deactivate`: `/apiAccess` false → «absent»; `/applications` [] → «absent»; `/defaultScopes/0` openid → «absent»
- [likely-Rust-bug] `get-by-client-id-unknown` error body: Go `error=OAuthClient_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `rotate-secret-not-confidential` error body: Go `error=NOT_CONFIDENTIAL` / Rust `error=NOT_CONFIDENTIAL code=NOT_CONFIDENTIAL` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=OAuthClient_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `delete-again` error body: Go `error=OAuthClient_NOT_FOUND` / Rust `error=OAUTH_CLIENT_NOT_FOUND code=OAUTH_CLIENT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-invalid-type` error body: Go `error=INVALID_CLIENT_TYPE` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] `update-unknown` error body: Go `error=OAuthClient_NOT_FOUND` / Rust `error=OAUTH_CLIENT_NOT_FOUND code=OAUTH_CLIENT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-unknown` error body: Go `error=OAuthClient_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (7): `deactivate`, `activate`, `rotate-secret-confidential`, `regenerate-secret-alias`, `revoke-previous-secret`, `revoke-previous-secret-again`, `create-second-client`
- [likely-Rust-bug] `create-missing-name` `POST /api/oauth-clients`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-portal-api-access-conflict` `POST /api/oauth-clients`: Go 400 `error=PORTAL_API_ACCESS_CONFLICT` / Rust 201
- [likely-Rust-bug] `confinement-get` `GET /api/oauth-clients/{x}`: Go 403 `error=NO_PLATFORM_ROLE` / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (3): `login-as-b`, `confinement-list`, `confinement-create`

### platform-config

**`platform-config/access.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `seed-a-property` `PUT /api/config/parity-pc-{x}/section-a/prop`: Go 200 / Rust (no record) — rust: capture 'propAId': pointer /id not present in the response body
- [likely-Rust-bug] `seed-another-property` `PUT /api/config/parity-pc-{x}/section-b/prop`: Go 200 / Rust (no record) — rust: capture 'propBId': pointer /id not present in the response body
- [likely-Rust-bug] Rust has no `GET /api/platform-config/parity-pc-{x}` (answers 404): `list-properties`, `confinement-list-properties-refused`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `confinement-create-client`, `confinement-create-role`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (2): `confinement-assign-role`, `confinement-login`

### platform

**`platform/cors.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `public-allowed-unauthenticated` `GET /api/platform/cors/allowed`: Go 200 / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `add-duplicate-origin` `POST /api/platform/cors`: Go 409 `error=ORIGIN_ALREADY_EXISTS` / Rust 400 `error=ORIGIN_ALREADY_EXISTS code=ORIGIN_ALREADY_EXISTS`
- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-trimmed`: `/description` «absent» → null
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (11): `list-before`, `add`, `get-by-id`, `list-after-add`, `public-allowed-after-add`, `add-wildcard-host`, `add-trims-whitespace`, `add-for-duplicate`, `public-allowed-after-delete`, `confinement-create-client`, `confinement-public-allowed-still-works`
- [likely-Rust-bug] `add-blank-origin` error body: Go `error=ORIGIN_REQUIRED` / Rust `error=ORIGIN_REQUIRED code=ORIGIN_REQUIRED` (message text differs too)
- [likely-Rust-bug] `add-invalid-scheme` error body: Go `error=INVALID_ORIGIN_FORMAT` / Rust `error=INVALID_ORIGIN_FORMAT code=INVALID_ORIGIN_FORMAT` (message text differs too)
- [likely-Rust-bug] `add-invalid-path-not-allowed` error body: Go `error=INVALID_ORIGIN_FORMAT` / Rust `error=INVALID_ORIGIN_FORMAT code=INVALID_ORIGIN_FORMAT` (message text differs too)
- [likely-Rust-bug] `add-invalid-userinfo` error body: Go `error=INVALID_ORIGIN_FORMAT` / Rust `error=INVALID_ORIGIN_FORMAT code=INVALID_ORIGIN_FORMAT` (message text differs too)
- [likely-Rust-bug] `get-unknown` error body: Go `error=CorsOrigin_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=CorsOrigin_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `delete-again` error body: Go `error=CorsOrigin_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (4): `confinement-assign-role`, `confinement-login`, `confinement-list-forbidden`, `confinement-add-forbidden`

**`platform/profile-only.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `allowed-api-me`: `/accessibleClientIds/0` «auto:id» → *; `/allApplications` true → «absent»; `/email` parity-profile-only-f0f6bce2… → parity-admin@example.com (+5 more)
- [likely-Rust-bug] `auth-me`: `/clientId` «auto:id» → «absent»; `/clients` «absent» → ["*"]; `/email` parity-profile-only-f0f6bce2… → parity-admin@example.com (+8 more)
- [likely-Rust-bug] `restore-admin-session`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `allowed-api-me-anchor`: `/accessibleClientIds/0` «absent» → *; `/allApplications` true → «absent»; `/email` parity-profile-only-anchor-f… → parity-admin@example.com (+4 more)
- [likely-Rust-bug] `restore-admin-session-again`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `admin-passes-gated-route`: `/items/0/applicationCode` parity → platform; `/items/0/description` «absent» → View-only access to clients,…; `/items/0/displayName` Config Reader → Platform Admin Read-Only (+381 more)
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-role-less-user`, `create-role-less-anchor-domain-user`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (7): `login-as-role-less-user`, `gated-bff-roles`, `gated-api-clients`, `login-as-role-less-anchor-user`, `gated-bff-roles-anchor`, `gated-api-clients-anchor`, `service-passes-gated-route`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `create-app-for-service-account`, `provision-service-account`
- [likely-Rust-bug] `unauthenticated-passes-through` `GET /api/clients`: Go 403 `error=UNAUTHENTICATED` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`

### portal-apps

**`portal-apps/portal-apps.json`** (false `covers`: deletePortalApp, updateOAuthClient)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-client`: `/id` «oc3bPortalClientId» → «clientId»
- [likely-Rust-bug] `update-unknown-app`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `login-as-admin-again`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] Rust has no `POST /api/portal-apps` (answers 404): `create-confidential`, `create-public`, `create-duplicate-code-other-case`, `create-wildcard-callback`, `create-invalid-client-type`, `create-missing-name`, `create-invalid-code`, `create-app-for-two-client-delete`, `confinement-create-out-of-scope`
- [likely-Rust-bug] Rust has no `GET /api/portal-apps` (answers 404): `list-after-create`, `list-missing-client-id-non-anchor`, `list-after-update`, `list-after-deletes`, `confinement-list-missing-client-id`, `confinement-list-out-of-scope`, `confinement-list-own-scope-ok`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (21): `get-oauth-client-of-confidential-app`, `update-rename`, `update-deactivate`, `update-missing-id-name-blank`, `link-second-oauth-client-to-app3`, `delete-app3-two-oauth-clients`, `delete-app2-one-oauth-client`, `delete-app2-again`, `get-oc3a-after-app3-delete`, `delete-missing-client-id`, `confinement-assign-role`, `confinement-login`, `oauth-client-portal-app-id-on-create`, `oauth-client-portal-app-client-mismatch`, `oauth-client-unlink-app`, `get-oc4-after-unlink`, `oauth-client-relink-app`, `get-oc4-after-relink`, `oauth-client-clear-portal-client-clears-app-link`, `get-oc4-after-clear`, `delete-oc4`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `confinement-create-client`
- [likely-Rust-bug] `oauth-client-portal-app-not-found` `POST /api/oauth-clients`: Go 404 `error=PortalApp_NOT_FOUND` / Rust 201

### portal-assign

**`portal-assign/portal-assign.json`** (false `covers`: assignUnassignedPortalUsers, deactivatePortalUser, updatePortalApp)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `create-client`
- [likely-Rust-bug] Rust has no `POST /api/portal-apps` (answers 404): `create-app-a`, `create-app-b`
- [likely-Rust-bug] Rust has no `POST /api/portal-users` (answers 404): `ensure-unassigned-1`, `ensure-unassigned-2`, `ensure-holder-of-b`
- [likely-Rust-bug] Rust has no `GET /api/portal-apps` (answers 404): `list-apps-unassigned-count`, `list-apps-without-client-id`, `list-apps-after-assign`
- [likely-Rust-bug] Rust has no `GET /api/portal-users` (answers 404): `list-users-unassigned`, `list-users-unassigned-with-app-code`, `list-users-after-assign`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (6): `suspend-unassigned-2`, `assign-unassigned-to-a`, `assign-again-assigns-none`, `deactivate-app-b`, `assign-to-inactive-app`, `assign-missing-client-id`

### portal-users

**`portal-users/portal-users.json`** (false `covers`: activatePortalUser, deactivatePortalUser, deletePortalUser, grantPortalUserApp, revokePortalUserApp, updatePortalApp)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create-portal-oauth-client`: `/client/apiAccess` false → «absent»; `/client/applications` [] → «absent»; `/client/grantTypes/0` «absent» → authorization_code (+1 more)
- [likely-Rust-bug] `ensure-with-unknown-portal-app-code`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `list-filter-by-unknown-portal-app-code`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `create-client`, `create-second-client`
- [likely-Rust-bug] Rust has no `POST /api/portal-users` (answers 404): `ensure-sends-invite`, `ensure-again-already-has-password-false-idempotent`, `ensure-return-invite-link`, `ensure-with-valid-redirect`, `ensure-with-invalid-redirect`, `ensure-missing-client-id`, `ensure-with-portal-app-code`, `ensure-search-pat`, `ensure-search-jonas`, `ensure-search-underscore`, `confinement-ensure-out-of-scope`
- [likely-Rust-bug] Rust has no `GET /api/portal-users` (answers 404): `list-by-client`, `list-missing-client-id`, `list-after-deactivate`, `list-after-delete`, `list-filter-by-portal-app-code`, `search-email-prefix-case-insensitive`, `search-name-prefix`, `search-not-a-prefix-finds-nothing`, `search-escapes-underscore`, `search-escapes-percent`, `list-page-1-size-2`, `list-after-grant`, `list-after-revoke`, `confinement-list-out-of-scope`, `confinement-list-own-scope-ok`
- [likely-Rust-bug] Rust has no `POST /api/portal-apps` (answers 404): `create-app-for-user-tests`, `create-inactive-app-for-grant-test`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (13): `activate`, `deactivate`, `activate-missing-client-id`, `delete`, `delete-again`, `grant-app-to-search-pat`, `revoke-app-from-search-pat`, `grant-unknown-app-code`, `revoke-unknown-app-code`, `deactivate-app-for-grant-test`, `grant-inactive-app-fails`, `grant-confinement-portal-role`, `login-as-confinement-caller`

### portal

**`portal/portal.json`** (false `covers`: updatePortalApp, grantPortalUserApp, revokePortalUserApp)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `validate-unknown-token`: `/requiresFactor` false → «absent»
- [likely-Rust-bug] `create-portal-oauth-client`: `/client/apiAccess` false → «absent»; `/client/applications` [] → «absent»; `/client/grantTypes/0` «absent» → authorization_code (+1 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `create-client`
- [likely-Rust-bug] Rust has no `POST /api/portal-users` (answers 404): `ensure-portal-user-with-invite-link`, `ensure-gate-user-granted-a-only`
- [likely-Rust-bug] Rust has no `GET /portal/authorize` (answers 404): `authorize-not-a-portal-client`, `authorize`, `authorize-again-for-password-reset`
- [likely-Rust-bug] Rust has no `POST /portal/auth/check-domain` (answers 404): `check-domain-unknown-flow`
- [likely-Rust-bug] Rust has no `GET /portal/auth/oidc/login` (answers 404): `portal-oidc-login-missing-params`
- [likely-Rust-bug] Rust has no `POST /api/portal-apps` (answers 404): `create-gate-app-a`, `create-gate-app-b`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (34): `validate-invite-token`, `confirm-portal-password`, `confirm-portal-password-token-already-burned`, `check-domain`, `login-wrong-password`, `login-right-password`, `login-flow-already-consumed`, `portal-password-reset-known-email`, `portal-password-reset-unknown-email`, `portal-oidc-login-unknown-provider`, `portal-oidc-login-flow-now-burned`, `set-gate-user-password`, `authorize-via-a`, `login-via-a-succeeds`, `redeem-code-a`, `authorize-via-b-first`, `login-via-b-wrong-password`, `login-via-b-correct-password-no-grant`, `grant-app-b`, `login-via-b-after-grant-succeeds`, `deactivate-app-a`, `authorize-via-a-second`, `login-via-a-after-deactivate`, `reactivate-app-a`, `revoke-app-a`, `authorize-via-a-third`, `login-via-a-after-revoke-fails`, `authorize-via-b-second`, `login-via-b-after-revoke-a-still-works`, `redeem-code-b2`, `authorize-via-b-third-for-redemption-race`, `login-via-b-for-redemption-race`, `revoke-app-b`, `redeem-code-b3-after-revoke`

### principals

**`principals/principals-access.json`** (false `covers`: listPrincipalClientAccess, grantPrincipalClientAccess, revokePrincipalClientAccess, setPrincipalClientAssociation, setPrincipalDeveloperCredential, revokePrincipalDeveloperCredential, listPrincipalApplicationAccess, assignPrincipalApplicationAccess, listPrincipalAvailableApplications)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `check-email-domain-new`: `/authMethod` internal → «absent»; `/hasAuthConfig` «absent» → false; `/hasIdpConfig` false → «absent» (+1 more)
- [likely-Rust-bug] `check-email-domain-existing`: `/authMethod` internal → «absent»; `/emailExists` true → false; `/hasAuthConfig` «absent» → false (+3 more)
- [likely-Rust-bug] `check-email-domain-missing`: `/headers/Content-Type` application/json → text/plain; charset=utf-8; `/` {"$schema":"«base»/ErrorMode… → Failed to deserialize query …
- [likely-Rust-bug] `create-user-sdk`: `/hasDeveloperCredential` false → «absent»
- [likely-Rust-bug] `create-user-no-invite`: `/hasDeveloperCredential` false → «absent»
- [likely-Rust-bug] `sync-users`: `/syncedEmails/0` parity-sync-f0f6bce23a38@exa… → «auto:entityId»@example.test
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (2): `create-second-client`, `confinement-second-client`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-partner-principal`, `create-association-target`, `create-developer-target`, `confinement-second-principal`
- [likely-Rust-bug] Rust has no `POST /api/principals/bulk-import` (answers 405): `bulk-import-users`, `bulk-import-missing-client`, `bulk-import-no-rows`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (19): `grant-client-access`, `list-client-access`, `grant-client-access-again-conflict`, `revoke-client-access`, `revoke-client-access-again`, `grant-client-access-unknown-client`, `set-client-association`, `set-client-association-missing-mode`, `grant-developer-role`, `set-developer-credential`, `revoke-developer-credential`, `set-developer-credential-not-a-developer`, `list-available-applications`, `list-application-access`, `assign-application-access`, `confinement-grant-client-admin-role`, `confinement-login`, `confinement-application-access-out-of-scope`, `confinement-client-access-anchor-only`
- [likely-Rust-bug] `list-developer-users` `GET /api/principals/developer-users`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `check-email-domain-invalid` error body: Go `error=INVALID_EMAIL` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] `create-user-return-invite-link` `POST /api/principals/users`: Go 200 / Rust (no record) — rust: capture 'returnInviteLink': pointer /inviteLink not present in the response body

**`principals/principals-core.json`** (false `covers`: updatePrincipal, getPrincipalVersion, activatePrincipal, deactivatePrincipal, deletePrincipal, listPrincipalRoles, assignPrincipalRoles, addPrincipalRole, removePrincipalRole, resetPrincipalPassword, sendPrincipalPasswordReset, resetPrincipalTwoFactor)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `list-all`: `/principals/0/active` true → false; `/principals/0/clientId` «clientBId» → null; `/principals/0/email` parity-assoc-f0f6bce23a38@ex… → parity-syncuser-f0f6bce23a38… (+159 more)
- [likely-Rust-bug] `list-by-client`: `/principals/0/email` parity-bulk1-f0f6bce23a38@ex… → null; `/principals/0/hasDeveloperCredential` false → «absent»; `/principals/0/id` «auto:id» → «auto:serviceAccountId» (+27 more)
- [likely-Rust-bug] `list-by-active`: `/principals/0/clientId` «clientBId» → null; `/principals/0/email` parity-assoc-f0f6bce23a38@ex… → «auto:entityId»@example.test; `/principals/0/hasDeveloperCredential` false → «absent» (+151 more)
- [likely-Rust-bug] `list-by-type`: `/principals/0/active` true → false; `/principals/0/clientId` «clientBId» → null; `/principals/0/email` parity-assoc-f0f6bce23a38@ex… → parity-syncuser-f0f6bce23a38… (+63 more)
- [likely-Rust-bug] `list-by-q`: `/principals/0` {"id":"«pId»","type":"USER",… → «absent»; `/total` 1 → 0
- [likely-Rust-bug] `list-sorted-desc`: `/principals/0/active` true → false; `/principals/0/clientId` «confClientId» → null; `/principals/0/email` parity-s3-me-conf-f0f6bce23a… → parity-syncuser-f0f6bce23a38… (+160 more)
- [likely-Rust-bug] `list-by-roles`: `/principals/0` {"id":"«confPrincipalId»","t… → «absent»; `/principals/1` {"id":"«confPrincipalId»","t… → «absent»; `/principals/2` {"id":"«pId»","type":"USER",… → «absent» (+6 more)
- [likely-Rust-bug] `confinement-list-hides-other-tenant`: `/principals/0/active` true → false; `/principals/0/clientId` «clientBId» → null; `/principals/0/email` parity-pcore-b-f0f6bce23a38@… → parity-syncuser-f0f6bce23a38… (+19 more)
- [likely-Rust-bug] `relogin-as-admin`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `invite-confirm-session-authenticates-me`: `/clientId` «auto:id» → «absent»; `/clients` «absent» → ["*"]; `/email` parity-pcore-invite-f0f6bce2… → parity-admin@example.com (+8 more)
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create`, `create-invalid-scope`, `create-missing-client`, `create-for-duplicate`, `create-duplicate-email`, `confinement-create-target`, `confinement-create-second-principal`, `invite-create-with-link`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (21): `get-created`, `get-version`, `update`, `get-updated`, `assign-roles`, `list-roles`, `add-role`, `remove-role`, `reset-password`, `send-password-reset`, `reset-2fa`, `deactivate`, `activate`, `delete`, `delete-again`, `get-deleted`, `confinement-grant-client-admin-role`, `confinement-login-as-b`, `confinement-read-out-of-scope`, `confinement-write-out-of-scope-roles`, `invite-confirm-establishes-session`
- [likely-Rust-bug] `get-unknown-id` error body: Go `error=Principal_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `confinement-create-second-client`
- [likely-Rust-bug] `invite-redirect-uri-invalid` `POST /api/principals/users`: Go 400 `error=INVITE_REDIRECT_URI_INVALID` / Rust 200 — rust: expected status 400 but got 200

### processes

**`processes/processes.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `cross-tenant-read-succeeds-because-global`: `/description` «absent» → null
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (11): `create`, `get-created`, `get-by-code`, `list-by-application`, `list-by-subdomain`, `get-updated`, `get-after-archive`, `list-archived-filter`, `create-duplicate-first`, `create-confinement-target`, `create-second-client`
- [likely-Rust-bug] `get-by-code-missing` error body: Go `error=Process_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `delete-again` error body: Go `error=Process_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=Process_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-code-required` error body: Go `error=CODE_REQUIRED` / Rust `error=CODE_REQUIRED code=CODE_REQUIRED` (message text differs too)
- [likely-Rust-bug] `create-invalid-code-format` error body: Go `error=INVALID_CODE_FORMAT` / Rust `error=INVALID_CODE_FORMAT code=INVALID_CODE_FORMAT` (message text differs too)
- [likely-Rust-bug] `create-name-required` error body: Go `error=NAME_REQUIRED` / Rust `error=NAME_REQUIRED code=NAME_REQUIRED` (message text differs too)
- [likely-Rust-bug] `create-duplicate-conflict` error body: Go `error=CODE_EXISTS` / Rust `error=CODE_EXISTS code=CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] Rust has no `POST /api/processes/sync` (answers 405): `sync-by-body`, `sync-by-body-missing-application-code`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug] `list-by-status` `GET /api/processes`: Go 200 / Rust 400 `error=VALIDATION_ERROR code=VALIDATION_ERROR`
- [likely-Rust-bug] `archive-again-is-idempotent` `POST /api/processes/{x}/archive`: Go 204 / Rust 409 `error=ALREADY_ARCHIVED code=ALREADY_ARCHIVED`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (2): `assign-role`, `login-as-b`

### reset-approvals

**`reset-approvals/reset-approvals.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `approve-unknown`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `approve-unknown-with-note`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `deny-unknown`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] `re-login-as-admin`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `confinement-approve-with-permission-unknown-id`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] Rust has no `GET /api/reset-approvals` (answers 404): `list-empty`, `confinement-list-no-permission`, `confinement-list-with-permission`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug] Rust has no `POST /api/reset-approvals/rar_doesnotexist0/approve` (answers 404): `confinement-approve-no-permission`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (1): `create-second-client`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (3): `login-as-no-permission-caller`, `grant-confinement-caller-client-admin`, `login-as-client-admin-caller`

### roles

**`roles/crud.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-by-id`: `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] `get-by-name-fallback`: `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] `get-by-code`: `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] `list`: `/roles/0/applicationCode` parity-sync-f0f6bce23a38 → parity-s3-bff-f0f6bce23a38; `/roles/0/applicationId` «auto:id» → «absent»; `/roles/0/clientManaged` true → false (+287 more)
- [likely-Rust-bug] `by-source-database`: `/0/applicationCode` parity → parity-s3-bff-f0f6bce23a38; `/0/description` «absent» → null; `/0/displayName` Config Reader → x (+13 more)
- [likely-Rust-bug] `filters-applications`: `/applicationCodes` ["parity","parity-sync-f0f6b… → «absent»; `/options` «absent» → [{"id":"«platformAppId»","co…
- [likely-Rust-bug] `get-after-update`: `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] `get-after-clear`: `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] `grant-by-body`: `/permissions/1` parity:admin:widget:manage → «absent»; `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] `revoke`: `/permissions/0` parity:admin:widget:manage → «absent»; `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] `permissions-catalogue-list`: `/permissions/0/action` «absent» → create; `/permissions/0/aggregate` «absent» → application; `/permissions/0/application` «absent» → platform (+47 more)
- [likely-Rust-bug] `confinement-read-succeeds`: `/permissions/0` parity:admin:widget:manage → «absent»; `/shortName` «absent» → role-f0f6bce23a38
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (3): `create`, `create-doomed`, `confinement-create-client`
- [likely-Rust-bug] `by-source-lenient` error body: Go `error=INVALID_SOURCE` / Rust `error=VALIDATION_ERROR code=VALIDATION_ERROR` (message text differs too)
- [likely-Rust-bug] `duplicate-role` error body: Go `error=ROLE_EXISTS` / Rust `error=ROLE_CODE_EXISTS code=ROLE_CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] `update-code-sourced-role-is-immutable` error body: Go `error=CODE_ROLE_IMMUTABLE` / Rust `error=CANNOT_MODIFY_ROLE code=CANNOT_MODIFY_ROLE` (message text differs too)
- [likely-Rust-bug] `delete-code-sourced-role-is-immutable` error body: Go `error=CODE_ROLE_IMMUTABLE` / Rust `error=CANNOT_DELETE_ROLE code=CANNOT_DELETE_ROLE` (message text differs too)
- [likely-Rust-bug] `get-unknown` error body: Go `error=Role_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-doomed-after-delete` error body: Go `error=Role_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `delete-doomed-again` error body: Go `error=Role_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-unknown-permission` error body: Go `error=Permission_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] Rust has no `GET /api/roles/parity:role-{x}/permissions` (answers 405): `list-role-permissions-empty`, `list-role-permissions-after-grants`, `list-role-permissions-after-revoke`
- [likely-Rust-bug] Rust has no `POST /api/roles/parity:role-{x}/permissions/parity:admin:widget:manage` (answers 405): `grant-by-path`, `grant-idempotent`
- [likely-Rust-bug] Rust has no `POST /api/roles/parity:doesnotexist-{x}/permissions/parity:x:y:z` (answers 405): `grant-unknown-role`
- [likely-Rust-bug] Rust has no `POST /api/roles/platform:viewer/permissions/parity:admin:widget:harness-probe-{x}` (answers 405): `grant-on-code-sourced-role-is-allowed`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-create-principal`
- [likely-Rust-bug] `revoke-absent-is-a-no-op` `DELETE /api/roles/parity:role-{x}/permissions/parity:admin:widget:delete`: Go 200 / Rust 400 `error=NO_CHANGES code=NO_CHANGES`
- [likely-Rust-bug] `validation-missing-application-code` `POST /api/roles`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `validation-missing-role-name` `POST /api/roles`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `validation-missing-display-name` `POST /api/roles`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `revoke-on-code-sourced-role-is-allowed` `DELETE /api/roles/platform:viewer/permissions/parity:admin:widget:harness-probe-{x}`: Go 200 / Rust 409 `error=CANNOT_MODIFY_ROLE code=CANNOT_MODIFY_ROLE`
- [likely-Rust-bug] `delete-unknown-permission-is-idempotent` `DELETE /api/roles/permissions/parity:does:not:exist`: Go 204 / Rust 405
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (4): `confinement-assign-readonly-role`, `confinement-login`, `confinement-write-forbidden`, `confinement-delete-forbidden`

### router-config

**`router-config/router-config.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `router-role-exists`: `/shortName` «absent» → router
- [likely-Rust-bug] `provision-router-service-account`: `/serviceAccount/principalId` «routerPrincipalId» → «routerServiceAccountId»
- [likely-Rust-bug] `router-service-account`: `/lastUsedAt` «absent» → null
- [likely-Rust-bug] `router-token`: `/access_token/«jwt»/claims/sub` «routerPrincipalId» → «routerServiceAccountId»
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (4): `create-router-application`, `assign-router-role`, `create-plain-application`, `provision-plain-service-account`
- [likely-Rust-bug] Rust has no `GET /api/dispatch/router-config` (answers 404): `router-config-with-router-role`, `router-config-without-the-permission`, `router-config-unauthenticated`

### scheduled-jobs

**`scheduled-jobs/scheduled-jobs.json`** (false `covers`: writeScheduledJobInstanceLog, assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-created`: `/clientId` «absent» → null; `/lastFiredAt` «absent» → null; `/updatedBy` «absent» → null
- [likely-Rust-bug] `get-by-code`: `/clientId` «absent» → null; `/lastFiredAt` «absent» → null; `/updatedBy` «absent» → null
- [likely-Rust-bug] `list-by-search`: `/data/0/clientId` «absent» → null; `/data/0/lastFiredAt` «absent» → null; `/data/0/updatedBy` «absent» → null
- [likely-Rust-bug] `get-updated`: `/clientId` «absent» → null; `/lastFiredAt` «absent» → null
- [likely-Rust-bug] `get-after-pause`: `/clientId` «absent» → null; `/lastFiredAt` «absent» → null
- [likely-Rust-bug] `get-after-resume`: `/clientId` «absent» → null; `/lastFiredAt` «absent» → null
- [likely-Rust-bug] `get-after-first-fire-has-active-instance`: `/clientId` «absent» → null; `/lastFiredAt` «absent» → null
- [likely-Rust-bug] `list-instances-queued`: `/data/0/clientId` «absent» → null; `/data/0/completedAt` «absent» → null; `/data/0/completionResult` «absent» → null (+5 more)
- [likely-Rust-bug] `get-after-archive`: `/clientId` «absent» → null; `/hasActiveInstance` false → true; `/lastFiredAt` «absent» → null
- [likely-Rust-bug] `list-page-non-integer-is-validation-error`: `/headers/Content-Type` application/json → text/plain; charset=utf-8; `/` {"$schema":"«base»/ErrorMode… → Failed to deserialize query …
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (7): `create`, `list-by-status-filtered-to-nothing`, `create-cron-invalid-shape`, `create-duplicate-first`, `create-client-scoped-confinement-target`, `create-second-client`, `b-can-create-and-fire-and-log-on-own-client-scoped-job`
- [likely-Rust-bug] `get-by-code-missing` error body: Go `error=ScheduledJob_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `delete-again` error body: Go `error=ScheduledJob_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=ScheduledJob_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-missing-instance` error body: Go `error=ScheduledJobInstance_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-duplicate-conflict` error body: Go `error=CODE_EXISTS` / Rust `error=CODE_EXISTS code=CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] `update-crons-required-empty-array` error body: Go `error=CRONS_REQUIRED` / Rust `error=CRONS_EMPTY code=CRONS_EMPTY` (message text differs too)
- [likely-Rust-bug] `fire-with-correlation-id` `POST /api/scheduled-jobs/{x}/fire`: Go 202 / Rust (no record) — rust: capture 'instanceId': pointer /instanceId not present in the response body
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (13): `get-instance`, `write-instance-log`, `list-instance-logs`, `complete-instance-sdk-dialect`, `get-instance-after-complete`, `write-log-missing-level`, `assign-role`, `login-as-b`, `b-cannot-read-client-a-scoped-job-is-403-forbidden-not-404`, `b-cannot-read-client-a-jobs-instance`, `b-cannot-log-on-client-a-instance`, `b-cannot-create-platform-wide`, `b-logs-on-its-own-instance`
- [likely-Rust-bug] `fire-archived-job-is-409-archived` `POST /api/scheduled-jobs/{x}/fire`: Go 409 `error=ARCHIVED` / Rust 415
- [likely-Rust-bug] `complete-missing-instance` `POST /api/scheduled-jobs/instances/sji_doesnotexist1/complete`: Go 404 `error=ScheduledJobInstance_NOT_FOUND` / Rust 422
- [likely-Rust-bug] `logs-of-missing-instance-is-empty-array-not-404` `GET /api/scheduled-jobs/instances/sji_doesnotexist1/logs`: Go 200 / Rust 404 `error=NOT_FOUND code=NOT_FOUND`
- [likely-Rust-bug] `create-invalid-code-format` `POST /api/scheduled-jobs`: Go 400 `error=INVALID_CODE_FORMAT` / Rust 201
- [likely-Rust-bug] `b-cannot-fire-client-a-job` `POST /api/scheduled-jobs/{x}/fire`: Go 403 `error=SCOPE_FORBIDDEN` / Rust 415
- [likely-Rust-bug] `fire-dup-job-for-a-log-validation-target` `POST /api/scheduled-jobs/{x}/fire`: Go 202 / Rust 415 — rust: expected status 202 but got 415
- [likely-Rust-bug] `fire-confinement-target-for-an-instance` `POST /api/scheduled-jobs/{x}/fire`: Go 202 / Rust 415 — rust: expected status 202 but got 415
- [likely-Rust-bug] `b-fires-its-own-job` `POST /api/scheduled-jobs/{x}/fire`: Go 202 / Rust 415 — rust: expected status 202 but got 415
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`

### service-accounts

**`service-accounts/service-accounts.json`**

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `create`: `/serviceAccount/applicationId` «absent» → null; `/serviceAccount/id` «saId» → «linkedPrincipalId»; `/serviceAccount/lastUsedAt` «absent» → null
- [likely-Rust-bug] `get-created`: `/applicationId` «absent» → null; `/id` «saId» → «linkedPrincipalId»; `/lastUsedAt` «absent» → null (+2 more)
- [likely-Rust-bug] `get-by-code`: `/applicationId` «absent» → null; `/id` «saId» → «linkedPrincipalId»; `/lastUsedAt` «absent» → null
- [likely-Rust-bug] `list`: `/serviceAccounts/0/code` app:parity-app-f0f6bce23a38 → app:cfc-app-f0f6bce23a38; `/serviceAccounts/0/description` Service account for applicat… → Service account for applicat…; `/serviceAccounts/0/id` «auto:entityId» → «auto:principalId» (+58 more)
- [likely-Rust-bug] `get-updated`: `/applicationId` «absent» → null; `/id` «saId» → «linkedPrincipalId»; `/lastUsedAt` «absent» → null (+2 more)
- [likely-Rust-bug] `regenerate-auth-token`: `/id` «saId» → «absent»
- [likely-Rust-bug] `regenerate-signing-secret-alias`: `/id` «saId» → «absent»
- [likely-Rust-bug] `get-deactivated`: `/active` false → true; `/applicationId` «absent» → null; `/id` «saId» → «linkedPrincipalId» (+3 more)
- [likely-Rust-bug] `create-for-duplicate`: `/serviceAccount/applicationId` «absent» → null; `/serviceAccount/description` «absent» → null; `/serviceAccount/id` «dupSaId» → «dupPrincipalId» (+1 more)
- [likely-Rust-bug] `mint-token-unknown-id`: `/headers/Content-Type` application/json → «absent»; `/` {"$schema":"«base»/ErrorMode… → 
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (3): `assign-roles`, `list-roles`, `confinement-second-client`
- [likely-Rust-bug] Rust has no `POST /api/service-accounts/{x}/token` (answers 404): `mint-token`
- [likely-Rust-bug] Rust has no `POST /api/service-accounts/{x}/regenerate-token` (answers 404): `regenerate-token-alias`
- [likely-Rust-bug] Rust has no `POST /api/service-accounts/{x}/regenerate-secret` (answers 404): `regenerate-signing-secret`
- [likely-Rust-bug] Rust has no `POST /api/service-accounts/{x}/deactivate` (answers 404): `deactivate`
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `confinement-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (3): `linked-account-refused-anchor-route`, `confinement-login`, `confinement-list-refused`
- [likely-Rust-bug] `delete-again` error body: Go `error=ServiceAccount_NOT_FOUND` / Rust `error=SERVICE_ACCOUNT_NOT_FOUND code=SERVICE_ACCOUNT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-deleted` error body: Go `error=ServiceAccount_NOT_FOUND` / Rust `error=SERVICE_ACCOUNT_NOT_FOUND code=SERVICE_ACCOUNT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-unknown-code` error body: Go `error=ServiceAccount_NOT_FOUND` / Rust `error=SERVICE_ACCOUNT_NOT_FOUND code=SERVICE_ACCOUNT_NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-duplicate-code` error body: Go `error=CODE_EXISTS` / Rust `error=CODE_EXISTS code=CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] `create-missing-code` `POST /api/service-accounts`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-missing-name` `POST /api/service-accounts`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-invalid-webhook-auth-type` `POST /api/service-accounts`: Go 400 `error=INVALID_AUTH_TYPE` / Rust 201
- [likely-Rust-bug] `confinement-assign-roles-refused` `PUT /api/service-accounts/{x}/roles`: Go 403 `error=NO_PLATFORM_ROLE` / Rust 404 `error=SERVICE_ACCOUNT_NOT_FOUND code=SERVICE_ACCOUNT_NOT_FOUND`

### smoke

**`smoke/event-types.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `health`: `/version` dev → 0.1.0
- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `me`: `/accessibleClientIds/0` «absent» → *; `/allApplications` true → «absent»; `/permissions` ["platform:*:*:*"] → «absent»
- [likely-Rust-bug] `get-created`: `/createdBy` «auto:principalId» → «absent»; `/event` «absent» → f0f6bce23a38; `/eventName` f0f6bce23a38 → «absent» (+1 more)
- [likely-Rust-bug] `list`: `/items/0` {"id":"«auto:id»","code":"pl… → «absent»; `/items/1` {"id":"«auto:id»","code":"pl… → «absent»; `/items/2` {"id":"«auto:id»","code":"pl… → «absent» (+73 more)
- [likely-Rust-bug] `get-updated`: `/createdBy` «auto:principalId» → «absent»; `/event` «absent» → f0f6bce23a38; `/eventName` f0f6bce23a38 → «absent» (+1 more)
- [likely-Rust-bug] `cross-tenant-read`: `/createdBy` «auto:principalId» → «absent»; `/description` «absent» → null; `/event` «absent» → f0f6bce23a38 (+2 more)
- [likely-Rust-bug] `unauthenticated` `GET /api/event-types`: Go 403 `error=UNAUTHENTICATED` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (3): `create`, `confinement-create`, `create-second-client`
- [likely-Rust-bug] `delete-again` error body: Go `error=EventType_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `validation-error` error body: Go `error=INVALID_CODE_FORMAT` / Rust `error=INVALID_CODE_FORMAT code=INVALID_CODE_FORMAT` (message text differs too)
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (2): `assign-role`, `login-as-b`

### subscriptions

**`subscriptions/subscriptions.json`** (false `covers`: assignPrincipalRoles)

- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `get-created`: `/applicationCode` «absent» → null; `/clientId` «absent» → null; `/clientIdentifier` «absent» → null (+9 more)
- [likely-Rust-bug] `list-by-status`: `/subscriptions/0/applicationCode` «absent» → cfc-app-f0f6bce23a38; `/subscriptions/0/clientId` «absent» → null; `/subscriptions/0/clientIdentifier` «absent» → null (+20 more)
- [likely-Rust-bug] `get-updated`: `/applicationCode` «absent» → null; `/clientId` «absent» → null; `/clientIdentifier` «absent» → null (+10 more)
- [likely-Rust-bug] `get-after-pause`: `/applicationCode` «absent» → null; `/clientId` «absent» → null; `/clientIdentifier` «absent» → null (+10 more)
- [likely-Rust-bug] `get-after-resume`: `/applicationCode` «absent» → null; `/clientId` «absent» → null; `/clientIdentifier` «absent» → null (+10 more)
- [likely-Rust-bug] only `$schema` differs (Go emits it, Rust does not) (5): `create`, `create-duplicate-first`, `create-confinement-target`, `create-second-client`, `b-can-create-scoped-to-own-client`
- [likely-Rust-bug] `pause` `POST /api/subscriptions/{x}/pause`: Go 204 / Rust 200 `code=s1csub-f0f6bce23a38`
- [likely-Rust-bug] `resume` `POST /api/subscriptions/{x}/resume`: Go 204 / Rust 200 `code=s1csub-f0f6bce23a38`
- [likely-Rust-bug] `resume-again-is-idempotent` `POST /api/subscriptions/{x}/resume`: Go 204 / Rust 409 `error=ALREADY_ACTIVE code=ALREADY_ACTIVE`
- [likely-Rust-bug] `create-code-required` `POST /api/subscriptions`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-name-required` `POST /api/subscriptions`: Go 400 `error=VALIDATION` / Rust 422
- [likely-Rust-bug] `create-invalid-endpoint` `POST /api/subscriptions`: Go 400 `error=INVALID_ENDPOINT` / Rust 201
- [likely-Rust-bug] `delete-again` error body: Go `error=Subscription_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `get-after-delete` error body: Go `error=Subscription_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `create-invalid-code-format` error body: Go `error=INVALID_CODE_FORMAT` / Rust `error=INVALID_CODE_FORMAT code=INVALID_CODE_FORMAT` (message text differs too)
- [likely-Rust-bug] `create-event-types-required` error body: Go `error=EVENT_TYPES_REQUIRED` / Rust `error=EVENT_TYPES_REQUIRED code=EVENT_TYPES_REQUIRED` (message text differs too)
- [likely-Rust-bug] `create-duplicate-conflict` error body: Go `error=CODE_EXISTS` / Rust `error=SUBSCRIPTION_CODE_EXISTS code=SUBSCRIPTION_CODE_EXISTS` (message text differs too)
- [likely-Rust-bug] `update-missing` error body: Go `error=Subscription_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] `pause-missing` error body: Go `error=Subscription_NOT_FOUND` / Rust `error=NOT_FOUND code=NOT_FOUND` (message text differs too)
- [likely-Rust-bug] Rust has no `POST /api/principals` (answers 405): `create-second-principal`
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (8): `assign-role`, `login-as-b`, `b-cannot-create-platform-wide`, `b-cannot-create-scoped-to-foreign-client`, `b-cannot-read-anchors-scoped-subscription`, `b-cannot-update-anchors-scoped-subscription`, `b-cannot-pause-anchors-scoped-subscription`, `b-cannot-delete-anchors-scoped-subscription`

### webauthn

**`webauthn/webauthn.json`**

- [likely-Rust-bug] `register-begin-unauthenticated` `POST /auth/webauthn/register/begin`: Go 403 `error=UNAUTHENTICATED` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `list-credentials-unauthenticated` `GET /auth/webauthn/credentials`: Go 403 `error=UNAUTHENTICATED` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `delete-credential-unauthenticated` `DELETE /auth/webauthn/credentials/pky_doesnotexist0`: Go 403 `error=UNAUTHENTICATED` / Rust 401 `error=UNAUTHORIZED code=UNAUTHORIZED`
- [likely-Rust-bug] `register-complete-unknown-state` `POST /auth/webauthn/register/complete`: Go 400 `error=STATE_NOT_FOUND` / Rust 422
- [likely-Rust-bug] `authenticate-begin-blank-email` `POST /auth/webauthn/authenticate/begin`: Go 400 `error=EMAIL_REQUIRED` / Rust 200
- [likely-Rust-bug] `authenticate-complete-unknown-state` `POST /auth/webauthn/authenticate/complete`: Go 403 `error=INVALID_CREDENTIALS` / Rust 422
- [likely-Rust-bug] `authenticate-complete-decoy-state` `POST /auth/webauthn/authenticate/complete`: Go 403 `error=INVALID_CREDENTIALS` / Rust 422
- [likely-Rust-bug] `authenticate-complete-via-authenticator` `POST /auth/webauthn/authenticate/complete`: Go 200 / Rust 401 `error=INVALID_CREDENTIALS code=INVALID_CREDENTIALS`
- [likely-Rust-bug] `login`: `/headers/Set-Cookie` fc_session=«cookie»; Expires… → fc_session=«cookie»; HttpOnl…; `/clientId` null → «absent»; `/permissions` ["platform:*:*:*","*"] → «absent» (+2 more)
- [likely-Rust-bug] `list-credentials-after-attempt`: `/0` {"id":"«auto:credentialId»",… → «absent»
- [likely-Rust-bug] `authenticate-begin-unknown-email`: `/options/publicKey/rpId` «auto:id» → «auto:rpId»; `/options/publicKey/userVerification` preferred → required
- [likely-Rust-bug] `authenticate-begin-for-admin-no-credential`: `/options/publicKey/allowCredentials/0/transpo…` ["internal"] → «absent»; `/options/publicKey/rpId` «auto:id» → «auto:rpId»; `/options/publicKey/timeout` 300000 → 60000 (+1 more)
- [likely-Rust-bug] `authenticate-begin-for-admin-with-credential`: `/options/publicKey/allowCredentials/0/transpo…` ["internal"] → «absent»; `/options/publicKey/rpId` «auto:id» → «auto:rpId»; `/options/publicKey/timeout` 300000 → 60000 (+1 more)
- [likely-Rust-bug] `register-begin` `POST /auth/webauthn/register/begin`: Go 200 / Rust 400 `error=VALIDATION_ERROR code=VALIDATION_ERROR` — rust: expected status 200 but got 400
- [likely-Rust-bug] `register-begin-for-real-ceremony` `POST /auth/webauthn/register/begin`: Go 200 / Rust 400 `error=VALIDATION_ERROR code=VALIDATION_ERROR` — rust: expected status 200 but got 400
- [likely-Rust-bug (cascade)] cascade of an earlier Rust failure (unresolved capture, or a request made under the wrong session) (4): `register-complete-name-required`, `register-complete-invalid-credential`, `register-complete-state-already-burned`, `register-complete-via-authenticator`
- [likely-Rust-bug] `delete-credential-unknown` error body: Go `error=Credential_NOT_FOUND` / Rust `error=CREDENTIAL_NOT_FOUND code=CREDENTIAL_NOT_FOUND` (message text differs too)

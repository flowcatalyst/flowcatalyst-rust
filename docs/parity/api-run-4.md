# API parity run 4: Go vs Rust

Date: 2026-09-26. Fourth full run of the Go-vs-Rust API parity harness (`harness/parity`, owner decision #29), after
the four per-area passes (`docs/parity/api-area-{a,b,c,d}.md`) were merged and owner decisions #33–#38 were taken.

| | |
|---|---|
| Go | `flowcatalyst-go` @ `73a6918`, built with `go build -mod=readonly` (Go tree unchanged) |
| Rust | `feat/decisions-33-38` @ `428f3198` (`fc-server` debug build, clean tree) |
| Scenarios | 45 files / 1363 steps, unchanged since run 1 |
| Seed | Go `fcdev init`, first attempt |
| Allow-list | 123 entries (78 before this branch, 45 added citing #33, #34, #35, #38, Direction and the `/auth/me` scope follow-up) |
| Command | `./target/debug/fc-parity --rust-bin-dir target/debug --report target/parity-run-4` |

## Totals

| status | run 1 | run 2 | run 3 | run 4 |
|---|---:|---:|---:|---:|
| OK | 89 | 587 | 871 | **1231** |
| ACCEPTED (allow-listed) | 33 | 35 | 37 | **92** |
| DIFF | 870 | 534 | 419 | **40** |
| ERROR | 371 | 207 | 36 | **0** |

No stale allow-list entry, no false `covers` claim. Coverage is unchanged: Go lockfile 252 / 256, outside-lockfile
surface 131 / 135. The run exits 1 on the 40 DIFFs.

Before this branch's changes (the merged areas only, same build otherwise) the run was 1229 / 68 / 66 / 0. This
branch moved 26 steps: 22 to ACCEPTED through the new allow-list entries, and 2 to OK through fixes (below); the
2FA cookie step that alternated between OK and DIFF is now OK every time.

## Per group

"Before" is the merged areas before this branch (run 4a); "run 4" is the final run.

| group | steps | before OK / ACC / DIFF / ERR | run 4 OK / ACC / DIFF / ERR |
|---|---:|---|---|
| `anchor-domains/*` | 23 | 21 / 1 / 1 / 0 | 21 / 2 / 0 / 0 |
| `applications/*` | 72 | 61 / 0 / 11 / 0 | 61 / 0 / 11 / 0 |
| `audit-logs/*` | 34 | 29 / 2 / 3 / 0 | 29 / 2 / 3 / 0 |
| `auth/*` | 99 | 93 / 3 / 3 / 0 | 95 / 4 / 0 / 0 |
| `auth-configs/*` | 26 | 26 / 0 / 0 / 0 | 26 / 0 / 0 / 0 |
| `auth-remainder/*` | 36 | 35 / 0 / 1 / 0 | 35 / 0 / 1 / 0 |
| `authz/*` | 15 | 13 / 1 / 1 / 0 | 13 / 2 / 0 / 0 |
| `bff/*` | 66 | 53 / 4 / 9 / 0 | 53 / 10 / 3 / 0 |
| `clients/*` | 43 | 38 / 1 / 4 / 0 | 38 / 5 / 0 / 0 |
| `code-first-connections/*` | 10 | 7 / 3 / 0 / 0 | 7 / 3 / 0 / 0 |
| `config/*` | 24 | 24 / 0 / 0 / 0 | 24 / 0 / 0 / 0 |
| `connections/*` | 34 | 32 / 1 / 1 / 0 | 32 / 2 / 0 / 0 |
| `dispatch-jobs/*` | 28 | 27 / 1 / 0 / 0 | 27 / 1 / 0 / 0 |
| `dispatch-pools/*` | 36 | 34 / 1 / 1 / 0 | 34 / 2 / 0 / 0 |
| `docs/*` | 21 | 18 / 0 / 3 / 0 | 18 / 0 / 3 / 0 |
| `email-domain-mappings/*` | 32 | 31 / 0 / 1 / 0 | 31 / 1 / 0 / 0 |
| `event-types/*` | 30 | 29 / 1 / 0 / 0 | 29 / 1 / 0 / 0 |
| `events/*` | 17 | 17 / 0 / 0 / 0 | 17 / 0 / 0 / 0 |
| `functions/*` | 41 | 7 / 33 / 1 / 0 | 7 / 34 / 0 / 0 |
| `identity-providers/*` | 28 | 28 / 0 / 0 / 0 | 28 / 0 / 0 / 0 |
| `idp-role-mappings/*` | 15 | 13 / 1 / 1 / 0 | 13 / 2 / 0 / 0 |
| `login-attempts/*` | 17 | 17 / 0 / 0 / 0 | 17 / 0 / 0 / 0 |
| `me-public-config/*` | 25 | 18 / 2 / 5 / 0 | 18 / 4 / 3 / 0 |
| `oauth-clients/*` | 32 | 31 / 0 / 1 / 0 | 31 / 0 / 1 / 0 |
| `platform/*` | 48 | 45 / 3 / 0 / 0 | 45 / 3 / 0 / 0 |
| `platform-config/*` | 10 | 10 / 0 / 0 / 0 | 10 / 0 / 0 / 0 |
| `portal/*` | 47 | 45 / 0 / 2 / 0 | 45 / 0 / 2 / 0 |
| `portal-apps/*` | 44 | 44 / 0 / 0 / 0 | 44 / 0 / 0 / 0 |
| `portal-assign/*` | 19 | 19 / 0 / 0 / 0 | 19 / 0 / 0 / 0 |
| `portal-users/*` | 48 | 48 / 0 / 0 / 0 | 48 / 0 / 0 / 0 |
| `principals/*` | 81 | 74 / 1 / 6 / 0 | 74 / 2 / 5 / 0 |
| `processes/*` | 31 | 30 / 1 / 0 / 0 | 30 / 1 / 0 / 0 |
| `reset-approvals/*` | 15 | 15 / 0 / 0 / 0 | 15 / 0 / 0 / 0 |
| `roles/*` | 46 | 41 / 4 / 1 / 0 | 41 / 4 / 1 / 0 |
| `router-config/*` | 13 | 12 / 1 / 0 / 0 | 12 / 1 / 0 / 0 |
| `scheduled-jobs/*` | 52 | 51 / 1 / 0 / 0 | 51 / 1 / 0 / 0 |
| `service-accounts/*` | 32 | 30 / 0 / 2 / 0 | 30 / 0 / 2 / 0 |
| `smoke/*` | 18 | 15 / 1 / 2 / 0 | 15 / 3 / 0 / 0 |
| `subscriptions/*` | 35 | 33 / 1 / 1 / 0 | 33 / 2 / 0 / 0 |
| `webauthn/*` | 20 | 15 / 0 / 5 / 0 | 15 / 0 / 5 / 0 |
| **total** | **1363** | **1229 / 68 / 66 / 0** | **1231 / 92 / 40 / 0** |

## What this branch changed

**Owner decisions.**

- **#37 developer portal** (`shared/bff_developer_api.rs`): an anchor caller still answers to Go's
  `anchorWith(application-openapi:view)` and sees every active application. A non-anchor caller holding the view or
  manage permission is admitted too, confined to the applications it can access (Go's `CanAccessApplication` rule)
  plus the seeded `platform` application, as Rust did before area A's `0a64796c`; another application answers 404.
  Anyone else gets Go's refusal; the response shapes stay Go's. No harness step shows a difference (the scenario's
  non-anchor caller holds neither permission and is refused on both sides). `docs/parity/read-permissions-vs-go.md`
  is updated.
- **#33 version**: every server reports `fc_common::BUILD_VERSION`: `FC_BUILD_VERSION` at compile time when set
  (Go's `-ldflags -X …/server.Version`), else the workspace version. That covers `/health` of `fc-server`,
  `fc-platform-server` and `fc-outbox-processor`, the router's health and monitoring documents, and the platform
  OpenAPI `info.version` (which the developer portal's synced platform spec carries). The Docker images take
  `FC_BUILD_VERSION` as a build argument, and the publish workflow passes the release tag, else the commit. There is
  no `/version` route on either side; Go reports `version` on `/health` and in its OpenAPI documents. The harness
  build is unflagged on both sides (`dev` vs `0.1.0`), allow-listed.
- **Allow-list**: 45 entries, each one step plus the members that differ, each citing its decision (see
  `harness/parity/expected-diffs.json`). #36 got none: the one step comparing command JSON (`audit-logs`
  `by-principal`) also differs in rows and field names that no decision covers, so a `**/operationJson` entry would
  hide real differences.

**Go-parity fixes** found by the run:

- A user created without a password is passwordless (no hash), as Go's `CreateUser`; Rust stored the hash of a random
  password, so `/auth/check-domain` never answered `passwordSetupRequired` for an invited user
  (`auth/session` `check-domain-passwordless-awaiting-setup`).
- `/auth/client/accessible` omits `currentClientId` when unset, as Go (`client-accessible`).
- The seeded `platform:admin:subscription:synced` schema carries Go's optional `clientId` (the event has carried it
  since area D's sync change). It was the only one of the 73 shared catalogue types whose schema differed from Go's.
- Completing a 2FA login with "remember this device" writes the trusted-device cookie before the session cookie
  every time, as Go. A cookie jar wrote them in hash order, so `auth-remainder` `finish-login-with-totp` (the harness
  records the first `Set-Cookie`) was OK or DIFF by chance.

## The 40 remaining DIFFs

Classes: **Go defect** (Go answers 500, leaves dangling rows or reports nothing, and Rust answers correctly; no
decision names the case yet), **cascade** (a consequence of an earlier Go defect on data later steps list),
**harness** (a normaliser artefact: the same value masked under different capture names), **open** (a known
difference owned by an open item), **Rust follow-up** (Rust should change; too large for this pass).

| step(s) | n | class | difference |
|---|---:|---|---|
| `applications/sdk-sync` `sync-{event-types,dispatch-pools,subscriptions,principals,processes}-*` | 11 | Go defect | Go answers 500 `AUDIT_WRITE`: its `aud_logs.entity_id` is `VARCHAR(17)` and the rollup row is keyed by the 24-character application code; Rust (migration 038) syncs. The same defect is ruled for `code-first-connections` under #31. It fits #38's rule ("Go answers 500 … and Rust answers correctly"), but the steps have no `expect` and differ in the whole body, so an entry would have to cover every member; left for an explicit owner call. |
| `principals-core` `list-all`, `list-by-type`, `list-sorted-desc`, `list-by-active` | 4 | cascade + Go defect | Rust lists the principal the sdk-sync created (Go rolled it back), and Go lists the SERVICE principal of the service account `authz` `cleanup-service-account` deleted: Go's delete removes the service-account row only and leaves its principal (active, still holding `platform:application-service`) and its OAuth client. Rust deletes all three. |
| `oauth-clients` `list` | 1 | Go defect | Go still lists the deleted service account's OAuth client (see above); the credentials of a deleted service account keep working on Go. |
| `audit-logs` `by-principal` | 1 | cascade + ruled + Rust follow-up | Rows: Rust has the sdk-sync audit rows Go rolled back, and provision-service-account is three Rust use cases (`AttachServiceAccountCommand`, `CreateCommand`, `CreateOAuthClientCommand`) where Go records one `ProvisionServiceAccountCommand` (area C open item). Casing: Go stores PascalCase command JSON (#36, ruled). Field names: some Rust commands name members differently from Go's JSON-tagged ones (`applicationId` vs `id` on application delete, `anchorDomainId` vs `id` on anchor-domain delete and update, the OAuth-client create command's members); follow-up. |
| `audit-logs` `entity-types-facet-…`, `operations-facet-…` | 2 | cascade + Rust follow-up | The facets list the entity types and operations of the rows above (sdk-sync rows; `AttachServiceAccountCommand` / `CreateOAuthClientCommand` vs `ProvisionServiceAccountCommand`; `SyncAppDocsCommand`). |
| `docs` ×3, `roles` `filters-applications`, `principals-access` `sync-users`, `service-accounts` `list` | 6 | harness | Byte-identical bodies once ids and times are set aside: the normaliser masks a value by the member it was first seen under, and Rust's sdk-sync audit rows (keyed by the application code) and its extra provisioning rows expose values under `entityId` that Go never showed, so the same code or id is `«auto:entityId»` on one side and plain or `«auto:id»` on the other. Goes away with the sdk-sync cascade. |
| `bff` `bff-sync-platform-event-types` (`/schemas/unchanged`) | 1 | Go defect | Go's sync-platform schema tally is "currently not instrumented" (always 0, `bff/event_types.go`); Rust reports 131 unchanged. The type counts are allow-listed (#35). |
| `auth-remainder` `oidc-login-unmapped-domain` | 1 | Go defect | Go 500 `OIDC_RESOLVE_FAILED` for a domain with no mapping; Rust 404 `EMAIL_DOMAIN_NOT_MAPPED`. A login route, not an admin write, so #38 does not name it. |
| `service-accounts` `mint-token` (`claims/name`) | 1 | Go defect | Go's account update does not rename the SERVICE principal, so its token still carries the old name; Rust keeps the two in step. |
| `bff` `developer-get-platform-current-spec`, `developer-get-platform-version`; `me-public-config` `openapi-json`, `openapi-yaml`, `q-openapi-alias` | 5 | open | The platform OpenAPI documents come from different generators (huma vs utoipa): about 3100 differing members each. The `version` members in them are allow-listed (#33). Open item on the cutover checklist. |
| `portal` `redeem-code-a`, `redeem-code-b2` | 2 | open (auth owner) | A portal access token's `tier` is `""` on Go and `CLIENT` on Rust; matching needs the platform's own token validation to accept an empty tier. |
| `webauthn` ×5 | 5 | open (library) | Ceremony options: Go (go-webauthn) advertises ten algorithms, UV `preferred`, `transports`, one decoy credential for an unknown email; Rust (webauthn-rs) two algorithms, UV `required`, credProps/credProtect, one or two decoys. Both are accepted by browsers; matching would weaken what webauthn-rs verifies. |

Nothing in the run is a Rust bug with a cheap fix left: the three small shape/status differences it found are fixed
above.

## Follow-ups

1. **Owner:** allow-list `applications/sdk-sync`'s eleven `AUDIT_WRITE` 500s under #38 (or #31, as for
   `code-first-connections`), with whole-response entries; the six harness artefacts and the sdk-sync half of the
   principal-list and audit diffs follow from them.
2. **Owner:** Go's service-account delete leaves the SERVICE principal and its OAuth client (working credentials
   for a deleted account). Rust deletes them. A ruling in the spirit of #34/#38 would allow-list
   `oauth-clients` `list` and the Go half of the principal lists.
3. **Rust:** give the application-delete, anchor-domain delete/update and OAuth-client create commands Go's JSON
   member names in the audit row (`#[serde(rename)]` on the command, checked against every handler that
   deserialises it), and record provision-service-account as one `ProvisionServiceAccountCommand` (area C).
4. **Cutover checklist:** the OpenAPI documents (huma vs utoipa) and the portal token's `tier`.

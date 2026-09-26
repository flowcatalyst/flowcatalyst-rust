# API parity, area A: audit logs, roles, authz, BFF, platform, config, me/public, docs

Date: 2026-09-26. Branch `feat/api-area-a`, off `main` @ `f7580210`. One of four per-area passes over the Go-vs-Rust
API parity harness (`harness/parity`, owner decision #29). Go is the reference (`flowcatalyst-go` @ `73a6918`);
owner rulings override it (`docs/owner-decisions-2026-09-25.md`).

Scenario groups: `bff/*`, `audit-logs/*`, `roles/*`, `authz/*`, `platform/*`, `platform-config/*`, `config/*`,
`me-public-config/*`, `docs/*`.

## Before and after

"Before" is parity run 3 on `main` (`a15dbbf3`, full run). "After" is a full run of all 45 files on this branch
(`12b86196`, debug `fc-server`), so the numbers include the data the other scenarios leave behind, as run 3's do.

| scenario file | run 3 OK / ACC / DIFF / ERR | area A OK / ACC / DIFF / ERR |
|---|---|---|
| `audit-logs/audit-logs.json` | 17 / 0 / 17 / 0 | 29 / 2 / 3 / 0 |
| `authz/permissions-from-roles.json` | 8 / 0 / 7 / 0 | 9 / 1 / 5 / 0 |
| `bff/bff.json` | 37 / 0 / 29 / 0 | 52 / 4 / 10 / 0 |
| `config/crud.json` | 20 / 0 / 4 / 0 | 24 / 0 / 0 / 0 |
| `docs/docs.json` | 18 / 0 / 3 / 0 | 18 / 0 / 3 / 0 |
| `me-public-config/me-public-config.json` | 14 / 0 / 11 / 0 | 17 / 2 / 6 / 0 |
| `platform-config/access.json` | 9 / 0 / 1 / 0 | 10 / 0 / 0 / 0 |
| `platform/cors.json` | 25 / 0 / 3 / 0 | 27 / 1 / 0 / 0 |
| `platform/profile-only.json` | 16 / 1 / 3 / 0 | 18 / 2 / 0 / 0 |
| `roles/crud.json` | 21 / 0 / 25 / 0 | 41 / 4 / 1 / 0 |
| **area A** | **185 / 1 / 103 / 0** | **245 / 16 / 28 / 0** |
| whole run (45 files) | 871 / 37 / 419 / 36 | 937 / 52 / 338 / 36 |

The whole-run gain outside area A (login-attempts, auth-remainder, reset-approvals, oauth-clients,
service-accounts, router-config) comes from the login `permissions` order and `null`. No allow-list entry is
stale.

## What changed

- **Audit logs** (`audit/api.rs`): the SPA's `applicationIds` / `clientIds` filters are honoured; page size
  outside 1..=200 means 50; a bad cursor is 400 `CURSOR`; `/recent` is the list; by-entity and by-principal are
  `{auditLogs, hasMore}`; one Go-shaped DTO with `omitempty` members; the gate is the audit-log view permission
  alone (Go), no anchor.
- **Audit operation names** (`usecase/audit_operation.rs`): every audit row records the name Go records for the
  same command (`CreateApplicationCommand` is stored as `CreateCommand`), so the operation facet and filter
  match Go and Go-era rows.
- **Permissions order**: login, 2FA completion, `/auth/me` and `/api/me` list permissions in Go's order (role by
  role in assignment order, each role sorted, first appearance kept), and `null` when there are none (login).
- **Roles** (`/api/roles`, `/bff/roles`): Go's DTOs (no `shortName` on `/api`, sorted permissions, `omitempty`),
  id-then-name resolution, `{applicationCodes}`, the `iam_permissions` catalogue on `/api/roles/permissions`, Go's
  `permissionCatalog` on `/bff/roles/permissions`, `INVALID_SOURCE`, `ROLE_EXISTS`, `CODE_ROLE_IMMUTABLE` (CODE
  only; an SDK role may change, as Go), `null` permissions on an empty role. `/api/roles` writes ask Go's
  permission first and then anchor (decision #25), so a caller lacking the permission is refused as Go refuses
  it; `/bff/roles` keeps Go's anchor check first.
- **BFF developer**: Go's `anchorWith(application-openapi:view)` gate and every active application (see below),
  404 for an unknown application, `omitempty` members.
- **BFF scheduled jobs**: Go's multi-select filters and in-query client confinement, `{data, page, size, total,
  totalPages}`, 404 for an unreachable job or instance, `applications` filter options.
- **BFF event types**: create answers the event type; schemas are served as PostgreSQL's `jsonb` text, as Go.
- **Public**: `platformName` on `/api/public/platform` and `/api/config/platform`; the login theme omits unset
  members and layers a client's theme (`?clientId=` / `?client=`).
- **Config / CORS / event-type writes**: a set without a description clears it; a duplicate CORS origin is 409;
  the event-type write refusal is Go's `PERMISSION_REQUIRED` `one of: …`.
- **Allow-list**: 19 entries for the role-catalogue extras that owner rulings make deliberate (below).

## Accepted (owner rulings)

Rust's built-in roles differ from Go's only by rulings, and after the order fix every remaining login, `/api/me`,
role-list and role-count difference in this area is exactly those extras (checked per step):

- `platform:messaging-admin` holds the function-runner permissions, and `platform:function-publisher` /
  `platform:function-host` exist: Direction (functions follow Java).
- `platform:admin`, `iam-admin`, `iam-readonly` and `viewer` hold the service-account permissions: #21 (Java
  ruling 13).

The same login diffs appear in other areas' scenarios (for example `anchor-domains` `confinement-login`); their
entries are left to those areas.

## What remains, and why

| step(s) | difference | owner of the fix |
|---|---|---|
| `audit-logs` `by-principal` | `operationJson` holds each platform's own command document: Go's commands have no JSON tags, so Go stores `{"Jobs":[{"Code":…}]}` where Rust stores camelCase. Rust also audits operations Go's run did not reach (the sdk-sync scenario's syncs). | owner: whether stored command JSON must match Go field for field |
| `audit-logs` entity-type / operation facets | Data from other scenarios: Rust audits the sdk-sync syncs (event types, dispatch pools, processes, principals, subscriptions) that Go's run did not; the provision-service-account route is three Rust use cases (`CreateCommand`, `AttachServiceAccountCommand`, `CreateOAuthClientCommand`) where Go records one `ProvisionServiceAccountCommand`. | area C (applications / sdk-sync) |
| `bff` `developer-list-applications`, `bff-roles-filter-applications`, `bff-scheduled-jobs-filter-options`, `me-applications` | An application Go deletes in `authz` `cleanup-application` survives on Rust: Rust's delete refuses an application with access grants (409 `APPLICATION_HAS_REFERENCES`), Go deletes it. `me-applications` also lacks `website` (Rust's application create does not keep it). | area C (applications) |
| `bff` `get-platform-application-id`, `authz` `provision-service-account`, `service-account-token`, `anchor-viewer-reads-every-client`, `cleanup-service-account-lookup`, `cleanup-application` | Application, service-account and client DTOs and behaviour. | area C |
| `bff` `bff-sync-platform-event-types`, `confinement-event-type-list-unfiltered-by-scope` | Rust's platform event-type catalogue has 131 types, Go's 73: Rust registers 58 types Go does not (`platform:admin:client:*`, `…:role:*`, `…:scheduled-job:*`, `…:process:*`, …); Go registers none that Rust lacks. | area B (event types) or an owner ruling on the catalogue |
| `bff` `developer-*-platform-*`, `me-public-config` `health` | `version`: Go reports its build variable (`dev` when built without ldflags), Rust its crate version (`0.1.0`). Not a contract; release builds differ anyway. | none (or set Rust's version at build time) |
| `me-public-config` `openapi-json`, `openapi-yaml`, `q-openapi-alias`, `bff` `developer-get-platform-*` spec bodies | The OpenAPI documents are generated by different tools (huma vs utoipa). | open item on the cutover checklist |
| `docs/*`, `roles` `filters-applications` | Normaliser artefact: an `entityId` Rust captured earlier (an sdk-sync audit row keyed by the application code, which Go's run never wrote) masks the application code in later steps. | follows area C's sdk-sync |
| `me-public-config` `me-clients` | Normaliser artefact from `oauth-clients` (`«auto:oidcClientId»`). | area C |

## For the apps and the SPA

- **Laravel SDK (integral, hr, rfp)**: `/api/roles` no longer sends `shortName` (Go never did; the SDK's `Role` DTO
  defaults it to `''`). `GET /api/roles/{name}/permissions` answers `null` for a role with no permissions, as Go.
  Role update and delete of an SDK-sourced role are now allowed, as in Go. Audit rows written by the platform now
  carry Go's operation names. Nothing an app calls changed shape otherwise.
- **AgentPlanner**: the login body's `permissions` is `null` for a user without roles, and ordered as Go orders it.
- **SPA**: the audit-log application/client filters work; `/bff/scheduled-jobs` filters and `totalPages` work;
  event-type create gets the created type back; the developer portal is anchor-only, as in Go.
- **Rust SDK**: `RoleResponse.short_name` is optional (`/api/roles` never carried it on Go).

## Flagged for the owner

- `/bff/developer/*` now follows Go (anchor plus `application-openapi:view`, every application). Rust used to
  admit application-scoped developers confined to their application access; restoring that needs a ruling
  (`docs/parity/read-permissions-vs-go.md`).
- Rust's application delete refuses while references remain; Go deletes (area C).
- The platform event-type catalogue (area B).

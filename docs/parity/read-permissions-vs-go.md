# Read permissions: Rust vs Go

Owner decisions 2026-09-25, follow-up "Read permissions": many Rust list and read endpoints only required a
login where Go checks a permission. This file lists every `GET` the Rust platform mounts under `/api` and
`/bff`, the gate Go puts on it, and Rust's gate before and after the change. The route list is the one
`crates/fc-platform/tests/route_auth_convention_test.rs` reads from the router
(`ROUTE_INVENTORY=1 cargo test -p fc-platform --test route_auth_convention_test -- --nocapture`).

Go references are `flowcatalyst-go/internal/platform/…`; `anchorWith(p)` is Go's "anchor scope, then
permission `p`" (`shared/auth/auth.go:704`). Unless a row says otherwise, a refusal answers as Go: 403
`PERMISSION_REQUIRED` (and 403 `ANCHOR_REQUIRED` first for `anchorWith`).

Every route below also sits behind the profile-only gate (Go `ProfileOnlyWithoutRole`): a USER with no role
and no permission gets 403 `NO_PLATFORM_ROLE` everywhere except `/auth/*`, `/portal/*` and `GET /api/me`.

**46 GET routes changed gate.** They are marked **changed**. The Docker test
`crates/fc-platform/tests/read_permissions_test.rs` hits each with an under-privileged caller.

## Changed

| Route | Go gate | Rust before | Rust after |
|---|---|---|---|
| `GET /api/applications` | `CanReadApplications` (`admin:application:view`) | login | **changed**: `can_read_applications` |
| `GET /api/applications/{id}` | `CanReadApplications` | login | **changed**: `can_read_applications` |
| `GET /api/applications/by-code/{code}` | `CanReadApplications` | login | **changed**: `can_read_applications` |
| `GET /api/applications/by-id/{id}/roles` | `CanReadApplications` | login | **changed**: `can_read_applications` |
| `GET /api/applications/{id}/clients` | `CanReadApplications` | login | **changed**: `can_read_applications` |
| `GET /api/applications/{id}/service-account` | (Rust only) | login | **changed**: `can_read_applications`, as its parent |
| `GET /api/clients` | `CanReadClients` = `anchorWith(admin:client:view)` | login, rows confined to the caller's clients | **changed**: `can_read_clients` |
| `GET /api/clients/search` | `CanReadClients` | login, rows confined | **changed**: `can_read_clients` |
| `GET /api/clients/by-identifier/{identifier}` | `CanReadClients` | login, client reach | **changed**: `can_read_clients` |
| `GET /api/clients/{id}` | `CanReadClients` | login, client reach | **changed**: `can_read_clients` |
| `GET /api/connections` | `CanReadConnections` (`messaging:connection:view`), `FilterClientScoped` | login, no row filter | **changed**: `can_read_connections`, platform rows plus the caller's clients' |
| `GET /api/connections/{id}` | `CanReadConnections`, client reach | login | **changed**: `can_read_connections`, client reach (403) |
| `GET /api/platform/cors` | `CanReadCorsOrigins` = `anchorWith(admin:cors-origin:view)` | login | **changed**: `can_read_cors_origins` |
| `GET /api/platform/cors/{id}` | `CanReadCorsOrigins` | login | **changed**: `can_read_cors_origins` |
| `GET /api/dispatch-pools` | `CanReadDispatchPools` (`messaging:dispatch-pool:view`), `FilterClientScoped` | login, rows confined | **changed**: `can_read_dispatch_pools`, rows confined as before |
| `GET /api/dispatch-pools/{id}` | `CanReadDispatchPools`, client reach | login, client reach | **changed**: `can_read_dispatch_pools`, client reach |
| `GET /api/email-domain-mappings` | `CanReadEmailDomainMappings` = `anchorWith(iam:email-domain-mapping:view)` | login | **changed**: `can_read_email_domain_mappings` |
| `GET /api/email-domain-mappings/{id}` | `CanReadEmailDomainMappings` | login | **changed**: `can_read_email_domain_mappings` |
| `GET /api/identity-providers` | `CanReadIdentityProviders` = `anchorWith(iam:idp:view)` | login | **changed**: `can_read_identity_providers` |
| `GET /api/identity-providers/{id}` | `CanReadIdentityProviders` | login | **changed**: `can_read_identity_providers` |
| `GET /api/login-attempts` | `CanReadLoginAttempts` = `anchorWith(admin:login-attempt:view)` | login | **changed**: `can_read_login_attempts` |
| `GET /api/roles` | `CanReadRoles` (`iam:role:view`) | login | **changed**: `can_read_roles` |
| `GET /api/roles/{roleName}` | `CanReadRoles` | login | **changed**: `can_read_roles` |
| `GET /api/roles/by-code/{code}` | `CanReadRoles` | login | **changed**: `can_read_roles` |
| `GET /api/roles/by-source/{source}` | `CanReadRoles` | login | **changed**: `can_read_roles` |
| `GET /api/roles/by-application/{applicationId}` | `CanReadRoles` | login | **changed**: `can_read_roles` |
| `GET /api/roles/filters/applications` | `CanReadRoles` | login | **changed**: `can_read_roles` |
| `GET /api/roles/permissions` | `CanReadRoles` | login | **changed**: `can_read_roles` |
| `GET /api/roles/permissions/{permission}` | `CanReadRoles` | login | **changed**: `can_read_roles` |
| `GET /api/principals` | `CanReadPrincipals` (`iam:user:view`); non-anchors see only principals of their clients | login, rows confined | **changed**: `can_read_principals`, rows confined as before |
| `GET /api/principals/{id}` | self, or `CanReadPrincipals`; another client's principal is 404 | login; another client's principal 403 | **changed**: self, or `can_read_principals`; out of reach 404 |
| `GET /api/principals/{id}/roles` | `CanReadPrincipals`, client reach | login, client reach | **changed**: `can_read_principals`, client reach |
| `GET /api/principals/{id}/application-access` | `CanReadPrincipals`, client reach | login, client reach | **changed**: `can_read_principals`, client reach |
| `GET /api/principals/{id}/available-applications` | `CanReadPrincipals`, client reach | login, client reach | **changed**: `can_read_principals`, client reach |
| `GET /api/principals/{id}/client-access` | `RequireAnchor` | login, client reach | **changed**: anchor (`ANCHOR_REQUIRED`) |
| `GET /api/principals/check-email-domain` | `CanReadPrincipals` | anchor | **changed**: `can_read_principals` (a client administrator's create form uses it) |
| `GET /api/audit-logs` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/recent` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/entity-types` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/operations` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/application-ids` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/client-ids` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/{id}` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/entity/{entityType}/{entityId}` | `admin:audit-log:view` | anchor | **changed**: `can_read_audit_logs` (anchor dropped, as Go; api-area-a) |
| `GET /api/audit-logs/principal/{principalId}` | `admin:audit-log:view` | anchor, or the principal itself | **changed**: `can_read_audit_logs` alone, as Go (api-area-a) |
| `GET /bff/dashboard/stats` | `RequireAnchor`, then `CanViewDashboardStats` (`admin:client:view` or `admin:application:view`) | anchor or `platform:*:*:*` | **changed**: `can_view_dashboard_stats` |

Deliberate deviation in this table: the audit-log reads keep anchor reach on top of Go's permission, because
their rows are not confined to the caller's clients (a non-anchor holder would read every tenant's trail).
Same reasoning as owner decisions #19 and #25. `GET /api/applications/{id}/service-account` is Rust-only and
takes its parent's gate.

## Unchanged: already Go's gate

| Route | Go gate | Rust |
|---|---|---|
| `GET /api/anchor-domains`, `/{id}`, `/check/{domain}` | `CanReadAnchorDomains` = `anchorWith(admin:anchor-domain:view)` | `can_read_anchor_domains` (`/check/{domain}` is Rust-only) |
| `GET /api/auth-configs`, `/{id}`, `/by-domain/{domain}` | `CanReadAuthConfigs` = `anchorWith(auth:client-auth-config:view)` | `can_read_auth_configs` |
| `GET /api/idp-role-mappings` | `CanReadIdentityProviders` | `can_read_identity_providers` |
| `GET /api/oauth-clients`, `/{id}`, `/by-client-id/{clientId}` | `CanReadOAuthClients` = `anchorWith(auth:oauth-client:view)` | `can_read_oauth_clients` |
| `GET /api/clients/{id}/applications` | anchor or client reach, no permission | same (allowlisted) |
| `GET /api/config-access/{appCode}` | `CanReadPlatformConfig` = `anchorWith(admin:config:view)` | `can_read_platform_config` |
| `GET /api/config/{appCode}`, `/{section}`, `/{section}/{property}` | anchor, or a read grant for the application; secrets masked for non-anchors | same (`can_read_config`) |
| `GET /api/dispatch-jobs*`, `/bff/dispatch-jobs*` | `messaging:dispatch-job:view` (`:view-raw` for raw), rows confined | `can_read_dispatch_jobs` / `_raw`, rows confined |
| `GET /api/events*`, `/bff/events*` | `messaging:event:view` (`:view-raw` for raw), rows confined | `can_read_events` / `_raw`, rows confined |
| `GET /bff/debug/events*`, `/bff/debug/dispatch-jobs*` | `:view-raw` | `can_read_events_raw` / `can_read_dispatch_jobs_raw` |
| `GET /api/event-types`, `/{id}`, `/by-code/{code}` | `CanReadEventTypes`, `FilterClientScoped` | `can_read_event_types`, rows confined |
| `GET /api/processes*`, `/bff/processes*` | `CanReadProcesses` (`messaging:process:view`) | `can_read_processes` (also admits `application-service:process:view`) |
| `GET /api/scheduled-jobs*`, `/bff/scheduled-jobs*` | `CanReadScheduledJobs`, rows confined | `can_read_scheduled_jobs` (API instances: `scheduled-job-instance:view` also admitted; the `/bff` instance routes ask `scheduled-job:view` alone, as Go), rows confined; an unreachable row is 404 |
| `GET /api/service-accounts`, `/{id}`, `/code/{code}`, `/{id}/roles` | `CanReadServiceAccounts` | `can_read_service_accounts` |
| `GET /api/subscriptions`, `/{id}` | `CanReadSubscriptions`, `FilterClientScoped` | `can_read_subscriptions`, rows confined |
| `GET /api/applications/{appCode}/roles` | (Rust-only SDK route) | `can_read_roles` + application scope |
| `GET /api/monitoring/*` | (Rust-only; Go's router exposes its own) | anchor |
| `GET /api/function*` | (Java reference, not Go) | the Java function permissions |

## Login only, as Go

Go checks no permission on these (the profile-only gate still applies). Each is on
`READS_WITHOUT_PERMISSION` in the convention test with its reason.

| Route | Go | Rust |
|---|---|---|
| `GET /api/me`, `/api/me/applications`, `/api/me/clients`, `/api/me/clients/{clientId}`, `/api/me/clients/{clientId}/applications` | `shared/me`: login; rows are the caller's own | same |
| `GET /bff/roles`, `/{roleName}`, `/filters/applications`, `/permissions`, `/permissions/{permission}` | `shared/bff/roles.go`: no check | login |
| `GET /bff/event-types/filters/{applications,subdomains,aggregates}` | no check | login |
| `GET /bff/filter-options/clients` | login, the caller's reachable clients | same |
| `GET /bff/filter-options`, `/dispatch-jobs`, `/dispatch-pools`, `/events`, `/subscriptions`, `/event-types*` | (Rust only) | login, rows confined to the caller's clients |
| `GET /api/platform/cors/allowed` | no check | login |
| `GET /api/email-domain-mappings/lookup/{domain}` | `/lookup?domain=` has no check | login |

## Where Rust stays stricter or broader than Go

- `GET /bff/event-types`, `GET /bff/event-types/{id}`: Go checks nothing; Rust keeps `can_read_event_types`
  and client confinement. Only the SPA calls `/bff`, and its event-type pages already need that permission.
- `GET /bff/developer/*`: a superset of Go by owner decision #37. An anchor caller answers to Go's
  `anchorWith(developer:application-openapi:view)` and sees every active application. A non-anchor caller
  holding the view or manage permission (an application-scoped developer) is admitted too, and sees only the
  applications it can access (Go's `CanAccessApplication` rule: all with `all_applications`, else its grants)
  plus the seeded `platform` application; another application answers 404. Anyone else is refused with Go's
  answer. Response shapes are Go's. `POST /bff/developer/sync-platform-openapi` asks
  `anchorWith(developer:application-openapi:sync)`, as Go.
- `GET /api/processes*` and scheduled-job instances admit one extra permission each (above); a superset of Go.
- Go answers an unauthenticated call to its ungated routes (for example `/bff/roles`); Rust requires a login on
  every `/api` and `/bff` route except the public ones in the convention test's `PUBLIC_ROUTES`.

## Go routes Rust does not have

Not in scope here (read gates only), listed so the gap is visible: `GET /api/principals/{id}/version`,
`GET /api/principals/developer-users`, `GET /api/applications/{id}/clients/{clientId}`,
`GET /api/events/list-raw`, `GET /api/dispatch-jobs/list-raw`, `GET /api/dispatch-jobs/event/{eventId}`,
`GET /api/docs*`, `GET /api/reset-approvals`, `GET /api/portal-users`, `GET /api/portal-apps`,
`GET /api/dispatch/router-config`.

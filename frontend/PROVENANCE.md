# Provenance of `frontend/`

The platform SPA is the Go platform's production SPA, taken verbatim.

- Source: `flowcatalyst-go`, directory `frontend/`
- Commit: `73a6918e3534c76338cab28eb9a36d6bd4b16c25` (`73a6918`); the last commit touching
  `frontend/` there is `a8ff165` (2026-09-24, "service accounts: a new account starts with no
  application access")
- Taken: 2026-09-25, as the git-tracked files of that directory (`git archive`)
- Left out: Go's server glue, `embed.go` and `handler.go`. Rust embeds `frontend/dist` itself
  (`bin/fc-dev`, rust-embed) or serves it from `FC_STATIC_DIR` (`fc-platform::router::serve_spa`).

The Rust platform is a drop-in replacement for Go, so the SPA calls the Rust platform's API exactly
as it calls Go's. Rust's older fork of this SPA (full-page create/detail views) was replaced
wholesale; the features Rust had that Go lacks are re-added on top in Go's idiom, in the commits that
follow this one. Anything in this directory that is not Go's is listed below.

## Changes on top of Go's SPA

(Kept current by each commit that diverges from Go.)

### Build and tooling glue

- `package.json`: `license` (AGPL-3.0-or-later) and `dev:full` runs `fc-dev` with `cargo watch`
  instead of Go's `make dev`. `vite.config.ts`: the proxy comment names the Rust backend. Vite's
  `base` and `outDir` are Go's defaults (`/`, `dist/`), which `bin/fc-dev` embeds and `serve_spa`
  serves unchanged.
- `openapi/openapi.json` is a verbatim copy of flowcatalyst-go's `api/openapi.lock.json` at the same
  commit, the document Go generates `src/api/generated/` from (Go's `frontend/openapi/openapi.json`
  was a stale snapshot). `openapi-ts.config.ts` reads it there instead of `../api/openapi.lock.json`;
  `pnpm api:generate` reproduces Go's committed `src/api/generated/` byte for byte. The root
  `justfile`'s `regen-sdks` no longer overwrites it with Rust's `/q/openapi`.

### Page gating (owner decision #8)

Go's mechanism is kept (`stores/permissions.ts`: `ROUTE_PERMISSIONS`, `canAccessPath`, `canSeeScope`,
`landingPath`; the sidebar and the route guard share them; a role-less user reaches only `/profile`).
On top of it:

- `ROUTE_PERMISSIONS` uses the platform catalogue's codes. Go's SPA named several that no role grants
  (`platform:iam:identity-provider:*`, `platform:iam:oauth-client:*`, `platform:admin:audit:view`,
  `platform:admin:cors:view`, `platform:admin:settings:view`), so only a super-admin saw those pages.
  A requirement may be a list (any one grants), as Rust's SPA had for create pages whose form also
  edits, `/developer` and `/processes`.
- `ANCHOR_ROUTES`: pages whose endpoints need anchor reach (clients, identity providers, email-domain
  mappings, OAuth clients, CORS, login attempts, audit log) also need anchor tier.
- The tier is read from `/auth/me`'s `scope` (a Rust addition to Go's body) into `User.scope`; when
  present it decides `userScope` / `isUnscopedUser`, else Go's "no home client" inference stands.

### Dashboard: temporary audit-log redact card

Rust's b36bc522, onto Go's dashboard: an "Audit logs" card in Platform Sync that confirms, then calls
`POST /bff/audit-logs/redact-existing` and toasts the counts (not part of Sync All; remove with the
backend route). `api/audit-logs.ts` gains `redactExistingAuditLogs`. Tests that mount components use
`@vue/test-utils` and `jsdom` (devDependencies added to Go's `package.json` / lockfile).

### Role assignment source tags

Rust's f6bf4a22: the user and service-account role tables highlighted `assignmentSource === "MANUAL"`,
which neither backend emits, so every role rendered muted. `utils/roleAssignment.ts`
(`assignmentSourceSeverity`) highlights `ADMIN_ASSIGNED` / legacy `ADMIN` and mutes the synced and
platform-granted sources.

### Functions (Go has none; Java is the reference)

- `api/functions.ts` + `api/generated-functions/` (types from
  `crates/fc-platform/resources/openapi/functions.openapi.json`, a second `openapi-ts` job), as in
  Rust's SPA (8e51797b and later).
- `api/client.ts` (Go's) gains two additive changes the function pages need: `ApiError.details` (the
  envelope's raw `details`, for per-field errors) and a caller-set `Content-Type` is kept (the
  artifact upload sends `application/octet-stream`).
- The function pages (`pages/functions/`, `pages/function-domains/`, `pages/function-policies/`),
  their routes, a "Functions" nav group, their `ROUTE_PERMISSIONS` entries, `/function-policies` in
  `ANCHOR_ROUTES`, and Rust's function tests (`tests/functions/`, adjusted to Go's permission rule:
  the permissions `/auth/me` sends decide, a role name alone grants nothing).

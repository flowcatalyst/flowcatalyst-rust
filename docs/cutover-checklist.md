# Go → Rust cutover checklist

The goal (owner, 2026-09-25): Rust is a **drop-in replacement for Go** in production. Go is the reference for
existing behaviour, Java for functions, and owner rulings override both
(`docs/owner-decisions-2026-09-25.md`). This file tracks what stands between today and a cutover. Tick items off
as they merge to `main`.

## Correctness gates (decisions #28, #29)
Delivery harness run 3 (after merging scheduler, outbox, router, lifecycle, events, and the TSID fix): **12/17 PASS**.
Go fails 3 scenarios that Rust passes (#31). `platform-down` and `router-restart` are in progress on `feat/delivery-fix`.
- [x] Mediation conformance corpus: a Rust runner, all rows passing, and the deviations from Go listed in
      `docs/parity/router-deviations-from-go.md` (`feat/go-conformance`)
- [x] Delivery parity harness, Go vs Rust end to end over SQS: scenarios pass or are ruled
      (`feat/harness-delivery`)
- [x] API parity runner, Go vs Rust on Java's 45 scenario files, built (`harness/parity`); run 1 in
      `docs/parity/api-run-1.md`: 89 OK, 33 ruled, 870 DIFF, 371 ERROR
- [ ] API parity converged: every diff fixed or ruled. Run 7, against Go `main` @ `5ffb009`: 1260 OK /
      87 ACCEPTED / 16 DIFF / 0 ERROR, allow-list 117 entries with none stale (`docs/parity/api-run-7.md`).
      Remaining: OpenAPI documents (5), webauthn library defaults (5), Go's `functions` platform docs page (3),
      audit by-principal/facets (2), the sync-platform schema tally (Go defect, unnamed)
- [x] **Service-account events use the account id:** every `platform:iam:serviceaccount:*` event (and
      `service-account-provisioned`) carries the account's `sac_` id as subject, group and
      `serviceAccountId`, so the audit `entityId` too, as Go (`feat/cutover-fixes`)
- [x] Syncs are atomic as Go's `usecaseop.Sync`: event types, processes, roles, dispatch pools,
      subscriptions (and the OpenAPI spec sync) plan every row, then write rows, per-row events and rollup
      in one transaction; a bad row writes nothing (`feat/cutover-fixes`)
- [x] Portal tokens carry Go's empty `tier`; the platform and the function host read it as no tier on an
      identity-only token, granting nothing (`feat/cutover-fixes`)
- [x] Go's auth purger: expired OAuth payloads, OIDC login states, portal login flows, 2FA email PINs and
      trusted devices, reset/invite tokens (30 days after expiry), lapsed OAuth secrets, and the
      `iam_login_attempts` quarterly partitions on a Go-partitioned database, every minute
      (`feat/cutover-fixes`)
- [x] Reset, invite and portal emails in Go's branded layout with the login theme (logo, colours, brand)
      (`feat/cutover-fixes`)

## Message pipeline (`docs/reviews/message-pipeline-review-2026-09-25.md`)
- [x] Scheduler publishes to SQS; jobs are inserted PENDING; Go's claim/hold/backoff model; `/process`
      authenticated, claimed, ACK semantics as Go; fan-out cache guard (`feat/go-scheduler`)
- [x] Outbox: status after the outcome; atomic claim for every item type; retries and recovery as Go; SDK
      outbox dispatch jobs carry ids (`feat/go-outbox`)
- [x] Router delivery semantics: in-pipeline deferred retry; group FIFO; panic slot leak (`feat/go-conformance`)
- [x] Router lifecycle: NATS defaults, config reload, shutdown order, consumer rebuild, watchdog, pool update
      in place, reaper, leader timeouts, HTTP-before-leadership (`feat/go-router-life`)

## Platform contract
- [x] Domain event type names and `data` match Go (about 40 differ) (`feat/go-events`)
- [x] `aws-sm://` secret references resolved as Go; backfill skips references (`feat/go-events`)
- [x] Read endpoints enforce Go's read permissions (many only require a login today) (`feat/go-authz`;
      `docs/parity/read-permissions-vs-go.md`)
- [x] Route-auth guardrail: a convention test that every `/api` and `/bff` route authenticates unless
      allowlisted (`feat/go-authz`: `tests/it/route_auth_convention_test.rs`)
- [x] `/auth/me` returns effective permissions plus scope/tier; the SPA hides nav items the user can't use
      (decision #8) (`feat/go-authz`)
- [x] Missing Go routes: `connections/sync`, `docs/sync`, `POST /api/processes/sync`, `router-config`,
      `/auth/password-setup/request`, 2FA/TOTP, reset-2fa, developer credentials
- [x] Behaviour behind the `client-admin` (`feat/go-authz`) and `portal-administrator` (`feat/go-routes-portal`)
      roles
- [x] Go's roleless-user "profile-only" middleware (`feat/go-authz`)

## API convergence (from parity run 1)
- [x] Core (`feat/api-core`): `POST /api/principals`; Go's error envelope and codes; extractor rejections as
      400 VALIDATION; Go's 401/403 rules and headers; `$schema` omitted (#30, provisional); OAuth gaps
      (unknown client, `client_credentials` with no service account, discovery, login backoff, family
      revocation); token claims per #3/#20; `/auth/login` and `/api/me` shapes; passkey gate for INTERNAL IdPs
- [x] Missing routes (`feat/go-routes`): the run-1 list (2FA, change-password, login-history, portal, docs,
      role-permission paths, service-account tokens, config properties, and more)
- [ ] OpenAPI documents (`/q/openapi` and the developer spec) vs Go's huma documents: decide whether they
      must match

## Rulings adopted from the Java session, not yet built
- [x] Router auth (ruling 2), built (`feat/router-auth`): platform bearer tokens verified against the
      platform's JWKS (`fc-platform-jwks`, shared with the function host), `router:view`/`operate`,
      the `platform:router-operator` role, `:view` on `viewer` and `application-service`, dev-only
      mocks, the PKCE dashboard (`FC_ROUTER_DASHBOARD_CLIENT_ID`). Production's `AUTH_MODE=NONE` is
      still honoured, loudly (decision #43), so nothing changes until the owner steps below.
- [ ] **Owner: switch router auth on** (decision #43), in this order:
      1. Release laravel-sdk 0.10.27 (and the TS SDK) with the router bearer; move integral, hr
         (from `^0.8` to `^0.10`) and rfp to it and deploy them. Their service accounts get
         `router:view` through `platform:application-service` once the Rust platform is deployed
         (the code-role sync applies it on start); a Go platform does not grant it.
      2. Assign `platform:router-operator` to the people and service accounts that operate the
         router (super-admins already hold it).
      3. Dashboard sign-in: `POST /api/oauth-clients` with `clientType: PUBLIC`,
         `grantTypes: ["authorization_code"]`, `pkceRequired: true`, `apiAccess: true`,
         `applicationIds: [<the platform application's id>]`,
         `defaultScopes: ["platform:messaging:router:view", "platform:messaging:router:operate"]`,
         `redirectUris: ["https://{routerDomain}/router/dashboard.html",
         "https://{routerDomain}/router/monitoring/dashboard"]`; put its client id on the router task
         as `FC_ROUTER_DASHBOARD_CLIENT_ID`.
      4. IaC (`inhance/iac/compute/fc-router.ts`): remove `AUTH_MODE=NONE`; keep
         `FC_ROUTER_PLATFORM_URL=http://fc-platform:8080` (tokens are verified against its discovery
         and `/.well-known/jwks.json`). After the deploy: the router logs "Router API
         authentication: platform bearer tokens", `/router/health` carries no `authWarning`, and an
         unauthenticated `GET /router/monitoring/pools` answers 401.
      5. Code: delete decision #43's `NONE` exception (`platform_auth::resolve`), so `NONE` is dev-only
         as the ruling says.
- [x] SDKs: single-flight refresh (ruling 5), webhook `check()` (ruling 11), bearer on router calls; licences as published (TS Apache-2.0, Laravel MIT, Go/Rust Apache-2.0)
- [x] SPA and SDKs: `allApplications` on service-account create; `passwordHashIgnored` in sync results (Java SDK: not done)

## Already done
- JWT claims in Go's shape (#3, #20)
- App compatibility items (#4)
- Cron dialect (#1)
- Security sweep S1–S15 and IAM authority (#21–#26)
- Go session cookie (#26)
- Batch ingest statuses and limits
- Listener timeouts (ruling 10)

## Deployment contract (the Rust images must run with the production task definitions unchanged)
- [x] Router: production runs **Go's `fc-server` in router-only role** (`inhance/iac/compute/fc-router.ts`).
      Rust must honour the same env: role toggles, comma-separated `FLOWCATALYST_CONFIG_URL` (the
      platform's router-config plus integral's `/api/config`), `FC_ROUTER_PLATFORM_URL` with
      client-credentials, settle reporting, notifications (`feat/router-env`)
- [x] Platform and worker tasks (`flowcatalyst.ts`): DB secret provider and ARN, JWT current and
      previous keys, app key, SMTP, WebAuthn, OIDC TTLs, Redis, subsystem toggles, health checks
      (`feat/platform-env`)
- [x] integral's create-user invite flags on `/api/principals/users` (`--invite-link`,
      `--invite-redirect-uri`) (`feat/go-routes` follow-up)
- [ ] IaC hygiene (owner): the router task definition holds the Teams webhook `sig=` in plain text;
      move it to SSM

## Known inherited defects (same in Go; not cutover blockers)
- Go's SPA redirects an already-signed-in OIDC interaction to `/oidc/interaction/{uid}/login`, which no
  backend serves (Go or Rust): `frontend/src/api/auth.ts`, `router/guards.ts`. Fix in the SPA (point it at
  `/auth/oidc/interaction/{uid}/…`) once the interaction flow is exercised.

## Developer machines (fcdev)
- [x] Rust `fc-dev` opens the embedded cluster Go's and Java's `fcdev` share
      (`<userDataDir>/flowcatalyst/embedded-pg`, port 15432, `postgres`/`postgres`, database `flowcatalyst`,
      PG 18 pinned), with their `app-key`, JWT signing key and PID file; `fc-dev stop`; PostGIS mirrored from
      Go's tree; one instance at a time (`feat/fcdev-shared-db`, `docs/developers/fc-dev.md`)
- [x] Rust migrations verified on a copy of a Go/Java-migrated dev cluster: four compatible DDL changes, no
      rows deleted (`docs/developers/fc-dev.md`, "Migrations on a database Go and Java migrated")
- [ ] Owner: stop Go/Java `fcdev` before the first `fc-dev` start; the old Rust-only cluster
      (`~/Library/Caches/flowcatalyst-dev/pgdata`) can be deleted once nothing in it is needed
- [x] Built-in role catalogue: `platform:admin:config:manage` (owner decision #44); built-in roles grant it,
      and a stored role still holding Go's `…:config:update` is honoured as `manage` (no data rewrite)

## Also landed
- Go's production SPA replaces the old Vue frontend (functions UI re-integrated in Go's idiom).
- Topcoat UI trial (`crates/fc-web`) on main behind `fc-dev --features web`; excluded from default builds;
  removal recipe in `docs/topcoat-trial.md`.
- Delivery harness run 4: 16 PASS, 1 ACCEPTED (Go defect, #31).
- Dispatch-pool writes gated by Go's `CanWriteDispatchPools` (was client reach only).

## Before deploy (owner)
- [ ] Run `docs/fc-predeploy-checks.sql` (tenant pins, cross-app role permissions, OAuth clients on non-service
      principals, service accounts' batch permissions, service accounts not tied to an application)
- [ ] Same RSA signing key, issuer and `FLOWCATALYST_APP_KEY` as Go
- [ ] App service accounts hold `platform:messaging:batch:events-write` (and `dispatch-jobs-write`)
- [x] Prod env names read by Rust: all three task definitions run unchanged, no IaC change.
      - Platform and worker tasks (`flowcatalyst.ts`): `feat/platform-env`, `docs/parity/platform-env-vs-go.md`.
        Key continuity: keep the SSM `jwt-private-key`, `jwt-previous-public-key` and `app-key` and
        `EXTERNAL_BASE_URL` as they are. Rust derives Go's public key and `kid`, so Go-issued tokens and stored
        secrets keep working.
      - Router task (`fc-router.ts`): `feat/router-env`, `docs/parity/router-env-vs-go.md`. Deploy the main
        `Dockerfile` image (`fc-server`, linux/arm64) to `inhance/fc-router`. Confirm note 1 there (Teams alerts
        start arriving).
- [ ] Rotate the leaked passwords; click Redact after the deploy
- [ ] SDK cutover steps in `docs/sdks.md`

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
- [ ] API parity converged: every diff fixed or ruled

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
      allowlisted (`feat/go-authz`: `tests/route_auth_convention_test.rs`)
- [x] `/auth/me` returns effective permissions plus scope/tier; the SPA hides nav items the user can't use
      (decision #8) (`feat/go-authz`)
- [ ] Missing Go routes: `connections/sync`, `docs/sync`, `POST /api/processes/sync`, `router-config`,
      `/auth/password-setup/request`, 2FA/TOTP, reset-2fa, developer credentials
- [ ] Behaviour behind the `client-admin` (`feat/go-authz`) and `portal-administrator` (`feat/go-routes-portal`)
      roles
- [x] Go's roleless-user "profile-only" middleware (`feat/go-authz`)

## API convergence (from parity run 1)
- [ ] Core (`feat/api-core`): `POST /api/principals`; Go's error envelope and codes; extractor rejections as
      400 VALIDATION; Go's 401/403 rules and headers; `$schema` omitted (#30, provisional); OAuth gaps
      (unknown client, `client_credentials` with no service account, discovery, login backoff, family
      revocation); token claims per #3/#20; `/auth/login` and `/api/me` shapes; passkey gate for INTERNAL IdPs
- [ ] Missing routes (`feat/go-routes`): the run-1 list (2FA, change-password, login-history, portal, docs,
      role-permission paths, service-account tokens, config properties, and more)
- [ ] Per-area pass: write status codes (201/204), idempotent no-op repeats, Go's input validation, list
      envelopes, null vs absent members, login-attempt fields, audit facet names
- [ ] OpenAPI documents (`/q/openapi` and the developer spec) vs Go's huma documents: decide whether they
      must match

## Rulings adopted from the Java session, not yet built
- [ ] Router auth (ruling 2): platform bearer tokens, `router:view`/`operate`, the `router-operator` role,
      dev-only mocks, the PKCE dashboard. **The SDK releases that send the bearer must reach
      integral/hr/rfp first** (laravel-sdk 0.10.27; hr is pinned to `^0.8` and must move to `^0.10`).
      Rust's role catalogue doesn't have `router:view`/`operate` yet: add them, and grant `:view` to
      `application-service` and `viewer`, when router auth is built.
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
- [ ] Router: production runs **Go's `fc-server` in router-only role** (`inhance/iac/compute/fc-router.ts`).
      Rust must honour the same env: role toggles, comma-separated `FLOWCATALYST_CONFIG_URL` (the
      platform's router-config plus integral's `/api/config`), `FC_ROUTER_PLATFORM_URL` with
      client-credentials, settle reporting, notifications (`feat/router-env`)
- [ ] Platform and worker tasks (`flowcatalyst.ts`): DB secret provider and ARN, JWT current and
      previous keys, app key, SMTP, WebAuthn, OIDC TTLs, Redis, subsystem toggles, health checks
      (`feat/platform-env`)
- [ ] integral's create-user invite flags on `/api/principals/users` (`--invite-link`,
      `--invite-redirect-uri`) (`feat/go-routes` follow-up)
- [ ] IaC hygiene (owner): the router task definition holds the Teams webhook `sig=` in plain text;
      move it to SSM

## Known inherited defects (same in Go; not cutover blockers)
- Go's SPA redirects an already-signed-in OIDC interaction to `/oidc/interaction/{uid}/login`, which no
  backend serves (Go or Rust): `frontend/src/api/auth.ts`, `router/guards.ts`. Fix in the SPA (point it at
  `/auth/oidc/interaction/{uid}/…`) once the interaction flow is exercised.

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

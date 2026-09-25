# Go → Rust cutover checklist

The goal (owner, 2026-09-25): Rust is a **drop-in replacement for Go** in production. Go is the reference for
existing behaviour, Java for functions, and owner rulings override both
(`docs/owner-decisions-2026-09-25.md`). This file tracks what stands between today and a cutover. Tick items off
as they merge to `main`.

## Correctness gates (decisions #28, #29)
- [ ] Mediation conformance corpus: a Rust runner, all rows passing, and the deviations from Go listed in
      `docs/parity/router-deviations-from-go.md` (`feat/go-conformance`)
- [ ] Delivery parity harness, Go vs Rust end to end over SQS: scenarios pass or are ruled
      (`feat/harness-delivery`)
- [ ] API parity runner, Go vs Rust on Java's 45 scenario files: diffs fixed or ruled (`feat/harness-parity`)

## Message pipeline (`docs/reviews/message-pipeline-review-2026-09-25.md`)
- [ ] Scheduler publishes to SQS; jobs are inserted PENDING; Go's claim/hold/backoff model; `/process`
      authenticated, claimed, ACK semantics as Go; fan-out cache guard (`feat/go-scheduler`)
- [ ] Outbox: status after the outcome; atomic claim for every item type; retries and recovery as Go; SDK
      outbox dispatch jobs carry ids (`feat/go-outbox`)
- [ ] Router delivery semantics: in-pipeline deferred retry; group FIFO; panic slot leak (`feat/go-conformance`)
- [ ] Router lifecycle: NATS defaults, config reload, shutdown order, consumer rebuild, watchdog, pool update
      in place, reaper, leader timeouts, HTTP-before-leadership (`feat/go-router-life`)

## Platform contract
- [ ] Domain event type names and `data` match Go (about 40 differ) (`feat/go-events`)
- [ ] `aws-sm://` secret references resolved as Go; backfill skips references (`feat/go-events`)
- [ ] Read endpoints enforce Go's read permissions (many only require a login today)
- [ ] Route-auth guardrail: a convention test that every `/api` and `/bff` route authenticates unless
      allowlisted
- [ ] `/auth/me` returns effective permissions plus scope/tier; the SPA hides nav items the user can't use
      (decision #8)
- [ ] Missing Go routes: `connections/sync`, `docs/sync`, `POST /api/processes/sync`, `router-config`,
      `/auth/password-setup/request`, 2FA/TOTP, reset-2fa, developer credentials
- [ ] Behaviour behind the `client-admin` and `portal-administrator` roles
- [ ] Go's roleless-user "profile-only" middleware

## Rulings adopted from the Java session, not yet built
- [ ] Router auth (ruling 2): platform bearer tokens, `router:view`/`operate`, the `router-operator` role,
      dev-only mocks, the PKCE dashboard. **The SDK releases that send the bearer must reach
      integral/hr/rfp first.**
- [ ] TS and Laravel SDKs: single-flight refresh (ruling 5) and webhook `check()` (ruling 11)
- [ ] SPA and SDKs: `allApplications` on service-account create; `passwordHashIgnored` in sync results

## Already done
- JWT claims in Go's shape (#3, #20)
- App compatibility items (#4)
- Cron dialect (#1)
- Security sweep S1–S15 and IAM authority (#21–#26)
- Go session cookie (#26)
- Batch ingest statuses and limits
- Listener timeouts (ruling 10)

## Before deploy (owner)
- [ ] Run `docs/fc-predeploy-checks.sql` (tenant pins, cross-app role permissions, OAuth clients on non-service
      principals, service accounts' batch permissions, service accounts not tied to an application)
- [ ] Same RSA signing key, issuer and `FLOWCATALYST_APP_KEY` as Go
- [ ] App service accounts hold `platform:messaging:batch:events-write` (and `dispatch-jobs-write`)
- [ ] Prod env names read by Rust (see `inhance/iac/compute/flowcatalyst.ts`, `fc-router.ts`)
- [ ] Rotate the leaked passwords; click Redact after the deploy
- [ ] SDK cutover steps in `docs/sdks.md`

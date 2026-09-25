# Platform and worker task environment: Go vs Rust

The production platform and worker tasks (`inhance/iac/compute/flowcatalyst.ts`) run the `fc-server` image with
the environment below. Rust must run with **exactly that environment, unchanged** (owner decision, 2026-09-25:
Rust is a drop-in for Go). This file lists every variable and secret in the two task definitions: what Go does
with it, what Rust did before `feat/platform-env`, and what Rust does now.

References: Go `internal/server/envcfg.go` (`LoadEnv`), `dbsecret.go`, `signing_key.go`, `subsystems.go`,
`wire_services.go`, `internal/platform/shared/{email,encryption}`; Rust `bin/fc-server/src/main.rs`,
`fc_platform::shared::{database, email_service, encryption_service, server_setup::auth_init}`,
`fc_platform::auth::signing_keys`, `fc_platform::webauthn::webauthn_service`.

Proof: `bin/fc-server/tests/prod_env_boot_test.rs` (Docker, `--ignored`) boots the real binary twice, once with the
platform task's environment and once with the worker's, against Postgres. The database credentials come through
`DB_SECRET_PROVIDER`/`DB_SECRET_ARN`/`DB_HOST`/`DB_NAME` from a fake Secrets Manager (`AWS_ENDPOINT_URL`, no AWS
call). The JWT keys are passed as SSM passes them (literal `\n`). The test then:
- checks `/health`, a password login, and JWKS with Go's key ids;
- validates tokens signed with the previous key;
- rotates the DB password and kills every connection, and the platform keeps answering;
- checks that the worker serves only `/health` and reports its subsystems.

## Shared environment (`sharedEnv`)

| Variable | Go (meaning, default) | Rust before | Rust now |
|---|---|---|---|
| `RUST_LOG` | Not read. Go logs JSON to stderr, level `FC_LOG_LEVEL` (default info) | Level filter; text logs with ANSI colours unless `LOG_FORMAT=json` | Level filter. `fc-server` logs **JSON by default**, as Go does (`LOG_FORMAT=text` opts out); `FC_LOG_LEVEL` is honoured when `RUST_LOG` is unset |
| `DB_SECRET_PROVIDER` | Must be `aws` (default); any other value is a startup error | Only `aws` did anything; another value silently fell through to explicit credentials | As Go: `aws` (any case) or a startup error |
| `DB_SECRET_ARN` | With `DB_HOST`, and no `FC_DATABASE_URL`/`DATABASE_URL`: read `{username,password,port?}` from Secrets Manager. The region comes from the ARN. Credentials are re-read every `DB_SECRET_REFRESH_INTERVAL_MS` (default 5 min, `<=0` off) and injected into new connections | Same precedence and refresh, but the region came from the SDK default chain (not the ARN), a new AWS client was built on every poll, and `-1` was not "off" | As Go: the region is taken from the ARN, the client is cached, empty username/password is refused, `<=0` disables the refresh. **Every pool registers the refresh**: the platform pool, and the stream processor's own pool (the scheduler and scheduled jobs share the platform pool). The SDK endpoint override (`AWS_ENDPOINT_URL[_SECRETS_MANAGER]`) is honoured, which is how the test fakes it |
| `DB_HOST` | Host (`host:port` allowed). Unset means `postgresql://postgres@localhost:5432/flowcatalyst` | Unset was a startup error | As Go |
| `DB_NAME` | Database name, default `flowcatalyst` | Same | Same |
| (`DB_PORT`) | Not set in prod. Default 5432; the secret's `port` wins | Same | Same |
| `REDIS_URL` | Alias of `FC_STANDBY_REDIS_URL`: leader election, **only when standby is enabled** (off in both tasks). Go's rate limiter and principal-version cache read `FC_REDIS_URL`, not this | Alias of `FC_STANDBY_REDIS_URL`; the redis client had **no TLS**, so a `rediss://` URL could not connect | Same alias. The redis client is built with rustls (`rediss://` works) and `fc-server` installs a process-wide rustls provider. Unused in prod while `STANDBY_ENABLED=false` |
| `DISPATCH_QUEUE_TYPE` | Alias of `FC_DISPATCH_QUEUE_TYPE`. `SQS` gives per-tenant SQS FIFO queues; anything else, including blank, gives the Postgres broker | Same names. Blank is refused (deliberate, `feat/go-scheduler`) | Unchanged (prod sets `SQS`) |
| `DISPATCH_QUEUE_URL` | Alias. Read only to derive the account and region | Same | Same |
| `DISPATCH_QUEUE_REGION` | Alias. Overrides the URL's region | Same | Same |
| `FC_DISPATCH_QUEUE_PREFIX` | `FC-{env}` queue-name prefix. Required with SQS | Same | Same |

In Go, the dispatch queue settings are resolved at boot by every role, and they also feed the `router-config`
document the platform serves. That route is tracked in `docs/cutover-checklist.md` (missing Go routes). In Rust,
the scheduler resolves them when it starts.

## Shared secrets (`sharedSecrets`, SSM)

| Secret | Go (meaning) | Rust before | Rust now |
|---|---|---|---|
| `FLOWCATALYST_APP_KEY` | Base64 32-byte AES-256-GCM key for stored secrets (`FLOWCATALYST_APP_KEY_PREVIOUS` for rotation). Also the input of HKDF-SHA256 (`info=fc-dispatch-auth`) for the dispatch-token HMAC. Unset: encryption disabled and the scheduler refuses to start. **Malformed: fatal at boot** (owner ruling 2026-09-08) | Same key format and the same HKDF (byte-identical tokens across a mixed Go/Rust deployment). A malformed key only logged a warning, and the platform ran with encryption disabled | As Go: a malformed key (current or previous) stops the boot |
| `FLOWCATALYST_JWT_PRIVATE_KEY` | RS256 signing key, inline PEM (PKCS#1 or #8), normalised (literal `\n`, quotes, whole-PEM base64). Public key **derived** from it. `kid` = base64url(sha256(derived PKIX PEM)[:16]). Also `FC_JWT_SIGNING_KEY_PATH` / `FC_JWT_SIGNING_KEY_PEM`. A bad key fails the boot | **Broken for prod.** The key was used only if `FLOWCATALYST_JWT_PUBLIC_KEY` was *also* set. Prod sets only the private key, so Rust generated a **fresh key pair** per task: every Go-issued token failed, and replicas rejected each other's tokens. No PEM normalisation. A bad key fell back to HS256 with an **empty secret** | As Go: read from Go's names, normalised, public key derived, byte-identical PEM so **the same `kid`** (unit-tested against Go's `pem.EncodeToMemory` layout). A bad key fails the boot (no HS256 fallback in `init_auth_services`) |
| `FLOWCATALYST_JWT_PREVIOUS_PUBLIC_KEY` | Validation-only previous key (rotation), normalised. A non-PEM value (an SSM placeholder) is ignored; an unparseable PEM fails the boot. Listed in JWKS under `kid` = hash of the normalised text. Go checks it in `authservice.ValidateToken` (`/oauth/userinfo`, `/oauth/introspect`) | **Not read.** Rust looked for `FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS` | Go's name first (the old name still works), normalised, SPKI or PKCS#1, the same `kid`. Tokens under it validate on userinfo/introspect **and** on API bearers and session cookies (Rust accepts the previous key on every validation path; Go's bearer/cookie middleware checks only the current key, so Rust is a superset) |

## Platform task

| Variable | Go | Rust before | Rust now |
|---|---|---|---|
| `PORT` | Alias of `FC_API_PORT`, default **8080** | Alias, default 3000 | Default 8080 (the worker task sets no `PORT` and listens on 8080, as Go) |
| (`FC_METRICS_PORT`) | 9090: `/health`, `/ready`, `/metrics` | 9090 | 9090. `/ready` now answers Go's shape (`status: ready` plus subsystem flags) |
| `PLATFORM_ENABLED` | `FC_PLATFORM_ENABLED` alias, default true. Serves the API and the embedded SPA; runs the purger and the A-01 reaper | Same alias | Same. Toggles parse as Go's `envBool` (`1/true/yes/on`, `0/false/no/off`, any case, else the default; a set primary name wins over its alias). The lapsed-secret purge and the rate-limit prune now run only where the platform runs |
| `STREAM_PROCESSOR_ENABLED` | Alias of `FC_STREAM_PROCESSOR_ENABLED`, default false | Same | Same |
| `DISPATCH_SCHEDULER_ENABLED` | Alias of `FC_SCHEDULER_ENABLED`, default false. **Only** the dispatch-job scheduler | Same alias, but it **also** started the scheduled-job cron engine | Only the dispatch scheduler (see `FC_SCHEDULED_JOB_ENABLED`) |
| `MESSAGE_ROUTER_ENABLED` | Alias of `FC_ROUTER_ENABLED`, default false | Same | Same (router wiring is `feat/router-env`'s; this branch only switched the line to the shared toggle parser) |
| `STANDBY_ENABLED` | Alias of `FC_STANDBY_ENABLED`, default false | Same | Same |
| `EXTERNAL_BASE_URL` | Issuer **and** audience of every JWT (`FC_JWT_ISSUER` > `FC_EXTERNAL_BASE_URL` > this), OIDC discovery, password-reset links. Default `http://localhost:8080` | Same chain; default `http://localhost:3000` | Same chain; default `http://localhost:8080`. **Must equal Go's value** or Go-issued tokens fail the issuer check |
| `OIDC_ACCESS_TOKEN_TTL` | Access-token lifetime (also `FC_JWT_ACCESS_TOKEN_TTL_SECS`), default 1h; non-positive means default | Honoured (as `FC_ACCESS_TOKEN_EXPIRY_SECS` alias) | Also `FC_JWT_ACCESS_TOKEN_TTL_SECS`; non-positive means default |
| `OIDC_SESSION_TTL` | Session JWT **and** cookie lifetime, default 24h | Session JWT only; the cookie's Max-Age stayed 24h (8h JWT inside a 24h cookie in prod) | Both, as Go |
| `OIDC_REFRESH_TOKEN_TTL` | Lifetime stamped on a new refresh token; the family's absolute cap (rotation carries it forward). Default 7 days | **Read but unused**: refresh tokens were hard-coded to 30 days | Applied (process-wide, set at boot); default 7 days as Go. Prod's `2592000` (30 d) behaves as before |
| `FC_WEBAUTHN_RP_ID` | Relying-party id, default `localhost` | Honoured; default `auth.flowcatalyst.io` | Honoured; default `localhost` |
| `FC_WEBAUTHN_ORIGINS` | Comma-separated exact origins, then legacy `FC_WEBAUTHN_RP_ORIGIN`, then `http://localhost:8080` | Honoured; fallback `https://{rp_id}` | As Go |
| `DISPATCH_SCHEDULER_PROCESSING_ENDPOINT` | Alias of `FC_DISPATCH_PROCESSING_ENDPOINT`: the callback URL the scheduler stamps into each message. Default `http://localhost:{port}/api/dispatch/process`. Read by whichever task runs the scheduler | Same | Same |
| `SMTP_HOST` | `FC_SMTP_HOST` > `SMTP_HOST`; unset means log-only | Same | Same (blank counts as unset) |
| `SMTP_PORT` | Default 587 | Same | Same |
| `SMTP_SECURE` | `true/1/yes/on`: implicit TLS; otherwise STARTTLS (net/smtp upgrades when the server offers it, as SendGrid :587 does) | `true`/`1`: implicit TLS, else required STARTTLS. The send **blocked a runtime thread** | Go's truthy set; STARTTLS required (credentials never in clear); async send |
| `SMTP_USERNAME` | AUTH only when non-empty | Always sent credentials | AUTH only when non-empty |
| `SMTP_FROM` | Default `noreply@flowcatalyst.local` | Same | Same |
| `SMTP_PASSWORD` (secret) | AUTH password | Same | Same |
| (the SPA) | Embedded in the binary, served when the platform is enabled | Served only when `FC_STATIC_DIR` was set, and **no task sets it**: prod would have served no SPA | `FC_STATIC_DIR`, else the image's `/app/frontend/dist` (Dockerfile) when it holds `index.html`; platform only |

## Worker task

| Variable | Go | Rust before | Rust now |
|---|---|---|---|
| `PLATFORM_ENABLED=false` | No API or SPA: the API listener serves only `/health` on 8080. The database is still connected, migrated and seeded | `/health` only, but on **3000** (no `PORT` in the worker task) | `/health` only, on 8080 |
| `STREAM_PROCESSOR_ENABLED=false`, `MESSAGE_ROUTER_ENABLED=false`, `STANDBY_ENABLED=false` | Off | Off | Off |
| `DISPATCH_SCHEDULER_ENABLED=true` | Dispatch scheduler: publishes to SQS, needs `FLOWCATALYST_APP_KEY` | On (and scheduled jobs with it) | On |
| `FC_SCHEDULED_JOB_ENABLED=true` | Scheduled-job cron engine (alias `SCHEDULED_JOB_SCHEDULER_ENABLED`), default false | **Not read** (the engine rode on the dispatch toggle) | Read, as Go |
| `DISPATCH_SCHEDULER_PROCESSING_ENDPOINT` | Stamped into each message; the router calls the **platform** there | Same | Same |

What the worker serves: Go's worker answers `GET /health` on 8080 and `/health`, `/ready`, `/metrics` on 9090. It
serves no processing endpoint of its own: `/api/dispatch/process` lives on the platform task, which the router
reaches at `http://fc-platform:8080` (Service Connect). The worker task definition declares no container health
check and no target group. The platform's ALB target group checks `GET /health` on the traffic port (8080).

## Environment ECS injects

`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI` (the task role) is read by the AWS SDK credential chain in both. Neither
task sets `AWS_REGION`, so both read the region from the ARN (Secrets Manager) and from `DISPATCH_QUEUE_REGION`
(SQS). Rust previously relied on the SDK region chain for Secrets Manager, which in bridge networking depends on
IMDS reachability.

## Not in the task definitions (noted, unchanged)

`FC_OUTBOX_*` (Go's outbox uses the shared pool when `postgres`; Rust needs `FC_OUTBOX_DB_URL`),
`FC_STREAM_BATCH_SIZE`/`FC_STREAM_PARTITION_*` tuning, `FC_PRINCIPAL_VERSION_CACHE_*`, `FC_RL_*`, `FC_REDIS_URL`
(both read it for rate limits), `FC_MCP_*`, `FC_ALB_*`, `FC_AUTH_ALLOW_TEST_HEADERS`. None is set in production.

## IaC changes needed for Rust

**None.** Both task definitions run unchanged. One comment in `flowcatalyst.ts` is outdated: it says the Rust
binary "accepts both FC_* and TS-style names" and that the worker is "Go fc-server". Both statements now describe
the same image.

## Cutover steps for secrets and keys

1. **Keep every SSM parameter as it is.** `app-key`, `jwt-private-key`, `jwt-previous-public-key` and
   `smtp_password` are read under the same names and with the same normalisation. Rust derives the same public
   key and the same `kid` from `jwt-private-key`, so Go-issued access tokens, refresh tokens (same
   `oauth_oidc_payloads` rows) and session cookies keep validating. JWKS consumers see the same key ids.
2. **Keep `EXTERNAL_BASE_URL` as it is** (the issuer and audience of every token).
3. **Keep `app-key`.** Stored secrets decrypt, and dispatch tokens minted by a Go worker validate on a Rust
   platform, or the reverse, during a rolling cutover (same HKDF).
4. **The RDS master secret needs nothing.** Rust reads it from the ARN's region with the task role, and follows
   rotations on every pool (5-minute poll).
5. Rotate the JWT key **after** the cutover has settled, not during it:
   - put the current public key into `jwt-previous-public-key`;
   - put the new private key into `jwt-private-key`;
   - restart both tasks;
   - once the longest token lifetime has passed (`OIDC_REFRESH_TOKEN_TTL`, 30 days in prod), set
     `jwt-previous-public-key` back to a placeholder.

   A non-PEM placeholder is ignored. A malformed PEM stops the boot, as in Go.
6. A malformed `app-key` or JWT key now stops the task at boot (as Go). Check the task's first log lines after
   deploy.

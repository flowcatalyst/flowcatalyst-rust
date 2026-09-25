# Router deployment contract: Rust vs Go (2026-09-25)

Production's message router is **Go's `fc-server` in its router role**, deployed by
`inhance/iac/compute/fc-router.ts`. For Rust to replace it, a Rust image must run under **that task
definition, unchanged**. This page maps every variable the task sets, and every variable Go's router
reads, to what Rust did before branch `feat/router-env` and what it does now.

Go references are to `../flowcatalyst-go`: `internal/server/envcfg.go` (`LoadEnv`),
`internal/server/run.go` (`Run`, `newRouterServer`, `resolveRouterAuth`), `cmd/fc-server/main.go`,
`internal/router/{config_sync,settled,server,notification}.go`, `internal/oauthtoken/token.go` and
`internal/logging/logging.go`.

## Decision: which image the router task runs

**The main `Dockerfile` image (`fc-server`), in its router role.** This is how Go works: one
`fc-server` image serves every task, and `MESSAGE_ROUTER_ENABLED=true` with `PLATFORM_ENABLED=false`
selects the router. Rust's `fc-server` now does the same:

- **No database.** Postgres is connected, migrated and seeded only when a database-backed subsystem
  runs (platform, stream, scheduler or outbox: Go's `needsDB`). The router task sets no database
  variable.
- **Shared router.** It runs the same router runtime (`fc_router::bootstrap::RouterRuntime`) as the
  standalone binary.
- **HTTP.** The router's surface is under `FC_ROUTER_HTTP_PREFIX` (default `/router`). `/health` at
  the root is Go's `{"status":"UP","version":…}`, always 200: that is the path the ALB target group
  probes on 8080.
- **Metrics listener.** It listens on `FC_METRICS_PORT` (9090), with `/health`, `/ready` and
  `/metrics`, as Go.

The standalone `fc-router` binary (`Dockerfile.router`) accepts the same environment through the same
runtime. Its surface is at the root. It remains for self-hosting; production doesn't need it.

**Mapping to the task definition.** Build the main `Dockerfile` for **`linux/arm64`** (the task's
`runtimePlatform` is ARM64). Push it to the router's ECR repository, `inhance/fc-router`, under the
stack's `routerImageTag`. The platform and worker tasks take the same image from
`inhance/flowcatalyst`. Nothing else changes: container port 8080, health path `/health`, the
Service Connect name `http`, the 512 MB hard limit, the SQS task-role actions (the Rust consumer calls
only `ReceiveMessage`, `DeleteMessage`, `ChangeMessageVisibility`, `GetQueueAttributes` and
`SendMessage`), and the two SSM secrets.

**IaC changes required: none.** The recommendations below are optional.

## A. Every variable the router task definition sets

Values are omitted; the task holds a webhook URL with a secret in it.

| Variable | Go's meaning | Rust before | Rust now (`fc-server` router role; standalone the same unless noted) |
|---|---|---|---|
| `RUST_LOG` | **Not read.** Go logs JSON at `FC_LOG_LEVEL`, default info. | Log filter | Log filter (`info` = Go's default level). When unset, Go's `FC_LOG_LEVEL` is now honoured. See note 3 on log format. |
| `API_PORT` | **Not read.** Go listens on `FC_API_PORT`, else `PORT`, else **8080**; the task's 8080 is the default anyway. | `fc-server`: not read, **default 3000** (the ALB would never see it). Standalone: read. | `fc-server`: not read, as Go; the default is now **8080**. The standalone binary still reads it after `FC_API_PORT` (its own historical name). |
| `AWS_REGION` | The AWS SDK's region | The AWS SDK's region | Unchanged |
| `MESSAGE_ROUTER_ENABLED` | Alias of `FC_ROUTER_ENABLED`, default off. Go's `envBool` table: `1/true/yes/on`, `0/false/no/off`, case-insensitive; anything else is the default. | `fc-server`: read, but only `true`/`1` counted. Standalone: ignored. | `fc-server`: Go's truth table (`fc_common::config::env_first_bool_go`). Standalone: ignored (it is always a router). |
| `PLATFORM_ENABLED` | Alias of `FC_PLATFORM_ENABLED`, default **on**. `false` stops it booting as the platform. | `fc-server`: read, but **connected, migrated and seeded Postgres anyway**, and failed at boot without DB variables. So Rust could not run this task at all. | Go's truth table. `false` with no other database subsystem means no Postgres connection. |
| `FLOWCATALYST_CONFIG_URL` | Comma-separated config sources, fetched in parallel. Merge is first-wins (pools by code, queues by URI). Each source is retried 12 × 5s; permanent refusals are not retried. A failing source serves its last good document. The watcher retries at 5s until a config lands, then polls. Unset: no queues and no pools. | `fc-server`: **one initial fetch, never polled again**; the router didn't start if it failed; no credentials. Standalone: Go's merge, last-known-good and boot retry, but no credentials, permanent errors retried, and a 30s timeout. Unset was fatal. | Go's semantics throughout, section C. Unset: runs with no queues and no pools, as Go. |
| `FLOWCATALYST_CONFIG_INTERVAL` | Alias of `FC_ROUTER_CONFIG_INTERVAL_SECONDS`. `0`, unset or unparseable means 300s. | `fc-server`: ignored. Standalone: read, but `0` panicked the ticker. | Go: primary name first, then the alias. Non-positive means 300s. |
| `FC_ROUTER_PLATFORM_URL` | The platform the router belongs to. Aliases `FC_API_BASE_URL`, `FLOWCATALYST_URL`. It names the only origin the credential may go to, and turns on settle reporting. | Ignored | As Go, sections D and E |
| `FLOWCATALYST_STANDBY_ENABLED` | **Not read.** Go reads `FC_STANDBY_ENABLED`, else `STANDBY_ENABLED`, default off. | `fc-server`: ignored. Standalone: read. | `fc-server`: ignored, as Go. It uses its own leader election (`FC_STANDBY_ENABLED` / `STANDBY_ENABLED`). Standalone: read as a fallback after Go's names. The task's `false` means no standby either way. |
| `AUTH_MODE` | `NONE` (trimmed, case-insensitive) turns off the router surface's basic auth, whatever credentials are set. | `NONE`/`BASIC`/`OIDC`/`OIDC_FLOW`, not trimmed; `fc-server` had no router surface. | The same modes, now trimmed. `NONE` leaves `/router/*` open, as Go. |
| `NOTIFICATION_TEAMS_ENABLED` | **Not read.** | Standalone: read (it could only widen). `fc-server`: no notifications at all. | **Deviation, note 1:** gates the legacy webhook URL. Only an explicit `false` turns it off. |
| `NOTIFICATION_TEAMS_WEBHOOK_URL` | **Not read.** Go notifies only to `FC_NOTIFY_WEBHOOK_URL`, so Go's production router sends **no** Teams notifications today. | Standalone: the Teams webhook. `fc-server`: none. | **Deviation, note 1:** the Teams webhook, after `FC_NOTIFY_WEBHOOK_URL`. |
| `NOTIFICATION_MIN_SEVERITY` | **Not read.** Go reads `FC_NOTIFY_MIN_SEVERITY`: an unknown value keeps WARNING. | Standalone: read, with a deprecation warning | Read after `FC_NOTIFY_MIN_SEVERITY`. An unknown value keeps WARNING, as Go. The task's `WARNING` is Go's default anyway. |
| `NOTIFICATION_BATCH_INTERVAL` | Alias of `FC_NOTIFY_BATCH_INTERVAL_SECONDS`. `0` or unset means 300s. | Standalone: read, but `0` turned batching off. | As Go |
| `FC_ROUTER_CLIENT_ID` (SSM secret) | The platform OAuth client. It must come with the secret and with `FC_ROUTER_PLATFORM_URL`, or the router refuses to start. | Ignored | As Go, section D. The secret is never logged or printed (`Debug` is redacted). |
| `FC_ROUTER_CLIENT_SECRET` (SSM secret) | As above | Ignored | As Go |

## B. Other variables Go's router reads (not set by the task)

| Variable | Go | Rust now |
|---|---|---|
| `FC_API_PORT`, `PORT` | API port, default 8080 | Same. `fc-server`'s default was 3000 before. |
| `FC_METRICS_PORT` | Metrics listener, default 9090 | Same in `fc-server`. The standalone binary serves metrics on the API port and ignores this, as before. |
| `FC_ROUTER_ENABLED`, `FC_PLATFORM_ENABLED` | Canonical toggles | Same, with Go's truth table |
| `FC_ROUTER_CONFIG_INTERVAL_SECONDS` | Canonical config interval | Same |
| `FC_API_BASE_URL`, `FLOWCATALYST_URL` | Aliases of `FC_ROUTER_PLATFORM_URL` | Same |
| `FC_NOTIFY_WEBHOOK_URL` | Notification webhook; a URL alone enables it | Same, ahead of the legacy name |
| `FC_NOTIFY_MIN_SEVERITY` | The notification floor | Same |
| `FC_NOTIFY_BATCH_INTERVAL_SECONDS` | Batch interval | Same |
| `FC_ROUTER_HTTP_PREFIX` | Router surface prefix, default `/router` | `fc-server`: same. Standalone: unset means root only, as before. |
| `FC_ROUTER_AUTH_USER` / `_PASS` (`AUTH_BASIC_USERNAME` / `_PASSWORD`) | Basic auth on the router surface | Same (unchanged) |
| `FC_STANDBY_ENABLED` / `STANDBY_ENABLED`, `FC_STANDBY_REDIS_URL` / `REDIS_URL`, `FC_STANDBY_LOCK_KEY` (default `fc:server:leader`) | Leader election | `fc-server`: same. Standalone: same names, plus its `FLOWCATALYST_STANDBY_*` fallbacks and its own default lock key `fc:router:leader`. |
| `FC_DRAIN_TIMEOUT_SECONDS` | Shutdown drain, default 60; `0` means 60 | Same. `fc-server`'s router now drains on shutdown; before, it didn't drain. |
| `FC_ROUTER_STRICT_ROUTING` | R-13/R-16 gate | Same |
| `FC_ROUTER_SYNTH_POOL_IDLE_SECS` | Synthesised-pool idle TTL; `0` means 1h | Same (a negative value disables the sweep) |
| `FC_ROUTER_DEFERRAL_BUDGET` | Deferral budget; `0` means 5000 | Same |
| `FC_ROUTER_DEFERRAL_MAX_DELAY_SECONDS` | Longest capacity deferral, default 1h | **Not honoured.** Rust has no configurable deferral horizon. Not set in production. |
| `FLOWCATALYST_DEV_MODE` | Go: an HTTP/1.1 mediator, nothing else | **Differs, unchanged:** Rust's dev mode swaps in a built-in LocalStack config. Not set in production. |
| `FC_ALB_*` | ALB self-registration | `fc-server` (feature `alb`): `FC_ALB_ENABLED`, `_TARGET_GROUP_ARN`, `_TARGET_ID`, `_TARGET_PORT`. It lacks Go's `FC_ALB_INSTANCE_IP`, `FC_ALB_REGION` and `FC_ALB_DEREGISTRATION_DELAY_SECONDS`. Not used in production (the ECS service registers targets). |
| `FC_LOG_LEVEL` | Log level | Honoured when `RUST_LOG` is unset |

## C. Config sources (`FLOWCATALYST_CONFIG_URL`)

Now as Go's `ConfigSource` and `Watch` (`crates/fc-router/src/config_sync.rs`):

- **Fetch.** The list is comma-separated; blanks are dropped. Every source is fetched in parallel with
  a **10s** request timeout (it was 30s).
- **Retries.** Each source gets up to 12 attempts, 5s apart.
- **Permanent refusals fail the source at once.** These are 403, 404, any other 4xx except
  408/425/429, and a 401 on a request sent without credentials. Only 5xx, 408, 425, 429 and an
  authenticated 401 are retried. (Rust retried everything before, holding every other source's
  config back for a minute.)
- **Credential hints.** An unauthenticated 401 or 403 names the missing
  `FC_ROUTER_PLATFORM_URL` / `FC_ROUTER_CLIENT_ID` / `FC_ROUTER_CLIENT_SECRET` in the error.
- **Merge.** First-wins in URL order: pools by code, queues by URI, with a warning on a conflicting
  duplicate.
- **Keep last good.** A failing source that has succeeded before contributes its cached document,
  with one CONFIGURATION warning per failure streak, resolved on recovery. The fetch fails only when
  every source fails and none has a cache.
- **Change detection.** A config that failed to apply is forgotten, so the next poll applies it
  again.
- **Watch.** At boot the watcher retries every 5s until a config lands, while HTTP is already
  serving. After that it polls every `FLOWCATALYST_CONFIG_INTERVAL`.

## D. Platform credential

`crates/fc-router/src/platform_token.rs` ports `oauthtoken.Manager`:

- **Minting.** A form-encoded `client_credentials` grant (`client_id` and `client_secret` in the
  body) goes to `{FC_ROUTER_PLATFORM_URL}/oauth/token`.
- **Caching.** The token is cached and refreshed 60s before it expires; an absent or zero
  `expires_in` counts as one hour. Concurrent callers share one mint.
- **401.** A 401 on an authenticated fetch drops the token, and the next attempt mints a new one.
- **Where it goes.** The bearer goes **only** to config URLs whose `scheme://host[:port]` origin
  equals the platform URL's origin. Integral's `/api/config` sources never see it.
- **Refusals at startup,** as Go's `newRouterServer`:
  - only one of id and secret set (a blank value counts as unset);
  - credentials without a platform URL.
- **Credentials without a config URL** are ignored, as Go.
- **Deviation.** Go's token request has no timeout of its own. Rust's shares the config client's 10s.

**Role catalogue.** Rust's built-in `platform:router` (`crates/fc-platform/src/role/entity.rs`)
grants exactly `platform:messaging:dispatch-pool:view`, like Go's (`seed/roles.go`). That is what
`GET /api/dispatch/router-config` checks, with anchor scope. The route itself is being added on
`feat/go-routes`; the delivery harness serves a shim document until it lands.

`POST /api/dispatch/settled` needs no role: it is route-allowlisted, and the platform verifies each
job's own dispatch token.

The router's service account must be anchor-scoped. Provisioning is described in the IaC comment:
a SERVICE principal with `platform:router`.

**Nothing to change in the catalogue.**

## E. Settle reporting (A-01)

`crates/fc-router/src/settled.rs` ports Go's settled reporter, wired whenever `FC_ROUTER_PLATFORM_URL`
is set.

**What triggers a report:** a BLOCK_ON_ERROR group whose head fails terminally (a permanent
rejection).

**What happens to the siblings buffered behind it:** they are **ACKed**, never delivered past the
failure, and never redelivered as a new head. The ones carrying a scheduler-signed dispatch token are
then reported:

```
POST {platform}/api/dispatch/settled
{"reason": "head failed under BLOCK_ON_ERROR", "jobs": [{"id": …, "token": …}]}
```

The report is sent as Go sends it:

- **Batching.** 1000 jobs per request, chunks sent one after another. Every chunk is attempted even
  after one fails.
- **Timeouts.** 5s per request, 10s for the whole report.
- **Delivery.** Fired on its own task after the ACKs, and never awaited by the pool.
- **Retries.** None. The platform's reaper is the backstop.
- **Auth.** No router credential. Each job's token authenticates it.
- **Success.** Only a 200.

Without a platform URL, Rust still hands the siblings back to the broker (NACK) instead of ACKing
them unreported as Go does. See `router-deviations-from-go.md`, D2.

## F. Config document parsing

As Go's `common.QueueConfig.UnmarshalJSON`:

- **Zero means unstated.** An explicit `0` for `connections` or `visibilityTimeout` is treated like
  an absent key or `null`, giving the defaults **1** and **120**. Before, Rust ran a queue with 0
  connections and a 0-second visibility timeout.
- **Keys.** `queueUri` is preferred, then the legacy `uri`. `queueName` is preferred, then the
  legacy `name`, then the URI.
- **Lists.** An absent or `null` `processingPools` or `queues` is an empty list.
- **Pools.** An absent pool `concurrency` is 0; the pool derives its effective concurrency.

## Notes and deviations

1. **Teams notifications (provisional, for owner confirmation).** Go reads none of the task's
   `NOTIFICATION_TEAMS_*` / `NOTIFICATION_MIN_SEVERITY` names, so today's Go router sends no Teams
   alerts, although the task says "Teams webhook enabled". Rust honours the task's intent (these were
   the Rust router's own names), so **alerts start arriving after cutover**. To keep Go's silence,
   set `NOTIFICATION_TEAMS_ENABLED=false`.
2. **`API_PORT` and `FLOWCATALYST_STANDBY_ENABLED`** are dead in both Go and Rust `fc-server`. Their
   values match the defaults, so behaviour is identical. They can be dropped from the IaC or left.
3. **Log format.** Go writes JSON to stderr. Rust writes text unless `LOG_FORMAT=json`. If
   CloudWatch metric filters or Insights queries parse Go's JSON, add `LOG_FORMAT=json` to the Rust
   tasks (optional IaC change). Rust's JSON field names are tracing's, not slog's.
4. **Router HTTP surface.** Under `/router`, the Rust router serves its own API (monitoring,
   dashboard, publish, metrics). Route-for-route parity with Go's `/router` API is not part of this
   contract. The ALB probes only `/health`.
5. **Standby in `fc-server`** is fc-server's own election (`fc:server:leader`), shared by every
   subsystem in the process, as Go. The router polls only while it leads, and its HTTP surface is
   served regardless.

## Proof

- `bin/fc-server/tests/router_role_prod_env.rs` starts the `fc-server` binary with every variable
  above (fake values, **no database variable**) against local stand-ins:
  - a platform (`/oauth/token`, a bearer-only `/api/dispatch/router-config`,
    `/api/dispatch/settled`);
  - an Integral config service and Teams webhook on another origin;
  - SQS through `AWS_ENDPOINT_URL_SQS`.

  It asserts that:
  - `/health` answers;
  - both sources' pools come up under `/router/monitoring/pools`;
  - a consumer polls each queue;
  - exactly one token is minted with the client credentials, and sent with every platform fetch;
  - neither the token nor the secret reaches Integral or SQS;
  - the secret is never logged.

  A second test drops the secret: the process refuses to start before contacting any source.
- `bin/fc-router/tests/prod_env.rs` runs the same contract against the standalone binary.
- Unit tests:
  - `config_sync` (credential origin, re-mint after a 401, permanent refusals, parsing);
  - `platform_token`;
  - `settled`;
  - `bootstrap::env` (the production task definition, the half-credential refusals, alias
    precedence, notification names);
  - `tests/settled_reporter_test.rs` (siblings ACKed and reported).
- The delivery harness (`harness/delivery`) now starts both sides' routers from `fc-server` with the
  task definition's names, plus harness plumbing.

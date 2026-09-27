# Developers

Building applications on FlowCatalyst. Audience: developers integrating their applications with the platform — publishing events, configuring subscriptions, writing webhook receivers.

For platform deployment (provisioning, secrets, HA) see [../operations/](../operations/). For internal architecture see [../architecture/](../architecture/).

## Start here

| If you're… | Read |
|---|---|
| New to FlowCatalyst | [quickstart.md](quickstart.md) — fc-dev in 5 minutes |
| Looking up an `fc-dev` flag or subcommand | [fc-dev.md](fc-dev.md) — complete CLI reference |
| Trying to understand the model | [concepts.md](concepts.md) — events, subscriptions, pools, dispatch modes |
| Publishing events from an application | [publishing-events.md](publishing-events.md) — outbox pattern, batch API, SDK sync |
| Building a webhook receiver | [receiving-webhooks.md](receiving-webhooks.md) — request shape, HMAC, ack/nack |
| Wiring events to webhooks | [subscriptions-and-pools.md](subscriptions-and-pools.md) — patterns, dispatch modes, pool sizing |
| Doing scheduled / cron work | [scheduled-jobs.md](scheduled-jobs.md) — EVENT vs WEBHOOK modes |
| Writing a function (WASM) | [functions.md](functions.md) — limits, fuel, database access |
| Running an app with PostGIS / external Postgres | [fc-dev.md#recipe-app-with-postgis](fc-dev.md#recipe-app-with-postgis-or-any-other-extension-fc-dev-cant-bundle) — `fc-dev outbox poll` sidecar |
| Diagnosing problems | [debugging.md](debugging.md) — by symptom |

## SDK references

| Language | Location |
|---|---|
| TypeScript / JavaScript | [`clients/typescript-sdk/README.md`](../../clients/typescript-sdk/README.md), plus [`docs/syncing-definitions.md`](../../clients/typescript-sdk/docs/syncing-definitions.md) for the manifest format |
| Laravel / PHP | [`clients/laravel-sdk/README.md`](../../clients/laravel-sdk/README.md) |
| Rust (in-workspace) | [`crates/fc-sdk/`](../../crates/fc-sdk/) — see [`shared-crates.md`](../architecture/shared-crates.md#fc-sdk---application-sdk) |

## Mental model in one paragraph

Your app emits events into FlowCatalyst (best via the outbox pattern, so the event publish is part of your business transaction). The platform stores them and matches them against active subscriptions — each match becomes a dispatch job. A scheduler queues those jobs onto a FIFO queue; a router consumes the queue and POSTs to the configured webhook endpoint, applying rate limits and circuit breakers per pool. Your receiver returns 200 (or 200-with-ack-false to ask for retry). At-least-once delivery; idempotency is your responsibility on the receive side.

## Conventions that matter

- **Event type codes are four colons:** `application:subdomain:aggregate:event_name`. Wildcards (`*`) work for subscription patterns.
- **Message groups give you FIFO** within a group, parallel across groups. Set them at publication time when ordering matters.
- **Dispatch modes** are per-subscription: `IMMEDIATE` (default, no ordering), `BLOCK_ON_ERROR` (strict FIFO + halt on failure), `NEXT_ON_ERROR` (FIFO + skip on failure).
- **Connections** are reusable destinations. Many subscriptions can point at one connection. **Pausing** a connection is the way to stop the bleeding for a misbehaving receiver.
- **The HTTP body is the contract.** Don't rely on the FC-specific headers being available in proxies — they should pass through, but the body itself carries the full event.
- **At-least-once delivery.** Make receivers idempotent. Use `X-FlowCatalyst-Dispatch-Job-Id` as the dedup key.

## Quick links

- [Operations docs](../operations/) — for the people running the platform.
- [Architecture docs](../architecture/) — for the people building the platform.
- [CLAUDE.md](../../CLAUDE.md) (repo root) — coding conventions used inside the platform itself.

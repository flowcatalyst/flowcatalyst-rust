# Edge integration with Apache Camel

Status: 2026-10-01. A plan, not built. A copy of `flowcatalyst-go/docs/camel-integration-plan.md` (Go commit 5a76623); the Go repo's copy is the one to edit. It works on any of the three platforms; the Go work it needs is in
[Work for the Go platform](#work-for-the-go-platform).

Business services are written in TypeScript, edge jobs run as functions, and protocols are handled by Apache Camel
connectors that the FlowCatalyst control plane manages.

Decisions this plan makes:

- **Business domain in TypeScript**, with TypeBox schemas as the single definition of a type, its validator, its
  OpenAPI schema and its event schema.
- **Edge jobs as functions**: per-customer mapping, HTTP adapters, table pollers and per-client deployments.
- **Protocols as Camel connectors**, not function code: SFTP/FTP, AS2, MQTT, raw TCP, SOAP. Built once, configured
  per customer.
- **Camel creates `TASK` dispatch jobs** that run mapping functions; the functions emit domain events. Commands in,
  facts out.
- **The control plane manages Camel** as it manages function hosts: versions, signing, desired state, heartbeats,
  status in the SPA.
- **Busy or stateful apps run as long-lived services** (Fastify or Hono on Node), sharing the same schemas and use
  cases as the functions.

## The four layers

Camel handles transport, functions turn inbound data into business facts, TypeScript services own the domain, and
the platform runs and supervises all of it.

```
                  ┌──────────────────────────────────────────────────────────────┐
                  │                  FlowCatalyst control plane                  │
                  │        integrations and functions: versions, signing,        │
                  │                        leases, status                        │
                  └───────────────────┬─────────────────────────────────────────┬┘
                                      ┆ assigns routes                          ┆
                                      ▼                                         ┆
┌──────────────────────┐   ┌──────────────────────┐   ┌──────────────────────┐  ┆
│ Customer systems     │──▶│ Camel connectors     │──▶│ TASK dispatch jobs   │  ┆
│ SFTP drops, AS2,     │   │ fetch, split, dedupe,│   │ ordered per group,   │  ┆
│ MQTT, trackers,      │   │ never process a file │   │ retried, rate-limited│  ┆
│ customer databases   │   │ twice                │   │ per pool             │  ┆
└──────────────────────┘   └──────────────────────┘   └──────────┬───────────┘  ┆
                                                 router delivers │              ┆
┌──────────────────────┐   ┌──────────────────────┐   ┌──────────▼───────────┐  ┆
│ Business services    │◀──│ Domain events        │◀──│ Mapping functions    │◀┄┘
│ TypeScript on Node,  │   │ shipment status      │   │ per-customer mapping,│
│ fed by event         │   │ changed, proof of    │   │ TypeBox validation   │
│ subscriptions        │   │ delivery received    │   │                      │
└──────────────────────┘   └──────────────────────┘   └──────────────────────┘

Solid lines carry data; dotted lines are the control plane assigning Camel routes and loading functions.
```

## Where each kind of edge job goes

Count edge jobs by kind, not by customer: fifty customers' SFTP drops are one connector with fifty configurations,
plus fifty mapping functions.

| Kind of job | What it really is | Where it runs |
|---|---|---|
| Mapping a customer's own schema into ours | pure transformation | function |
| Adapters over HTTP, REST or SOAP | `fetch` plus mapping | function (Camel for SOAP) |
| Polling our own tables | scheduled job, database access, a watermark row | function |
| Ingesting HTTP payloads | authenticated endpoint, rate limits | the platform or a function's public route |
| SFTP/FTP/FTPS drops | stateful: files seen, locks, retries, archiving | Camel connector |
| AS2 exchanges | signed messages and receipts | Camel connector |
| MQTT telemetry | a long-lived subscription | Camel connector (a native ingest path only if volume proves it) |
| Raw TCP from trackers | long-lived connections | Camel listener, in its own pool |
| Polling a customer's database (SQL Server, Oracle, MySQL) | JDBC with a watermark | Camel connector |
| Busy, cache-heavy or CPU-heavy apps | in-memory state across requests | long-lived service |

The rule: protocol work goes in a connector, mapping goes in a function, and state that must survive across
requests goes in a service or a table.

## Data flow: commands in, facts out

Camel turns each inbound file or message into a `TASK` dispatch job aimed at a mapping function
(`POST /api/dispatch-jobs/batch`); the function maps it and emits domain events that the rest of the system
subscribes to.

- **A dispatch job is a command**: point-to-point, one target. Raw inbound data is not a business fact yet, so it
  is not an event.
- **The function emits the facts** (shipment status changed, proof of delivery received). Those carry the audit
  trail, event history and replay.
- **Use a `file-received` event instead** only when several consumers each need the raw data.

What the dispatch job gives the Camel-to-function hop:

| Field | Use |
|---|---|
| `messageGroup`, `sequence` | FIFO per customer, shipment or file |
| `mode` | `BLOCK_ON_ERROR` stops a group on a bad record; `NEXT_ON_ERROR` skips it |
| `idempotencyKey` | file name plus content hash, so a re-polled file maps once |
| `dispatchPoolId` | rate and concurrency per pool, so one large file cannot starve other customers |
| `maxRetries`, `retryStrategy`, `timeoutSeconds` | retries and backoff; the function answers `429` with `Retry-After` to ask for later |
| `correlationId` | one id from the inbound file to the events the function emits |

Job ingest is an infrastructure path, so jobs add no domain events of their own, and every job is visible in the
dispatch-jobs UI with its attempts.

**Large files** go by reference: the payload is stored on the job row and functions buffer request bodies. Camel
either splits a file into one job per record or stores it in object storage and sends a reference with its hash.

**The target** lives in the integration's definition, not in the route: for example, integration
`acme-sftp-orders` dispatches `TASK` code `acme:orders:file:map` to function `logistics.acme.order-mapper`.
Repointing a customer needs no route redeploy.

## Camel in the control plane

Camel hosts are managed the way function hosts already are; the one new idea is that a polling route has exactly
one owner at a time.

**The integration aggregate.** A sibling of the function registry, reusing its pieces:

| Function registry today | Integration |
|---|---|
| function, versions, signed artifact, `live` alias | integration, versions; the artifact is Camel YAML, signed under the same signer policy |
| pools of hosts long-polling desired state | a `camel` pool; hosts long-poll which routes they own |
| heartbeat with per-function loaded status | heartbeat with per-route status: started, stopped or failed; exchanges total, failed and in flight; last exchange; last error |
| config, secrets, secret references | the same, resolved by the host into Camel properties |
| client ownership, policy ceilings | the same |
| function pages in the SPA | an integrations page: status, throughput and errors per route and host |
| heartbeat as an infrastructure write | the same exception: no event per beat |

**Placement and leases.** A function is loaded on every host in its pool; a polling route must run on one, or two
hosts process every file twice.

- The control plane assigns each polling route to one host and holds a lease on it.
- When that host's heartbeats stop, the lease expires and another host takes the route.
- Desired state is therefore per host, not per pool.
- Listener routes (AS2 receive, TCP behind a load balancer) can run on every host; the integration declares which
  kind it is.

**Publishing.** A new version is test-loaded on a host before it can go live, the way function versions become
`READY`. A route that fails to load never replaces the running one.

**The host image.** One curated image with an allowlist of Camel components. No `exec`, no scripting language
unless chosen. Its dependencies go through the same supply-chain checks as everything else.

**Metrics.** Camel's Micrometer metrics go to Prometheus for history; the heartbeat carries only the latest status
per route.

## TypeScript standards

TypeScript's guarantees are opt-in, so the tooling enforces them from the first commit; the TypeScript platform
repo shows what happens otherwise (casts, `?? 500` status maps, unchecked switches, no CI typecheck).

**One schema library: TypeBox.** A TypeBox schema is the TypeScript type, the runtime validator, the OpenAPI schema
(through `@fastify/swagger`) and the event type's JSON Schema version, as one value. No Zod: its JSON Schema export
is one-way and drops refinements.

**Schema for shape, code for rules.** A pattern goes in the schema; a check digit (ISO 6346 container numbers)
goes in a small parser that returns a branded type. The parser holds the only cast.

**Wire types and domain types.** `format: "date-time"` is a `string` and money arrives as a string or float.
Convert dates and money into domain types where the domain does arithmetic on them: cut-offs, transit days, rates.

**CI fails on:**

1. `tsc --noEmit` with `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`.
2. Type-aware lint: switch exhaustiveness, no floating promises, the `no-unsafe-*` family, no `as` outside parsers
   and tests.
3. Kysely types out of date with the migrated schema (`kysely-codegen`).

**Frameworks.** Hono for functions (web-standard `Request`/`Response`); Fastify or Hono on Node for long-lived
services. Use cases, schemas and repositories are shared; only the entry point differs.

**Exhaustive maps** use `satisfies Record<Union["kind"], T>`, so a new member fails to compile.

## Why Camel, not our own connectors

The protocol code is cheap; the behaviour discovered in production is not, and Camel and the libraries under it
have about 15 years of it.

- Files still being written: read locks, done files, size and change checks, coarse server timestamps.
- After processing: move, rename or delete, with rename semantics that differ by server.
- Never twice: a durable record of processed files (a JDBC repository in Postgres) that survives restarts and
  failover.
- FTP servers with odd listing formats, and very large directories.
- Key rotation, keyboard-interactive login, host keys that change.
- AS2 signing, encryption and receipts (MDNs).
- Character sets and line endings in customer files.

Camel's costs: a JVM service in its own image; endpoint options are strings that fail when a route loads, not when
it builds; a large framework to learn. Keeping routes small and the logic in functions limits how much Camel anyone
needs to know.

Redpanda Connect (Go, YAML pipelines) is the lighter alternative, strong for telemetry; check its licence, as some
connectors are under an enterprise licence. Camel stays the pick for legacy protocols.

## Work for the Go platform

The Camel work is the same on every platform. On Go, it sits beside the existing function runner
(`internal/functions`, `internal/platform/function`).

**Camel support:**

- [ ] Integration aggregate (entity, repository, use cases, API) with versions and signing, alongside
      `internal/platform/function`.
- [ ] Camel control API: desired state per host, leases, heartbeat with per-route status, reusing the
      long-poll pattern of `internal/platform/function/control`.
- [ ] Integrations page in the SPA.
- [ ] Curated Camel host image with a component allowlist.
- [ ] Object storage for large files, passed to functions by reference.
- [ ] Confirm a function endpoint accepts and verifies the router's delivery of a `TASK` dispatch job.

**TypeScript functions on Go's runner:**

- [ ] Web-standard `Request`, `Response`, `Headers` and `URL` for JS functions, so Hono runs. Go's shared QuickJS
      engine (`clients/fn-js-engine`) has none today, and `clients/fn-ts` uses its own request types. Either add
      them to the engine or ship them as a polyfill and adapter in `clients/fn-ts`.
- Database access from TS functions already exists (`db(name).query`, `.exec`, transactions).

**What Go does not get:** JVM functions. A JVM runtime is a non-goal in `flowcatalyst-go/docs/function-runner-plan.md` §14, and
Camel covers the protocols that made one attractive.

For comparison, from `flowcatalyst-go/docs/function-runner-comparison.md` (macOS, different guests,
orders of magnitude only):

| | Rust | Go | Java |
|---|---|---|---|
| TypeScript functions | V8, web-standard `Request`/`Response` | shared QuickJS, 13.9 µs warm call, 0.95 MB per function; own SDK | QuickJS on the JVM: 5.8 ms per call, about 50 MB per function |
| Database access from TS functions | not yet | available | n/a |
| Java functions | Java's host in a `jvm` pool | not possible | native |
| In production | no | yes | no |

## Open questions and risks

- [ ] **`TASK` job authentication.** Not yet traced end to end: does a function endpoint accept the router's
      signature for a service account's `TASK` job, or only a subscription delivery's?
- [ ] **TypeBox compiled validators.** `TypeCompiler` generates code with `new Function`; untested on the JS
      runners. `Value.Check` works without it, more slowly.
- [ ] **Large files.** Split into one job per record, or pass an object-storage reference with a content hash.
      Decide per integration, or as a default.
- [ ] **Telemetry volume.** Start on Camel; build a native ingest path only if measured volume justifies it.
- **Route YAML is code.** Camel can call beans, scripts and `exec:`. Routes are first-party, signed, and limited by
  the component allowlist; customers never author them.
- **Camel errors surface at runtime.** Publishing test-loads each version; routes that matter keep tests in Camel's
  test kit.
- **Another JVM service.** The platform plus Camel means two runtimes to operate.

# Writing functions

A **function** is a small unit of your code that the platform deploys and runs for you on a *function host*: an
HTTP endpoint, an event subscriber, a scheduled job, or all three. You publish an artifact with a `manifest.json`
(its endpoints, subscriptions, schedules, config and secrets), promote a version to `live`, and the host serves it at
`/functions/<application>.<service>.<name>/…` (and on the public routes you claim). Functions are first-party code:
they are chosen for density and fine-grained deployment, and each runs with memory and time limits.

The Rust function host runs two kinds:

| Runtime | Artifact | Write it in | Template |
|---|---|---|---|
| `component` | a WASI 0.2 component exporting `wasi:http/incoming-handler` | Rust (`fc-function-pdk`), or any language with a `wasm32-wasip2` toolchain | `fc-dev fn init --lang rust` |
| `js` | one ES module bundle (UTF-8 JavaScript) | TypeScript or JavaScript | `fc-dev fn init --lang ts` / `--lang js` |

`wasm` is the older name a component can be published under (entrypoint `wasi_http_incoming_handler`), for a platform
without `component`. `jvm` functions (jars) run on Java's function host only, in a pool of their own (see
[JVM functions](#jvm-functions-runtime-jvm)). The engine and hosting design is in
[`../function-runner-plan.md`](../function-runner-plan.md); a complete Rust function is
[`examples/function-hello-rust`](../../examples/function-hello-rust/).

Both kinds see the same host services, from the WIT package `wit/flowcatalyst-function` (config, secrets, logging,
events, the invocation context) plus outbound HTTP limited to the manifest's `httpAllow`, and both follow the same
rules: one fresh instance per request, the endpoint's `timeoutMs`, `limits.wasmMemoryMb`, and Java's
`500 {"error":"the function failed"}` for anything that is not a response. Two things are WASM-only for now: fuel
metering (`limits.maxFuel`) and database access (`db[]`); the JS API gains a `flowcatalyst:function/db` module
when the runtime supports it.

## The loop, locally

`fc-dev` runs the platform and a function host together (see [fc-dev.md](fc-dev.md)):

```sh
fc-dev                                            # platform + function host
fc-dev init --code shop --name Shop               # once: an application

fc-dev fn init --lang ts hello && cd hello        # or --lang rust / --lang js
fc-dev fn build                                   # ts/js: npm install + typecheck + esbuild → dist/function.mjs
                                                  # rust: cargo build --target wasm32-wasip2 → target/…/hello.wasm
fc-dev fn config set shop.default.hello GREETING=Hello
fc-dev fn deploy dist/function.mjs shop.default.hello
fc-dev fn invoke shop.default.hello --path /hello/world
```

`fn deploy` uploads the artifact, publishes a version with `manifest.json`, waits until the host has it `READY` and
promotes it to `live`. `fn validate <address> --manifest manifest.json` shows what a promote would wire, without
publishing.

## Limits

`limits` in the manifest bounds every invocation of a version. An absent limit takes the platform
default, clamped to the owning client's ceiling (the client's function policy).

| Key | Default | What happens past it |
|---|---|---|
| `maxDurationMs` | 30 000 | An endpoint's `timeoutMs` defaults to it. The guest is stopped wherever it is: `504 FUNCTION_TIMEOUT`. |
| `maxConcurrency` | 32 | Calls in flight per host; one more is `429 BUSY` with `Retry-After: 1`. |
| `wasmMemoryMb` | 64 | Growing linear memory past it fails the allocation (a trap in Rust): `500 FUNCTION_ERROR`. For `js` it caps the isolate's JavaScript heap (at least 8 MiB) and, separately, its `ArrayBuffer` storage: `500`. |
| `maxFuel` | none | The guest is stopped wherever it is: `500 FUNCTION_FUEL_EXHAUSTED`. Rust hosts, `wasm` and `component` only (see below). |

### Fuel

Every invocation on a Rust host is **metered in fuel**: wasmtime's count of the WebAssembly instructions
the guest executed (most instructions cost 1; control-flow bookkeeping such as `nop`, `block` and `loop`
costs 0). Fuel is deterministic: the same call on the same input spends the same fuel on any machine,
which wall-clock time is not. It measures only the guest's own work; time waiting on the host (outbound
HTTP, an emit, a database query, a sleep) costs no fuel.

`limits.maxFuel` (optional, a positive integer up to 2⁶³−1) caps the fuel **one invocation** may spend.
Past it the guest stops at once, wherever it is, and the caller gets:

```json
500 {"error":"FUNCTION_FUEL_EXHAUSTED","message":"the invocation used up the fuel its limits allow"}
```

Use it as a runaway guard that trips long before the deadline: a loop that never ends burns its budget
in milliseconds instead of holding a guest thread until `timeoutMs`. Size it from the metrics below: take
the largest `fc_fn_invocation_fuel` your function spends on real traffic and allow a generous margin
(10× is reasonable). Without `maxFuel` an invocation is still metered, just never stopped for fuel.

- `maxFuel` applies to `runtime: wasm` and `component` only; on a `jvm` or `js` function it is
  `400 LIMIT_NOT_APPLICABLE`. JS functions are not fuel-metered (V8 has no fuel); their deadline and
  memory limit bound them. It has no platform default and no client ceiling.
- It is a Rust-host extension: Java's manifest parser refuses it (`MANIFEST_UNKNOWN_FIELD`), so keep it
  out of manifests that must also publish to a Java platform.
- Rough scale (Apple M4, release build): a trivial handler spends tens of thousands of fuel; one second
  of tight guest compute is on the order of 10⁹.

### What the host exports

The host's `/metrics` (the observability port) carries, per invocation, labelled by `address` and
`client` (the owning client's id, or `PLATFORM` for a platform function):

| Series | Type | Meaning |
|---|---|---|
| `fc_fn_fuel_total` | counter | Fuel spent, all invocations together. Per client: `sum by (client) (rate(fc_fn_fuel_total[5m]))`. |
| `fc_fn_invocation_fuel` | histogram | Fuel per invocation (buckets 10⁴ … 10¹⁰). |
| `fc_fn_invocation_peak_memory_bytes` | histogram | The highest linear memory one invocation reached (256 KiB … 4 GiB). |

An invocation stopped by its deadline still reports what it spent up to then (to within 10 M fuel). A
fuel-exhausted call counts under `fc_fn_invocations_total{outcome="fuel_exhausted"}`. The series of a
function that leaves the host's desired state are dropped, like its other series.

Metering costs little: between 0 and 2% on typical handlers and about 20% on a tight arithmetic loop
(`docs/function-runner-density.md` §9).

## Database access

A WASM function reaches the PostgreSQL databases its manifest declares under `db[]`, through the host's shared
connection pools (owner decision #7; Java's W4 `fc_db_*` contract). The WIT interface is
`flowcatalyst:function/db` (package 0.1.2); in Rust, [`fc-function-pdk`](../../crates/fc-function-pdk/README.md)
wraps it:

```json
"secrets": ["ORDERS_DB"],
"db": [{ "name": "orders", "secretRef": "ORDERS_DB", "poolSize": 4 }]
```

```rust
use fc_function_pdk::prelude::*;

#[handler]
fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
    let db = ctx.db("orders")?;                    // DB_NOT_DECLARED for any other name
    let id = req.path_param("id").unwrap_or_default().to_owned();

    let tx = db.begin()?;                          // a guard: dropped uncommitted, it rolls back
    tx.execute("UPDATE orders SET state = 'shipped' WHERE id = ?", params![id.as_str()])?;
    tx.execute("INSERT INTO shipments (order_id, at) VALUES (?, ?)", params![id.as_str(), "2026-09-27T10:00:00Z"])?;
    tx.commit()?;

    let rows = db.query("SELECT id, state, total FROM orders WHERE id = ?", params![id.as_str()])?;
    Ok(Response::json(200, rows.json())?)         // [{"id":"…","state":"shipped","total":"12.50"}]
}
```

`db.transaction(|tx| { …; Ok(value) })` commits on `Ok` and rolls back on `Err`. In unit tests,
`TestHost::new().db("orders", |call| Ok(DbReply::rows(json!([…]))))` answers the statements and
`host.db_events()` records what the function did.

**The connection.** `secretRef` names the secret holding it (the platform delivers it with the function's
other secrets; set it with `fc-dev fn secret set` or the API). Accepted, PostgreSQL only:

- `postgres://user:pass@host[:port]/db[?sslmode=…]` (or `postgresql://`);
- `jdbc:postgresql://host[:port]/db?user=…&password=…` (Java's form);
- `aws-sm://<secret id or ARN>` on hosts built with AWS support (`fc-server`): an AWS Secrets Manager
  secret holding either of the above or an RDS-style JSON secret (`username`, `password`, `host`, `port`,
  `dbname`). The host re-reads it every `FC_FN_DB_SECRET_REFRESH_SECONDS` (300), so an RDS password rotation
  reaches the pool without a redeploy.

Anything else fails the load with `DB_UNSUPPORTED` (heartbeat `FAILED`, the previous version keeps
serving); an `aws-sm://` secret that cannot be read, `DB_SECRET_UNRESOLVED`. No connection is opened at
load, so a database that is down is `DB_UNAVAILABLE` at run time, not a failed load. A plain DSN rotates
through the platform: setting the secret's new value reloads the function onto a new pool.

**The contract** (Java's):

| | |
|---|---|
| Parameters | Bound to `?` placeholders in order, never interpolated. `??` is a literal `?` (jsonb's `?`, `?|`, `?&`). Integers are sent as `int8`, floats as `float8`, booleans as `bool`, `Param::Decimal` (and a JSON number that is not an integer) as `numeric`; text and `NULL` untyped, so the server reads text as whatever the placeholder needs (`uuid`, `timestamptz`, `jsonb`, `date`, an enum, …). A type text cannot be bound to directly (`interval`, arrays, ranges) goes through text in the SQL: `?::text::interval`. One SQL command per statement. |
| Connections | Without a transaction, each statement borrows a connection and returns it before answering (autocommit). A transaction holds one until commit, rollback or drop; the invocation ending rolls back whatever is still open. Every connection goes back to the pool clean: an open or failed transaction rolled back and the session reset (`DISCARD ALL`), whatever the SQL did (`BEGIN` as a statement, `SET`, temporary tables, advisory locks). |
| Deadline | Each statement's timeout is the time left before the invocation's deadline; with less than 1 ms left it is not sent. Waiting for a connection also ends at the deadline. |
| Size | A query answers at most 10 000 rows or 8 MiB of row JSON; `truncated` says there were more. |
| Rows | A JSON array of objects keyed by column label: `int2/4/8`, `oid` and `float4/8` as numbers (`NaN`, `Infinity` as strings), `numeric` as its exact text (`"12.50"`), `bool` as a boolean, `timestamptz` as ISO-8601 in UTC with `Z`, `timestamp`/`date`/`time`/`timetz` as ISO-8601, `bytea` as base64, `json`/`jsonb` as the value, `NULL` as null; everything else (text, uuid, interval, arrays, enums, inet, …) as PostgreSQL's text form. Types without a renderer (ranges, composites, geometric, `money`) come back as raw text or `\x`-hex: cast them (`col::text`). |
| Errors | Values, never traps; SQL text and parameter values are never logged. Codes: `DB_NOT_DECLARED`, `DB_BAD_REQUEST` (a malformed call, a parameter its placeholder cannot take, too many connections at once), `DB_TX_UNKNOWN`, and by SQLSTATE class `DB_CONSTRAINT` (23), `DB_SYNTAX` (42), `DB_TIMEOUT` (57014, the deadline), `DB_UNAVAILABLE` (08, 53, 57: worth a retry; also logged for the operator), `DB_ERROR` (anything else). A commit after a statement failed in the transaction is `DB_ERROR`: nothing was committed. |

**Pools and limits** (the host's operator settings are in
[`../operations/configuration.md`](../operations/configuration.md)):

- One pool per connection (host, port, database, user, password and parameters; or per `aws-sm://`
  reference), shared by every function on the host that names it, sized to the largest `poolSize` among
  them. `poolSize` defaults to 4 and is capped by the client's function policy.
- A function holds at most its own `poolSize` connections of a shared pool at once, across all its
  invocations, so one function with slow callers cannot take a pool from the others: its extra calls wait
  (until their deadline, then `DB_TIMEOUT`). A function that needs its own pool gets its own database
  user, so its own connection string.
- One invocation holds at most `FC_FN_DB_MAX_CONNECTIONS_PER_INVOCATION` (2) open transactions per database,
  and never more than its `poolSize`: one more `begin` is `DB_BAD_REQUEST` rather than a wait on itself.
- A host opens at most `FC_FN_MAX_DB_POOLS` (16) pools; a function that would need one more fails its
  load with `DB_POOL_LIMIT`.

JS functions do not have database access yet: a `js` function that declares `db[]` fails its load with
`DB_UNSUPPORTED`. The JS API will gain a `flowcatalyst:function/db` module mirroring the WIT interface.

## TypeScript and JavaScript functions (`runtime: js`)

A JS function is **one ES module bundle**. Its default export (or the export `entrypoint` names) handles every
request, web-style:

```ts
import { get as config } from "flowcatalyst:function/config";

export default async function handle(request: Request): Promise<Response> {
  const name = new URL(request.url).searchParams.get("name") ?? "world";
  return Response.json({ message: `${config("GREETING") ?? "Hello"}, ${name}!` });
}
```

An object with a `fetch(request)` method works too (`export default { fetch(request) { … } }`).

### The host's modules

A JS projection of `wit/flowcatalyst-function`, one module per WIT interface. The template's
`types/flowcatalyst-function.d.ts` declares all of it for TypeScript (it is the host's own file,
`crates/fc-fnhost-js/types/flowcatalyst-function.d.ts`).

| Module | Exports | WIT |
|---|---|---|
| `flowcatalyst:function/config` | `get(key): string \| undefined`: a key the manifest declares under `config` | `config.get` |
| `flowcatalyst:function/secrets` | `get(key): string \| undefined`: a declared, non-empty secret; never log it | `secrets.get` |
| `flowcatalyst:function/log` | `log(level, message)`, `trace`/`debug`/`info`/`warn`/`error(message)`: lines on the function's logger `fn.<address>` with the invocation's fields | `log.log` |
| `flowcatalyst:function/events` | `emit(event): Promise<{ ok: true, id } \| { ok: false, error }>` | `events.emit-event` |
| `flowcatalyst:function/invocation` | `context()`: `invocationId`, `address`, `version`, `caller`, `correlationId`, `causationId?`, `originalHost?`, `originalPath?`, `remoteAddress?`, `pathParams` | `invocation.context` |
| `flowcatalyst:function` | all of the above as namespaces: `import { config, events } from "flowcatalyst:function"` | |

`emit` takes `{ type, dedupId, subject?, data?, correlationId?, causationId?, messageGroup?, source?, dataContentType? }`
(`data` is any JSON value). A refusal is a value, not a rejection, mirroring `emit-event`'s
`result<string, emit-event-error>`:

```ts
const result = await emit({ type: "shop:orders:order:shipped", dedupId: `shipped-${order.id}`, data: order });
if (!result.ok) {
  // result.error is { kind: "invalid", code } | { kind: "refused", code, status, message } | { kind: "unavailable", message }
  if (result.error.kind !== "invalid" && (result.error.kind === "unavailable" || result.error.status >= 500)) {
    return new Response(null, { status: 429, headers: { "retry-after": "30" } }); // worth a retry
  }
  throw new Error(`emit refused: ${JSON.stringify(result.error)}`);
}
```

Correlation and causation ids default to the invocation's: the inbound event's on a verified webhook delivery, else
the `X-Correlation-Id` header, else the invocation id. A later WIT interface (for example `db`) becomes one more
module here.

### Globals

The ECMAScript built-ins and a web-platform subset: `Request`, `Response` (with `Response.json`), `Headers`, `fetch`,
`URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder` (UTF-8), `atob`, `btoa`, `console`, `setTimeout`/`setInterval`
and their `clear*`, `queueMicrotask`, `structuredClone`, `crypto.getRandomValues`, `crypto.randomUUID`. There is **no**
Node API (`process`, `require`, `Buffer`, `node:*`), no filesystem, no environment variables, no `Deno`, no WebAssembly
and no network other than `fetch`. Bundle what you need from npm; read settings through `config` and `secrets`.

`console.debug` logs at DEBUG, `console.log`/`info` at INFO, `warn` at WARN and `error` at ERROR, one line per `\n`,
split at 8 KiB.

**`fetch`** reaches only the hosts in the manifest's `httpAllow` (an exact host, or `*.suffix` for subdomains, never
the apex), over `https` (plain `http` only to `localhost`, `127.0.0.1`, `::1`). Redirects are returned, never
followed. Its timeout is the smaller of the time left before the invocation's deadline and 30 s. A call that does not
produce a response rejects with an `HttpError` whose `code` is the `wasi:http` error code a component would see:
`HTTP-request-denied` for a refused host, `HTTP-response-timeout`, `connection-refused`, `HTTP-response-body-size`, …

### The rules

- **One isolate per request.** The bundle's top-level code runs at the start of every request's isolate, then the
  handler; nothing survives from one request to the next. Keep top-level work small, and do not rely on module-level
  caches.
- **Host APIs are for requests.** `config`, `secrets`, `events`, `invocation` and `fetch` work inside the handler,
  not in top-level code (a bundle that calls them at the top level is refused at load, `JS_INIT_FAILED`). Logging
  works everywhere.
- **Bodies are buffered** (no streams): the request's is capped by the endpoint's `maxBodyBytes`, the response's by
  `limits.wasmMemoryMb`. A response body is a string, an `ArrayBuffer`, a typed array, `URLSearchParams` or `null`.
- **Memory:** `limits.wasmMemoryMb` caps the isolate's JavaScript heap (at least 8 MiB) and, separately, its
  `ArrayBuffer` storage. Past either, the call ends with 500.
- **Time:** the endpoint's `timeoutMs` stops the function wherever it is (the caller gets 504). JavaScript is not
  preempted otherwise: a computation holds its worker thread, and one of the host's `FC_FN_MAX_EXECUTING`
  executing slots (shared with every WASM function on the host), until it awaits, so keep CPU-heavy work short.
  Awaiting `fetch`, an emit or a timer holds neither.
- **Settle before you return.** Work still pending when the response is ready is dropped with the isolate.
- **Failures:** a throw, a rejection, a value that is not a `Response`, or a promise that can never settle answers
  `500 {"error":"the function failed"}`. The error goes to the host's log, never to the caller.
- **Subscriptions and schedules** deliver a signed webhook to the endpoint the manifest names: an empty `200`
  acknowledges it, a `500` (or a throw) fails the attempt, and `429` with `Retry-After` asks for it again later.

### Building and publishing

The artifact is a single ES module, UTF-8, that imports nothing but `flowcatalyst:function/*`; the platform checks
it is text at publish (`ARTIFACT_RUNTIME_MISMATCH` otherwise) and the host refuses anything else at load:

| Heartbeat `LOAD:` code | Why |
|---|---|
| `JS_INVALID` | not UTF-8, or not a module V8 compiles |
| `JS_IMPORT_NOT_ALLOWED` | an import other than `flowcatalyst:function/*` (bundle your dependencies) |
| `JS_ENTRYPOINT_NOT_EXPORTED` | the entrypoint export is missing, or neither a function nor an object with `fetch` |
| `JS_INIT_FAILED` | the top-level code threw, called a request-only host API, ran out of memory, or took over 10 s |

The templates build with esbuild (`--bundle --format=esm --platform=neutral --external:flowcatalyst:*`) to
`dist/function.mjs`, and the TypeScript one typechecks with `tsc` against the declarations (`lib: ["es2023"]`, no
DOM). The toolchain is optional: any bundler that emits one ES module works, and a function written as one `.mjs`
file (like `templates/function-js/src/index.js`) deploys as it stands.

The manifest says `"runtime": "js"`; `entrypoint` is optional (`default`, or another export's name).

## Rust functions (`runtime: component`)

A WASI 0.2 component exporting `wasi:http/incoming-handler`, written with the PDK (`crates/fc-function-pdk`):
`#[handler] async fn handle(req: Request, ctx: Context) -> Result<Response, E>`, with `ctx.config()`,
`ctx.secrets()`, `ctx.events().emit(…)`, `ctx.logger()`, `ctx.http()`, `ctx.db(…)` and `ctx.invocation()`. The template
(`templates/function-rust`) and the example (`examples/function-hello-rust`, an adapter that maps an event, calls an
HTTPS API and emits an event) show the whole shape, with native unit tests through `testing::TestHost`.

## JVM functions (`runtime: jvm`)

Java functions are jars that implement Java's `function-api` (`io.flowcatalyst.function.Function`). This platform is
their control plane, and **Java's function host** runs them: `function-host` in `flowcatalyst-javalin`, shipped as
the `flowcatalyst-fnhost` image (owner, 2026-09-28). A Rust host never runs a jar. The two kinds of host share the
control API (desired state, heartbeat, artifact download, event emit), so nothing about publishing changes:

- **Write and build** with Java's tooling. `examples/function-hello` in `flowcatalyst-javalin` is the reference: a
  `webhook` endpoint that reads config and a secret and emits an event, a `platform` endpoint that checks the
  caller's permission, and a `none` health check. Build its shrunk jar with Maven; scaffold a new one with Java's
  `fcdev fn init --runtime jvm`.
- **Manifest**: `"runtime": "jvm"`, `entrypoint` is the class name, and `pool` names a pool that only Java hosts
  serve (convention: `jvm`, or `jvm-<purpose>`). `limits.maxFuel` does not apply (`LIMIT_NOT_APPLICABLE`).
- **Publish** as any other function: `fc-dev fn deploy target/hello-shrunk.jar hello.default.hello --manifest
  manifest.json --bundle fn.sigstore.json` (the CLI does not care about the runtime), Java's `fcdev fn deploy`, or the
  API directly (`PUT /api/functions/{address}/artifacts/{digest}`, `POST …/versions`, `PUT …/aliases/live`). The
  owner's signer policy must list `jvm` among the signer's runtimes. The Java host downloads the jar from the
  platform, verifies its signature against the recorded signer, and registers it; the platform marks it `READY`.

### Pools: keep runtimes apart

A pool is served through one URL (`FC_FN_POOL_URL`, by default `http://fn-{pool}:8080`), so every host behind it
must be able to run every function placed there. What happens when they can't:

| Placement | Result |
|---|---|
| `jvm` function, pool served only by Rust hosts | Publish refused: `409 POOL_RUNTIME_UNSUPPORTED` (Rust hosts report the runtimes they load; none reports `jvm`). |
| `jvm` function, pool with no live host yet | Published; the manifest check warns `POOL_HAS_NO_LIVE_HOSTS`. A Rust host that later serves the pool reports it `FAILED RUNTIME_UNSUPPORTED`. |
| `jvm` function, pool shared by Rust and Java hosts | Published with the warning `POOL_RUNTIME_UNKNOWN`. The Java host loads it; each Rust host reports it `FAILED RUNTIME_UNSUPPORTED` without downloading it, logs it once, and keeps running. The version stays `READY` and live. A request the pool's URL sends to a Rust host answers `503 FUNCTION_UNAVAILABLE`. |
| `component` or `js` function in a Java pool | The Java host can't read the runtime: `FAILED UNREADABLE:manifest runtime is unreadable`. |
| `wasm` function (a component) in a Java pool | Java reads `wasm` as its own Extism core-module runtime. It registers the candidate, so the version can go `READY`, and fails it once live: `FAILED LOAD:WASM_INVALID`, calls answer `503`. |

A host's failure shows per host in `GET /api/functions/{address}/status` (`hosts[].loaded[]`, with `error`) and on
the function's page in the SPA (the version tag reads `FAILED`, with the code as its tooltip). Java hosts report no
`runtimes` in their heartbeat, so the manifest check of any function in a Java pool carries the warning
`POOL_RUNTIME_UNKNOWN`. It is expected, not a problem.

### What is proven

`bin/fc-server/tests/jvm_function_host_e2e.rs` runs the whole path against a Postgres container, with signatures
`required` (a private Sigstore trust root given to the platform and both hosts):

- The platform (`fc-server`) runs with the stream processor and scheduled jobs on. Java's host runs from a scratch
  build of the javalin checkout, in pool `jvm`.
- `function-hello` is published and becomes `READY` on the Java host, then is promoted.
- It answers `auth: none`, and `auth: platform` with the permission check, on platform tokens.
- A pinned version needs `platform:function:version:invoke`.
- An ingested event is delivered through `/api/dispatch/process`, signed with the application's secret. The function
  reads its config and secret and emits an event that lands in `msg_events`.
- A config change reloads the function.
- A scheduled-job firing reaches a JVM function and parses with Java's `Webhook.schedule`.
- Every row of the table above behaves as it says.
- Disabling the function unloads it, and the host stops cleanly on `SIGTERM`.

```sh
cargo test -p fc-server --test jvm_function_host_e2e -- --ignored --nocapture
```

It needs Docker, a JDK 25 (`java`, `javac`) and Maven 3.9, and skips with a message when one is missing. It builds
`../flowcatalyst-javalin` (`FC_JAVALIN_DIR`), never in place: it exports `HEAD` into
`target/jvm-e2e/javalin-<commit>` and builds it once. `FC_JVM_BUILD_DIR=<built tree>` reuses a build.
Deployment is in [configuration](../operations/configuration.md#jvm-function-host-javas-fc-fnhost).

## The registry's tables: one prefix per implementation (temporary)

Three platforms have a function registry, and they share databases (fc-dev's shared cluster, production's cutover
onto a Go-migrated database). Their tables are incompatible, and each creates them with `CREATE TABLE IF NOT EXISTS`,
so on one database the first to migrate would win and the others fail later. Until the owner picks one
implementation, each has its own prefix (owner decision #48, 2026-09-28):

| Platform | Prefix | Tables |
|---|---|---|
| Java (`flowcatalyst-javalin`) | `fn_` | `fn_functions`, `fn_versions`, `fn_aliases`, `fn_hosts`, `fn_client_policies`, `fn_domains`, `fn_routes`, `fn_trigger_objects`, `fn_config`, `fn_secrets` |
| Rust (this platform) | `fnr_` | the same ten tables, same columns and constraints, as `fnr_*` (migration 062) |
| Go (`flowcatalyst-go`) | `fng_` | its own runner's tables (its migration 059 creates them as `fn_*` today; Go is moving them to `fng_`) |

Rust never creates, alters or drops a `fn_*` table: the migrations that did (034, 037, 056) are retired (see
[fc-dev.md](fc-dev.md#retired-migrations)). The hosts are unaffected: a host, Java's included, talks to the
platform's API, not its tables.

**Functions registered before the move need publishing again.** On a database where Rust's old 034 ran, the
functions, versions, aliases, config, secrets, policies, domains and routes Rust wrote are still in `fn_*`, and
062 copies none of them into `fnr_*`: on a shared database those rows may be Java's, and nothing tells them apart.
Deploy each function again (`fc-dev fn deploy …`, then its config and secrets), and re-create its client policy,
domains and routes. The old `fn_*` rows are left as they are.

## Which to choose

| | Rust component | TypeScript / JavaScript |
|---|---|---|
| Memory per loaded function | ≈0.15 MiB (from the `.cwasm` cache) | about twice the bundle (≈24 KiB for a small one) |
| Memory per request in flight | an instance (its linear memory) | an isolate, ≈1.8 MiB |
| Per request, through the listener (Linux) | ≈0.1 ms | ≈1.1 ms, plus the bundle's top-level code (+0.7 ms at 256 KiB) |
| Isolation of a busy function | preempted every 1 ms (epoch ticks) | runs to its next `await` on its worker thread |
| Executing slots (`FC_FN_MAX_EXECUTING`, one budget for the host) | held while computing, given back every 1 ms and at every wait | held while computing, until the next `await` |
| Fuel metering, `maxFuel` | yes | no |
| Database access (`db[]`) | yes | not yet |
| Ecosystem | crates that build for `wasm32-wasip2` | npm packages that bundle without Node APIs |

On macOS (fc-dev) a JS request costs about 3 ms: isolates are made without V8's snapshot there (see
`FC_FN_JS_SNAPSHOT` in [configuration](../operations/configuration.md)).

Measurements: `docs/function-runner-density.md` (§9 for fuel, §10 for JS).

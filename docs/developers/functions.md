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
without `component`. `jvm` functions (jars) run on Java hosts only.

Both kinds see the same host services, from the WIT package `wit/flowcatalyst-function` (config, secrets, logging,
events, the invocation context) plus outbound HTTP limited to the manifest's `httpAllow`, and both follow the same
rules: one fresh instance per request, the endpoint's `timeoutMs`, `limits.wasmMemoryMb`, and Java's
`500 {"error":"the function failed"}` for anything that is not a response.

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
  preempted otherwise: a computation holds its worker thread until it awaits, so keep CPU-heavy work short.
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
`ctx.secrets()`, `ctx.events().emit(…)`, `ctx.logger()`, `ctx.http()` and `ctx.invocation()`. The template
(`templates/function-rust`) and the example (`examples/function-hello-rust`, an adapter that maps an event, calls an
HTTPS API and emits an event) show the whole shape, with native unit tests through `testing::TestHost`.

## Which to choose

| | Rust component | TypeScript / JavaScript |
|---|---|---|
| Memory per loaded function | ≈0.15 MiB (from the `.cwasm` cache) | about twice the bundle (≈24 KiB for a small one) |
| Memory per request in flight | an instance (its linear memory) | an isolate, ≈1.8 MiB |
| Per request, through the listener (Linux) | ≈0.1 ms | ≈1.1 ms, plus the bundle's top-level code (+0.7 ms at 256 KiB) |
| Isolation of a busy function | preempted every 1 ms (epoch ticks) | runs to its next `await` on its worker thread |
| Ecosystem | crates that build for `wasm32-wasip2` | npm packages that bundle without Node APIs |

On macOS (fc-dev) a JS request costs about 3 ms: isolates are made without V8's snapshot there (see
`FC_FN_JS_SNAPSHOT` in [configuration](../operations/configuration.md)).

Measurements: `docs/function-runner-density.md` (§9 for JS).

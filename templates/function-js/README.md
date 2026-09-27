# {{project-name}}

A FlowCatalyst function in JavaScript: one ES module bundle, run by the
function host in a V8 isolate (`runtime: js`). Its default export handles
every request, web-style: it takes a `Request` and returns a `Response`.

```sh
npm install          # once: esbuild (a dev dependency only)
npm run build        # esbuild: dist/function.mjs, the artifact
```

`fc-dev fn build` runs the build (and `npm install` when `node_modules/` is
missing). `jsconfig.json` points editors at `types/flowcatalyst-function.d.ts`,
which declares the whole API.

## The toolchain is optional

The platform only needs the bundle: a single ES module, UTF-8, that imports
nothing but `flowcatalyst:function/*`. `src/index.js` imports nothing else,
so it is a valid artifact as it stands:

```sh
fc-dev fn deploy src/index.js <app>.default.{{project-name}}
```

esbuild only matters once you import from npm: it bundles those in and
leaves `flowcatalyst:*` external.

## The API

| Module | What |
|---|---|
| `flowcatalyst:function/config` | `get(key)`: a manifest-declared config value |
| `flowcatalyst:function/secrets` | `get(key)`: a manifest-declared secret |
| `flowcatalyst:function/log` | `log(level, message)`, `info(…)` and friends; `console.*` works too |
| `flowcatalyst:function/events` | `emit(event)`: `{ ok: true, id }` or `{ ok: false, error }` |
| `flowcatalyst:function/invocation` | `context()`: invocation id, caller, correlation id, path params, … |

Outbound HTTP is the global `fetch`, limited to the hosts the manifest lists
in `httpAllow`, over https (plain http only to loopback), redirects never
followed. There is no Node API, no filesystem and no environment.

Each request gets a fresh isolate: the bundle's top-level code runs before
every request, and nothing survives from one request to the next. The host
APIs work only inside the handler.

## Publishing the function

`manifest.json` declares the function's endpoints, config and secrets. Keep
`runtime: js`; `entrypoint` defaults to `default` (the default export), and
`limits.wasmMemoryMb` caps the isolate's memory.

```sh
fc-dev fn config set <app>.default.{{project-name}} GREETING=Hello
fc-dev fn deploy dist/function.mjs <app>.default.{{project-name}}
fc-dev fn invoke <app>.default.{{project-name}} --path /hello/world
```

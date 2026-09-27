// FlowCatalyst JS functions: the API a `runtime: js` function is written
// against, for TypeScript.
//
// A JS function is ONE ES module bundle. Its default export (or the export
// the manifest's `entrypoint` names) handles every request:
//
//     export default async function handle(request: Request): Promise<Response> { … }
//
// or, equally, an object with a `fetch` method:
//
//     export default { fetch(request: Request): Response { … } };
//
// The host's own modules below are a JS projection of the WIT package
// `flowcatalyst:function@0.1.1` (wit/flowcatalyst-function/function.wit),
// one module per WIT interface, with the same semantics: what a component
// function can do through `flowcatalyst:function/*`, a JS function can do
// through these modules. Outbound HTTP (`wasi:http` for a component) is the
// global `fetch`.
//
// What a function sees, and what it does not:
// - Globals: the web-platform subset declared at the end of this file
//   (Request, Response, Headers, fetch, URL, URLSearchParams, TextEncoder,
//   TextDecoder, atob, btoa, console, timers, crypto.getRandomValues and
//   crypto.randomUUID, structuredClone, queueMicrotask) and the ECMAScript
//   built-ins. There is no Node API (`process`, `require`, `Buffer`, `node:*`),
//   no filesystem, no environment, no `Deno`, no WebAssembly, and no network
//   other than `fetch`.
// - Imports: only the modules below. Everything else is bundled in (the
//   template builds with esbuild); any other import is refused at load
//   (JS_IMPORT_NOT_ALLOWED).
//
// The host's rules, which the types cannot express:
// - One isolate serves exactly one request. The bundle's top-level code
//   runs in every request's isolate, before the handler; module-level state
//   never survives from one request to the next.
// - The host APIs (config, secrets, events, invocation, fetch) work only
//   while a request is handled, not in top-level code. Logging works
//   everywhere.
// - The request and response bodies are buffered in full (no streams). The
//   request body is capped by the endpoint's `maxBodyBytes`, the response
//   body by the manifest's `limits.wasmMemoryMb`.
// - `limits.wasmMemoryMb` also caps the isolate's JavaScript heap (at least
//   8 MiB) and, separately, its ArrayBuffer storage. Past either, the call
//   ends with 500.
// - The call stops at the endpoint's `timeoutMs`, wherever the function is:
//   the caller gets 504. JavaScript is not preempted otherwise: a
//   computation holds its worker thread until it awaits.
// - A handler that throws, rejects, returns something that is not a
//   Response, or waits on a promise that can never settle answers
//   `500 {"error":"the function failed"}`; the error is only on the host's
//   log.
// - Settle everything before returning: work still pending when the
//   response is ready is dropped with the isolate.

/** The function's manifest-declared configuration (WIT `config`). */
declare module "flowcatalyst:function/config" {
  /**
   * The value of `key` when the manifest declares it under `config` and the
   * platform holds a value for it; `undefined` otherwise, including for a
   * key the platform holds but the manifest does not declare.
   */
  export function get(key: string): string | undefined;
}

/** The function's manifest-declared secrets (WIT `secrets`). */
declare module "flowcatalyst:function/secrets" {
  /**
   * The value of `key` when the manifest declares it under `secrets` and the
   * platform holds a non-empty value for it; `undefined` otherwise. The host
   * never logs a secret value, and neither should the function.
   */
  export function get(key: string): string | undefined;
}

/**
 * Structured log lines on the function's own logger (`fn.<address>`), each
 * carrying the invocation's fields (`function`, `version`, `execution_id`,
 * `correlation_id`) (WIT `log`). `console.*` writes there too: `debug` at
 * DEBUG, `log`/`info` at INFO, `warn` at WARN, `error` at ERROR, one line per
 * `\n`, split at 8 KiB.
 */
declare module "flowcatalyst:function/log" {
  export type Level = "trace" | "debug" | "info" | "warn" | "error";
  /** One line at `level`, verbatim. */
  export function log(level: Level, message: string): void;
  export function trace(message: string): void;
  export function debug(message: string): void;
  export function info(message: string): void;
  export function warn(message: string): void;
  export function error(message: string): void;
}

/** Publishing events to the platform, on the function's behalf (WIT `events`). */
declare module "flowcatalyst:function/events" {
  /** An event to publish: FlowCatalyst's `OutboundEvent` (WIT `outbound-event`). */
  export interface OutboundEvent {
    /** The event type code (`application:subdomain:aggregate:event`). The function's application must own it. */
    type: string;
    /** The CloudEvents `source`. Not yet carried by the platform's events route; accepted for forward compatibility. */
    source?: string;
    /** The CloudEvents `subject`. */
    subject?: string;
    /** The media type of `data`. Not yet carried by the platform's events route (the payload is always JSON). */
    dataContentType?: string;
    /** The payload: any JSON value (it is `JSON.stringify`-ed). Absent is an event with no payload. */
    data?: unknown;
    /** Links this event to the flow that caused it. Absent takes the invocation's correlation id. */
    correlationId?: string;
    /** The id of the event that directly caused this one. Absent takes the invocation's causation id. */
    causationId?: string;
    /** The ordering group the event belongs to. */
    messageGroup?: string;
    /** The id the platform deduplicates the event on. Required, never blank. */
    dedupId: string;
  }

  /** Why an event was not published (WIT `emit-event-error`). */
  export type EmitEventError =
    /**
     * The host refused the event before it reached the platform: `code` is
     * `INVALID_EVENT: type is required`, `INVALID_EVENT: data is not JSON` or
     * `DEDUP_ID_REQUIRED`.
     */
    | { kind: "invalid"; code: string }
    /**
     * The platform refused the event: its own code (for example
     * `EVENT_TYPE_NOT_OWNED`, `DEDUP_ID_DUPLICATE`), the HTTP status it
     * answered (a 5xx is worth a retry, a 4xx is not) and its message.
     */
    | { kind: "refused"; code: string; status: number; message: string }
    /** The platform could not be reached (code `UNAVAILABLE`, status 503); worth a retry. */
    | { kind: "unavailable"; message: string };

  /**
   * `result<string, emit-event-error>`: the id the platform stored the event
   * under, or why it was not published. The id is empty when the platform
   * accepted the event but its answer could not be read: the event is
   * stored, so never emit it again.
   */
  export type EmitResult = { ok: true; id: string } | { ok: false; error: EmitEventError };

  /**
   * Publishes one event through the host's control plane (WIT
   * `emit-event`). Resolves once the platform has accepted or refused it;
   * a refusal is a value, not a rejection. Rejects only for an argument that
   * is not an event object.
   */
  export function emit(event: OutboundEvent): Promise<EmitResult>;
}

/** What the host knows about the current invocation (WIT `invocation`). */
declare module "flowcatalyst:function/invocation" {
  /** An authenticated platform principal: a bearer token the host verified. */
  export interface Principal {
    /** The principal's id (JWT `sub`). */
    readonly id: string;
    /** For example `USER` or `SERVICE`. */
    readonly principalType: string;
    /** `ANCHOR`, `PARTNER` or `CLIENT`, when the token carries one. */
    readonly tier: string | null;
    /** Client ids the principal can access; `*` means every client. */
    readonly clients: readonly string[];
    readonly roles: readonly string[];
    /** Explicit application ids; ignored when `allApplications`. */
    readonly applications: readonly string[];
    readonly allApplications: boolean;
    /** Flattened permission codes, sorted. A `*` segment is a wildcard. */
    readonly permissions: readonly string[];
  }

  /** Who made the call, decided by the matched endpoint's `auth`. */
  export type Caller =
    /** `auth: webhook`: a platform delivery whose signature the host verified. */
    | { readonly kind: "platform" }
    /** `auth: none`: the host checked nothing. */
    | { readonly kind: "anonymous" }
    /** `auth: platform`: a verified platform bearer token. */
    | { readonly kind: "principal"; readonly principal: Principal };

  export interface InvocationContext {
    /** Unique per attempt: the `execution_id` on every log line. */
    readonly invocationId: string;
    /** The function's address, `application.service.function`. */
    readonly address: string;
    /** The version handling the call. */
    readonly version: number;
    readonly caller: Caller;
    /**
     * The correlation id events default to: the inbound event's own on a
     * verified webhook delivery, else the `X-Correlation-Id` header, else the
     * invocation id.
     */
    readonly correlationId: string;
    /** The causation id events default to: the inbound event's id on a verified webhook delivery of an event. */
    readonly causationId?: string;
    /** The `Host` (or `:authority`) the call arrived on. */
    readonly originalHost?: string;
    /** The request path as it arrived, before the host stripped its prefix. */
    readonly originalPath?: string;
    /** The TCP peer, or on the public listener the right-most `X-Forwarded-For` entry from a trusted proxy. */
    readonly remoteAddress?: string;
    /** The parameters the matched endpoint's path pattern bound, percent-decoded, in pattern order. */
    readonly pathParams: ReadonlyArray<readonly [name: string, value: string]>;
  }

  /** The current invocation. */
  export function context(): InvocationContext;
}

/** Every host module at once: `import { config, events } from "flowcatalyst:function"`. */
declare module "flowcatalyst:function" {
  export * as config from "flowcatalyst:function/config";
  export * as secrets from "flowcatalyst:function/secrets";
  export * as log from "flowcatalyst:function/log";
  export * as events from "flowcatalyst:function/events";
  export * as invocation from "flowcatalyst:function/invocation";
}

// ── the web-platform subset a function sees as globals ─────────────────
//
// A function's tsconfig uses `"lib": ["es2023"]` (no DOM): these are the
// only web APIs there are.

type BufferSource = ArrayBuffer | ArrayBufferView;
type BodyInit = string | BufferSource | URLSearchParams | null;
type HeadersInit = Headers | Record<string, string> | Iterable<readonly [string, string]>;

declare class Headers implements Iterable<[string, string]> {
  constructor(init?: HeadersInit);
  append(name: string, value: string): void;
  delete(name: string): void;
  /** Every value of `name`, joined with `, `; `null` when there is none. */
  get(name: string): string | null;
  /** Each `set-cookie` value, unjoined. */
  getSetCookie(): string[];
  has(name: string): boolean;
  set(name: string, value: string): void;
  forEach(callback: (value: string, name: string, headers: Headers) => void, thisArg?: unknown): void;
  entries(): IterableIterator<[string, string]>;
  keys(): IterableIterator<string>;
  values(): IterableIterator<string>;
  [Symbol.iterator](): IterableIterator<[string, string]>;
}

/** Bodies are buffered: there is no `body` stream (it is always `null`). */
interface Body {
  readonly body: null;
  readonly bodyUsed: boolean;
  arrayBuffer(): Promise<ArrayBuffer>;
  bytes(): Promise<Uint8Array>;
  text(): Promise<string>;
  json(): Promise<any>;
}

interface RequestInit {
  method?: string;
  headers?: HeadersInit;
  body?: BodyInit;
}

declare class Request implements Body {
  constructor(input: string | URL | Request, init?: RequestInit);
  /** Upper-cased for the standard methods. */
  readonly method: string;
  /** `http://<original host><path>?<query>` for the request a function handles. */
  readonly url: string;
  readonly headers: Headers;
  /** Redirects are never followed. */
  readonly redirect: "manual";
  readonly body: null;
  readonly bodyUsed: boolean;
  arrayBuffer(): Promise<ArrayBuffer>;
  bytes(): Promise<Uint8Array>;
  text(): Promise<string>;
  json(): Promise<any>;
  clone(): Request;
}

interface ResponseInit {
  /** 200–599; 200 by default. */
  status?: number;
  statusText?: string;
  headers?: HeadersInit;
}

declare class Response implements Body {
  constructor(body?: BodyInit, init?: ResponseInit);
  /** A JSON body (`JSON.stringify(data)`), `content-type: application/json` unless `init` sets one. */
  static json(data: unknown, init?: ResponseInit): Response;
  static redirect(url: string | URL, status?: 301 | 302 | 303 | 307 | 308): Response;
  readonly status: number;
  readonly statusText: string;
  readonly ok: boolean;
  readonly headers: Headers;
  /** The URL a `fetch` response came from; empty otherwise. */
  readonly url: string;
  readonly redirected: false;
  readonly type: "basic" | "default";
  readonly body: null;
  readonly bodyUsed: boolean;
  arrayBuffer(): Promise<ArrayBuffer>;
  bytes(): Promise<Uint8Array>;
  text(): Promise<string>;
  json(): Promise<any>;
  clone(): Response;
}

/**
 * What `fetch` rejects with when the call did not produce a response: its
 * `code` is the `wasi:http` error code the WASM runtime's guests see —
 * `HTTP-request-denied` for a host not on the manifest's `httpAllow`, or
 * plain `http` to a host other than loopback; `HTTP-response-timeout`,
 * `connection-timeout`, `connection-refused`, `HTTP-response-body-size`,
 * `HTTP-protocol-error`, `internal-error`, …
 */
interface HttpError extends TypeError {
  readonly name: "HttpError";
  readonly code: string;
}

/**
 * Outbound HTTP under the manifest's `httpAllow` (an exact host, or
 * `*.suffix` for subdomains, never the apex), over `https` (plain `http`
 * only to loopback). Redirects are returned, never followed. The timeout is
 * the smaller of the time left before the invocation's deadline and 30 s.
 * Bodies are buffered; the response's is capped by `limits.wasmMemoryMb`.
 * Rejects with an {@link HttpError}.
 */
declare function fetch(input: string | URL | Request, init?: RequestInit): Promise<Response>;

declare class URLSearchParams implements Iterable<[string, string]> {
  constructor(init?: string | URLSearchParams | Record<string, string> | Iterable<readonly [string, string]>);
  readonly size: number;
  append(name: string, value: string): void;
  delete(name: string, value?: string): void;
  get(name: string): string | null;
  getAll(name: string): string[];
  has(name: string, value?: string): boolean;
  set(name: string, value: string): void;
  sort(): void;
  forEach(callback: (value: string, name: string, params: URLSearchParams) => void, thisArg?: unknown): void;
  entries(): IterableIterator<[string, string]>;
  keys(): IterableIterator<string>;
  values(): IterableIterator<string>;
  [Symbol.iterator](): IterableIterator<[string, string]>;
  toString(): string;
}

/** A WHATWG URL. */
declare class URL {
  constructor(url: string | URL, base?: string | URL);
  static canParse(url: string | URL, base?: string | URL): boolean;
  static parse(url: string | URL, base?: string | URL): URL | null;
  href: string;
  readonly origin: string;
  protocol: string;
  username: string;
  password: string;
  host: string;
  hostname: string;
  port: string;
  pathname: string;
  search: string;
  readonly searchParams: URLSearchParams;
  hash: string;
  toString(): string;
  toJSON(): string;
}

declare class TextEncoder {
  readonly encoding: "utf-8";
  encode(input?: string): Uint8Array;
  encodeInto(input: string, destination: Uint8Array): { read: number; written: number };
}

/** UTF-8 only. */
declare class TextDecoder {
  constructor(label?: "utf-8" | "utf8", options?: { fatal?: boolean; ignoreBOM?: boolean });
  readonly encoding: "utf-8";
  readonly fatal: boolean;
  readonly ignoreBOM: boolean;
  decode(input?: BufferSource): string;
}

declare class DOMException extends Error {
  constructor(message?: string, name?: string);
}

declare function atob(data: string): string;
declare function btoa(data: string): string;

declare var console: {
  trace(...data: unknown[]): void;
  debug(...data: unknown[]): void;
  log(...data: unknown[]): void;
  info(...data: unknown[]): void;
  warn(...data: unknown[]): void;
  error(...data: unknown[]): void;
  dir(value: unknown): void;
  assert(condition?: boolean, ...data: unknown[]): void;
  count(label?: string): void;
  countReset(label?: string): void;
  time(label?: string): void;
  timeEnd(label?: string): void;
};

declare function setTimeout<A extends unknown[]>(callback: (...args: A) => void, delay?: number, ...args: A): number;
declare function clearTimeout(id: number | undefined): void;
declare function setInterval<A extends unknown[]>(callback: (...args: A) => void, delay?: number, ...args: A): number;
declare function clearInterval(id: number | undefined): void;
declare function queueMicrotask(callback: () => void): void;
declare function structuredClone<T>(value: T): T;

declare var crypto: {
  /** At most 65536 bytes. */
  getRandomValues<T extends Int8Array | Uint8Array | Uint8ClampedArray | Int16Array | Uint16Array | Int32Array | Uint32Array | BigInt64Array | BigUint64Array>(array: T): T;
  randomUUID(): `${string}-${string}-${string}-${string}-${string}`;
};

declare var self: typeof globalThis;

// The FlowCatalyst JS function runtime's bootstrap (fc-fnhost-js).
//
// Run once per loaded version, as a script, before the function's bundle,
// inside the isolate whose heap becomes the version's startup snapshot. It
// builds, from the host's ops:
//
// - the host API the `flowcatalyst:function/*` modules export (a JS
//   projection of `wit/flowcatalyst-function`: config, secrets, log,
//   events, invocation);
// - the web-platform subset a function sees as globals: `Request`,
//   `Response`, `Headers`, `fetch` (outbound HTTP under the manifest's
//   `httpAllow`), `URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder`,
//   `atob`, `btoa`, `console`, timers, `crypto.getRandomValues` /
//   `crypto.randomUUID`, `structuredClone`, `queueMicrotask`;
// - the dispatcher the host calls once per request.
//
// It leaves them on `globalThis.__fcHost`, which the host deletes once the
// bundle has been evaluated; the modules have captured what they need by
// then. There is no filesystem, no process, no environment and no network
// other than `fetch`: the only ops are the ones below, and each enforces
// the host's policy itself.

((globalThis) => {
  "use strict";

  const core = globalThis.Deno.core;
  const {
    op_fc_config_get,
    op_fc_secret_get,
    op_fc_log,
    op_fc_invocation,
    op_fc_emit,
    op_fc_fetch,
    op_fc_random_fill,
    op_fc_url_parse,
    op_fc_url_set,
    op_fc_utf8_valid,
  } = core.ops;

  const encodeUtf8 = (text) => core.encode(text);
  const decodeUtf8 = (bytes) => core.decode(bytes);

  // ── helpers ────────────────────────────────────────────────────────────

  const toBytes = (value, what) => {
    if (value instanceof Uint8Array) return value;
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (ArrayBuffer.isView(value)) {
      return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    }
    throw new TypeError(`${what} must be an ArrayBuffer or an ArrayBufferView`);
  };

  const copyBytes = (bytes) => {
    const out = new Uint8Array(bytes.byteLength);
    out.set(bytes);
    return out;
  };

  // A readable description of any value, for console lines and errors.
  const inspect = (value, depth = 0, seen = new Set()) => {
    switch (typeof value) {
      case "string":
        return depth === 0 ? value : JSON.stringify(value);
      case "number":
      case "boolean":
      case "undefined":
      case "symbol":
        return String(value);
      case "bigint":
        return `${value}n`;
      case "function":
        return `[Function ${value.name || "(anonymous)"}]`;
    }
    if (value === null) return "null";
    if (value instanceof Error) {
      return value.stack ? String(value.stack) : `${value.name}: ${value.message}`;
    }
    if (seen.has(value)) return "[Circular]";
    if (depth > 4) return Array.isArray(value) ? "[Array]" : "[Object]";
    seen.add(value);
    try {
      if (Array.isArray(value)) {
        return `[ ${value.map((v) => inspect(v, depth + 1, seen)).join(", ")} ]`;
      }
      if (value instanceof Uint8Array) {
        return `Uint8Array(${value.length}) [ ${Array.from(value.subarray(0, 16)).join(", ")}${value.length > 16 ? ", ..." : ""} ]`;
      }
      if (value instanceof Map) {
        return `Map(${value.size}) { ${Array.from(value, ([k, v]) => `${inspect(k, depth + 1, seen)} => ${inspect(v, depth + 1, seen)}`).join(", ")} }`;
      }
      if (value instanceof Set) {
        return `Set(${value.size}) { ${Array.from(value, (v) => inspect(v, depth + 1, seen)).join(", ")} }`;
      }
      if (value instanceof URL || value instanceof URLSearchParams) {
        return `${value.constructor.name} ${value.toString()}`;
      }
      const entries = Object.keys(value).map((k) =>
        `${/^[A-Za-z_$][\w$]*$/.test(k) ? k : JSON.stringify(k)}: ${inspect(value[k], depth + 1, seen)}`
      );
      const name = value.constructor && value.constructor !== Object ? `${value.constructor.name} ` : "";
      return entries.length ? `${name}{ ${entries.join(", ")} }` : `${name}{}`;
    } finally {
      seen.delete(value);
    }
  };

  // ── flowcatalyst:function/log and console ─────────────────────────────

  const LEVELS = ["trace", "debug", "info", "warn", "error"];
  const log = (level, message) => {
    if (!LEVELS.includes(level)) {
      throw new TypeError(`log level must be one of ${LEVELS.join(", ")}`);
    }
    op_fc_log(level, String(message), false);
  };

  const format = (args) => args.map((a) => inspect(a)).join(" ");
  const counts = new Map();
  const timers = new Map();
  const console = {
    trace: (...args) => op_fc_log("trace", format(args), true),
    debug: (...args) => op_fc_log("debug", format(args), true),
    log: (...args) => op_fc_log("info", format(args), true),
    info: (...args) => op_fc_log("info", format(args), true),
    warn: (...args) => op_fc_log("warn", format(args), true),
    error: (...args) => op_fc_log("error", format(args), true),
    dir: (value) => op_fc_log("info", inspect(value, 1), true),
    assert: (condition, ...args) => {
      if (!condition) {
        op_fc_log("error", `Assertion failed${args.length ? ": " + format(args) : ""}`, true);
      }
    },
    count: (label = "default") => {
      const n = (counts.get(label) ?? 0) + 1;
      counts.set(label, n);
      op_fc_log("info", `${label}: ${n}`, true);
    },
    countReset: (label = "default") => counts.delete(label),
    time: (label = "default") => timers.set(label, Date.now()),
    timeEnd: (label = "default") => {
      const start = timers.get(label);
      if (start !== undefined) {
        timers.delete(label);
        op_fc_log("info", `${label}: ${Date.now() - start}ms`, true);
      }
    },
  };

  // ── flowcatalyst:function/config, secrets, invocation ─────────────────

  const config = Object.freeze({
    get: (key) => op_fc_config_get(String(key)) ?? undefined,
  });
  const secrets = Object.freeze({
    get: (key) => op_fc_secret_get(String(key)) ?? undefined,
  });
  const invocation = Object.freeze({
    context: () => {
      const c = op_fc_invocation();
      const caller = c.caller.kind === "principal"
        ? Object.freeze({ kind: "principal", principal: Object.freeze(c.caller.principal) })
        : Object.freeze({ kind: c.caller.kind });
      return Object.freeze({
        invocationId: c.invocationId,
        address: c.address,
        version: c.version,
        caller,
        correlationId: c.correlationId,
        causationId: c.causationId ?? undefined,
        originalHost: c.originalHost ?? undefined,
        originalPath: c.originalPath ?? undefined,
        remoteAddress: c.remoteAddress ?? undefined,
        pathParams: Object.freeze(c.pathParams.map((p) => Object.freeze(p))),
      });
    },
  });

  // ── flowcatalyst:function/events ──────────────────────────────────────

  const optionalString = (event, key) => {
    const value = event[key];
    if (value === undefined || value === null) return null;
    if (typeof value !== "string") throw new TypeError(`event.${key} must be a string`);
    return value;
  };

  const events = Object.freeze({
    // `emit-event` (0.1.1): resolves to `{ ok: true, id }` or
    // `{ ok: false, error }`; never rejects for a refusal.
    emit: async (event) => {
      if (event === null || typeof event !== "object") {
        throw new TypeError("emit takes an event object");
      }
      let data = null;
      if (event.data !== undefined) {
        try {
          // `undefined` (a function, a symbol) is not JSON either.
          data = JSON.stringify(event.data) ?? "undefined";
        } catch {
          data = "undefined";
        }
      }
      return await op_fc_emit({
        type: typeof event.type === "string" ? event.type : "",
        source: optionalString(event, "source"),
        subject: optionalString(event, "subject"),
        dataContentType: optionalString(event, "dataContentType"),
        data,
        correlationId: optionalString(event, "correlationId"),
        causationId: optionalString(event, "causationId"),
        messageGroup: optionalString(event, "messageGroup"),
        dedupId: typeof event.dedupId === "string" ? event.dedupId : "",
      });
    },
  });

  // ── base64, text encoding ─────────────────────────────────────────────

  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const B64_INDEX = new Map(Array.from(B64, (c, i) => [c, i]));

  const btoa = (data) => {
    const text = String(data);
    let out = "";
    for (let i = 0; i < text.length; i += 3) {
      const codes = [text.charCodeAt(i), text.charCodeAt(i + 1), text.charCodeAt(i + 2)];
      for (const [j, code] of codes.entries()) {
        if (i + j < text.length && code > 0xff) {
          throw new DOMException("btoa: the string contains characters outside of Latin1", "InvalidCharacterError");
        }
      }
      const [a, b, c] = codes;
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] +
        (i + 1 < text.length ? B64[(n >> 6) & 63] : "=") +
        (i + 2 < text.length ? B64[n & 63] : "=");
    }
    return out;
  };

  const atob = (data) => {
    let text = String(data).replace(/[\t\n\f\r ]/g, "");
    if (text.length % 4 === 0) text = text.replace(/==?$/, "");
    if (text.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(text)) {
      throw new DOMException("atob: the string is not correctly encoded", "InvalidCharacterError");
    }
    let out = "";
    let buffer = 0;
    let bits = 0;
    for (const c of text) {
      buffer = (buffer << 6) | B64_INDEX.get(c);
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((buffer >> bits) & 0xff);
      }
    }
    return out;
  };

  class DOMException extends Error {
    constructor(message = "", name = "Error") {
      super(message);
      Object.defineProperty(this, "name", { value: name, configurable: true, writable: true });
    }
  }

  class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(input = "") {
      return encodeUtf8(String(input));
    }
    encodeInto(input, destination) {
      const bytes = encodeUtf8(String(input));
      // Whole characters only: never cut a sequence in half.
      let written = Math.min(bytes.length, destination.length);
      while (written > 0 && written < bytes.length && (bytes[written] & 0xc0) === 0x80) written--;
      destination.set(bytes.subarray(0, written));
      const read = decodeUtf8(bytes.subarray(0, written)).length;
      return { read, written };
    }
  }

  class TextDecoder {
    #fatal;
    #ignoreBOM;
    constructor(label = "utf-8", options = {}) {
      const normal = String(label).trim().toLowerCase();
      if (normal !== "utf-8" && normal !== "utf8" && normal !== "unicode-1-1-utf-8") {
        throw new RangeError(`TextDecoder: only utf-8 is supported, not '${label}'`);
      }
      this.#fatal = Boolean(options.fatal);
      this.#ignoreBOM = Boolean(options.ignoreBOM);
    }
    get encoding() {
      return "utf-8";
    }
    get fatal() {
      return this.#fatal;
    }
    get ignoreBOM() {
      return this.#ignoreBOM;
    }
    decode(input = new Uint8Array(0)) {
      let bytes = toBytes(input, "TextDecoder.decode's input");
      if (this.#fatal && !op_fc_utf8_valid(bytes)) {
        throw new TypeError("TextDecoder: the input is not valid utf-8");
      }
      if (!this.#ignoreBOM && bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
        bytes = bytes.subarray(3);
      }
      return decodeUtf8(bytes);
    }
  }

  // ── URL, URLSearchParams ──────────────────────────────────────────────

  const formEncode = (text) => {
    let out = "";
    for (const byte of encodeUtf8(String(text))) {
      if (byte === 0x20) out += "+";
      else if (
        (byte >= 0x30 && byte <= 0x39) || (byte >= 0x41 && byte <= 0x5a) ||
        (byte >= 0x61 && byte <= 0x7a) || byte === 0x2a || byte === 0x2d || byte === 0x2e || byte === 0x5f
      ) out += String.fromCharCode(byte);
      else out += "%" + byte.toString(16).toUpperCase().padStart(2, "0");
    }
    return out;
  };

  const percentDecode = (text) => {
    const bytes = encodeUtf8(text);
    const out = new Uint8Array(bytes.length);
    let n = 0;
    const hex = (b) => (b >= 0x30 && b <= 0x39) || (b >= 0x41 && b <= 0x46) || (b >= 0x61 && b <= 0x66);
    for (let i = 0; i < bytes.length; i++) {
      if (bytes[i] === 0x25 && i + 2 < bytes.length && hex(bytes[i + 1]) && hex(bytes[i + 2])) {
        out[n++] = parseInt(String.fromCharCode(bytes[i + 1], bytes[i + 2]), 16);
        i += 2;
      } else {
        out[n++] = bytes[i];
      }
    }
    return decodeUtf8(out.subarray(0, n));
  };

  const parseForm = (text) => {
    const pairs = [];
    for (const piece of String(text).split("&")) {
      if (piece === "") continue;
      const at = piece.indexOf("=");
      const name = at === -1 ? piece : piece.slice(0, at);
      const value = at === -1 ? "" : piece.slice(at + 1);
      pairs.push([percentDecode(name.replace(/\+/g, " ")), percentDecode(value.replace(/\+/g, " "))]);
    }
    return pairs;
  };

  class URLSearchParams {
    #pairs = [];
    #url = null;
    constructor(init = "") {
      if (init instanceof URLSearchParams) {
        this.#pairs = init.#pairs.map(([k, v]) => [k, v]);
      } else if (typeof init === "object" && init !== null) {
        if (typeof init[Symbol.iterator] === "function") {
          for (const pair of init) {
            const [k, v, ...rest] = Array.from(pair);
            if (rest.length || v === undefined) {
              throw new TypeError("URLSearchParams: each pair must have exactly two items");
            }
            this.#pairs.push([String(k), String(v)]);
          }
        } else {
          for (const key of Object.keys(init)) this.#pairs.push([key, String(init[key])]);
        }
      } else {
        const text = String(init);
        this.#pairs = parseForm(text.startsWith("?") ? text.slice(1) : text);
      }
    }
    static _attach(params, url) {
      params.#url = url;
    }
    static _reset(params, search) {
      params.#pairs = parseForm(search.startsWith("?") ? search.slice(1) : search);
    }
    #changed() {
      if (this.#url) URL._setSearch(this.#url, this.toString());
    }
    get size() {
      return this.#pairs.length;
    }
    append(name, value) {
      this.#pairs.push([String(name), String(value)]);
      this.#changed();
    }
    delete(name, value) {
      name = String(name);
      this.#pairs = this.#pairs.filter(([k, v]) => !(k === name && (value === undefined || v === String(value))));
      this.#changed();
    }
    get(name) {
      name = String(name);
      const found = this.#pairs.find(([k]) => k === name);
      return found ? found[1] : null;
    }
    getAll(name) {
      name = String(name);
      return this.#pairs.filter(([k]) => k === name).map(([, v]) => v);
    }
    has(name, value) {
      name = String(name);
      return this.#pairs.some(([k, v]) => k === name && (value === undefined || v === String(value)));
    }
    set(name, value) {
      name = String(name);
      value = String(value);
      const at = this.#pairs.findIndex(([k]) => k === name);
      if (at === -1) this.#pairs.push([name, value]);
      else {
        this.#pairs[at][1] = value;
        this.#pairs = this.#pairs.filter(([k], i) => k !== name || i === at);
      }
      this.#changed();
    }
    sort() {
      this.#pairs = this.#pairs
        .map((p, i) => [p, i])
        .sort((a, b) => (a[0][0] < b[0][0] ? -1 : a[0][0] > b[0][0] ? 1 : a[1] - b[1]))
        .map(([p]) => p);
      this.#changed();
    }
    forEach(callback, thisArg) {
      for (const [k, v] of this.#pairs) callback.call(thisArg, v, k, this);
    }
    *entries() {
      for (const [k, v] of this.#pairs) yield [k, v];
    }
    *keys() {
      for (const [k] of this.#pairs) yield k;
    }
    *values() {
      for (const [, v] of this.#pairs) yield v;
    }
    [Symbol.iterator]() {
      return this.entries();
    }
    toString() {
      return this.#pairs.map(([k, v]) => `${formEncode(k)}=${formEncode(v)}`).join("&");
    }
  }

  // [href, origin, protocol, username, password, host, hostname, port,
  // pathname, search, hash], parsed by the `url` crate (WHATWG URL).
  class URL {
    #parts;
    #searchParams;
    constructor(url, base) {
      const parts = op_fc_url_parse(String(url), base === undefined ? "" : String(base));
      if (parts === null) throw new TypeError(`Invalid URL: '${url}'`);
      this.#parts = parts;
      this.#searchParams = new URLSearchParams(parts[9]);
      URLSearchParams._attach(this.#searchParams, this);
    }
    static canParse(url, base) {
      return op_fc_url_parse(String(url), base === undefined ? "" : String(base)) !== null;
    }
    static parse(url, base) {
      try {
        return new URL(url, base);
      } catch {
        return null;
      }
    }
    // A URLSearchParams change writes the query back.
    static _setSearch(url, search) {
      url.#set("search", search, false);
    }
    #set(part, value, resync = true) {
      const parts = op_fc_url_set(this.#parts[0], part, String(value));
      if (parts === null) {
        if (part === "href") throw new TypeError(`Invalid URL: '${value}'`);
        return;
      }
      this.#parts = parts;
      if (resync) URLSearchParams._reset(this.#searchParams, parts[9]);
    }
    get href() { return this.#parts[0]; }
    set href(value) { this.#set("href", value); }
    get origin() { return this.#parts[1]; }
    get protocol() { return this.#parts[2]; }
    set protocol(value) { this.#set("protocol", value); }
    get username() { return this.#parts[3]; }
    set username(value) { this.#set("username", value); }
    get password() { return this.#parts[4]; }
    set password(value) { this.#set("password", value); }
    get host() { return this.#parts[5]; }
    set host(value) { this.#set("host", value); }
    get hostname() { return this.#parts[6]; }
    set hostname(value) { this.#set("hostname", value); }
    get port() { return this.#parts[7]; }
    set port(value) { this.#set("port", value); }
    get pathname() { return this.#parts[8]; }
    set pathname(value) { this.#set("pathname", value); }
    get search() { return this.#parts[9]; }
    set search(value) { this.#set("search", value); }
    get hash() { return this.#parts[10]; }
    set hash(value) { this.#set("hash", value); }
    get searchParams() {
      return this.#searchParams;
    }
    toString() {
      return this.#parts[0];
    }
    toJSON() {
      return this.#parts[0];
    }
  }

  // ── Headers ───────────────────────────────────────────────────────────

  const TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  const normaliseValue = (value) => {
    const text = String(value).replace(/^[\t\n\r ]+|[\t\n\r ]+$/g, "");
    if (/[\0\r\n]/.test(text) || /[^\u0000-ÿ]/.test(text)) {
      throw new TypeError(`Headers: invalid header value '${text}'`);
    }
    return text;
  };
  const normaliseName = (name) => {
    const text = String(name);
    if (!TOKEN.test(text)) throw new TypeError(`Headers: invalid header name '${text}'`);
    return text.toLowerCase();
  };

  class Headers {
    // Every [name, value] as appended, names lower-cased, so repeated
    // headers (set-cookie) reach the wire one line each.
    #list = [];
    constructor(init) {
      if (init === undefined || init === null) return;
      if (init instanceof Headers) {
        this.#list = init.#list.map(([k, v]) => [k, v]);
      } else if (typeof init[Symbol.iterator] === "function") {
        for (const pair of init) {
          const [k, v, ...rest] = Array.from(pair);
          if (rest.length || v === undefined) throw new TypeError("Headers: each pair must have exactly two items");
          this.append(k, v);
        }
      } else if (typeof init === "object") {
        for (const key of Object.keys(init)) this.append(key, init[key]);
      } else {
        throw new TypeError("Headers: init must be an object, an iterable of pairs or a Headers");
      }
    }
    static _list(headers) {
      return headers.#list;
    }
    append(name, value) {
      this.#list.push([normaliseName(name), normaliseValue(value)]);
    }
    delete(name) {
      name = normaliseName(name);
      this.#list = this.#list.filter(([k]) => k !== name);
    }
    get(name) {
      name = normaliseName(name);
      const values = this.#list.filter(([k]) => k === name).map(([, v]) => v);
      return values.length ? values.join(", ") : null;
    }
    getSetCookie() {
      return this.#list.filter(([k]) => k === "set-cookie").map(([, v]) => v);
    }
    has(name) {
      name = normaliseName(name);
      return this.#list.some(([k]) => k === name);
    }
    set(name, value) {
      name = normaliseName(name);
      value = normaliseValue(value);
      const at = this.#list.findIndex(([k]) => k === name);
      if (at === -1) this.#list.push([name, value]);
      else {
        this.#list[at][1] = value;
        this.#list = this.#list.filter(([k], i) => k !== name || i === at);
      }
    }
    *entries() {
      const names = [...new Set(this.#list.map(([k]) => k))].sort();
      for (const name of names) {
        if (name === "set-cookie") {
          for (const value of this.getSetCookie()) yield [name, value];
        } else {
          yield [name, this.get(name)];
        }
      }
    }
    *keys() {
      for (const [k] of this.entries()) yield k;
    }
    *values() {
      for (const [, v] of this.entries()) yield v;
    }
    forEach(callback, thisArg) {
      for (const [k, v] of this.entries()) callback.call(thisArg, v, k, this);
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  }

  // ── bodies, Request, Response ─────────────────────────────────────────

  // [bytes, default content type]
  const extractBody = (body) => {
    if (body === undefined || body === null) return [null, null];
    if (typeof body === "string") return [encodeUtf8(body), "text/plain;charset=UTF-8"];
    if (body instanceof URLSearchParams) {
      return [encodeUtf8(body.toString()), "application/x-www-form-urlencoded;charset=UTF-8"];
    }
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) {
      return [copyBytes(toBytes(body, "a body")), null];
    }
    throw new TypeError("a body must be a string, an ArrayBuffer, an ArrayBufferView or URLSearchParams (streams are not supported: bodies are buffered)");
  };

  class Body {
    #bytes;
    #used = false;
    _initBody(bytes) {
      this.#bytes = bytes;
    }
    get body() {
      return null;
    }
    get bodyUsed() {
      return this.#used;
    }
    _peekBytes() {
      return this.#bytes;
    }
    #consume() {
      if (this.#used) throw new TypeError("the body has already been read");
      this.#used = true;
      return this.#bytes ?? new Uint8Array(0);
    }
    async arrayBuffer() {
      const bytes = this.#consume();
      return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    }
    async bytes() {
      return copyBytes(this.#consume());
    }
    async text() {
      return decodeUtf8(this.#consume());
    }
    async json() {
      return JSON.parse(decodeUtf8(this.#consume()));
    }
    async formData() {
      throw new TypeError("formData() is not supported");
    }
    async blob() {
      throw new TypeError("blob() is not supported");
    }
  }

  const METHODS = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH"];

  class Request extends Body {
    #method;
    #url;
    #headers;
    constructor(input, init = {}) {
      super();
      let base = null;
      if (input instanceof Request) {
        base = input;
        this.#url = input.url;
      } else {
        this.#url = new URL(String(input)).toString();
      }
      const method = init.method ?? base?.method ?? "GET";
      const upper = String(method).toUpperCase();
      this.#method = METHODS.includes(upper) ? upper : String(method);
      if (!TOKEN.test(this.#method)) throw new TypeError(`Request: invalid method '${method}'`);
      this.#headers = new Headers(init.headers ?? base?.headers);
      let bytes;
      if (init.body !== undefined) {
        const [extracted, type] = extractBody(init.body);
        bytes = extracted;
        if (type && !this.#headers.has("content-type")) this.#headers.set("content-type", type);
      } else {
        bytes = base ? base._peekBytes() : null;
      }
      if (bytes !== null && bytes !== undefined && (this.#method === "GET" || this.#method === "HEAD") && !Request._internal) {
        throw new TypeError("Request: a GET or HEAD request cannot have a body");
      }
      this._initBody(bytes ?? null);
    }
    get method() {
      return this.#method;
    }
    get url() {
      return this.#url;
    }
    get headers() {
      return this.#headers;
    }
    get redirect() {
      return "manual";
    }
    clone() {
      if (this.bodyUsed) throw new TypeError("Request.clone: the body has already been read");
      Request._internal = true;
      try {
        return new Request(this);
      } finally {
        Request._internal = false;
      }
    }
  }
  Request._internal = false;

  const REDIRECTS = [301, 302, 303, 307, 308];

  class Response extends Body {
    #status;
    #statusText;
    #headers;
    #url = "";
    constructor(body = null, init = {}) {
      super();
      const status = init.status === undefined ? 200 : Number(init.status);
      if (!Number.isInteger(status) || status < (Response._internal ? 100 : 200) || status > 599) {
        throw new RangeError(`Response: status ${init.status} is outside 200-599`);
      }
      this.#status = status;
      this.#statusText = init.statusText === undefined ? "" : String(init.statusText);
      this.#headers = new Headers(init.headers);
      const [bytes, type] = extractBody(body);
      if (bytes !== null && [101, 204, 205, 304].includes(status)) {
        throw new TypeError(`Response: a ${status} response cannot have a body`);
      }
      if (type && !this.#headers.has("content-type")) this.#headers.set("content-type", type);
      this._initBody(bytes);
    }
    static json(data, init = {}) {
      const text = JSON.stringify(data);
      if (text === undefined) throw new TypeError("Response.json: the value is not JSON");
      const headers = new Headers(init.headers);
      if (!headers.has("content-type")) headers.set("content-type", "application/json");
      return new Response(text, { ...init, headers });
    }
    static redirect(url, status = 302) {
      if (!REDIRECTS.includes(status)) throw new RangeError(`Response.redirect: ${status} is not a redirect status`);
      return new Response(null, { status, headers: { location: new URL(String(url)).toString() } });
    }
    static _fetched(url, status, statusText, headers, body) {
      Response._internal = true;
      try {
        const response = new Response(null, { status, statusText, headers });
        response._initBody(body);
        response.#url = url;
        return response;
      } finally {
        Response._internal = false;
      }
    }
    get status() {
      return this.#status;
    }
    get statusText() {
      return this.#statusText;
    }
    get ok() {
      return this.#status >= 200 && this.#status <= 299;
    }
    get headers() {
      return this.#headers;
    }
    get url() {
      return this.#url;
    }
    get redirected() {
      return false;
    }
    get type() {
      return this.#url ? "basic" : "default";
    }
    clone() {
      if (this.bodyUsed) throw new TypeError("Response.clone: the body has already been read");
      const copy = Response._fetched(this.#url, this.#status, this.#statusText, this.#headers, this._peekBytes());
      copy.#url = this.#url;
      return copy;
    }
  }
  Response._internal = false;

  // ── fetch: outbound HTTP under the manifest's httpAllow ───────────────

  class HttpError extends TypeError {
    constructor(code, message) {
      super(message);
      this.name = "HttpError";
      // The wasi:http error-code this failure is, e.g. `HTTP-request-denied`.
      this.code = code;
    }
  }

  const fetch = async (input, init = undefined) => {
    const request = input instanceof Request && init === undefined ? input : new Request(input, init ?? {});
    if (request.bodyUsed) throw new TypeError("fetch: the request body has already been read");
    const body = request._peekBytes();
    const result = await op_fc_fetch({
      method: request.method,
      url: request.url,
      headers: Headers._list(request.headers),
      body: body ?? null,
    });
    if (!result.ok) throw new HttpError(result.code, result.message);
    return Response._fetched(request.url, result.status, result.statusText, result.headers, result.body);
  };

  // ── timers, crypto ────────────────────────────────────────────────────

  const liveTimers = new Map();
  let nextTimerId = 1;
  const schedule = (callback, delay, args, repeat) => {
    if (typeof callback !== "function") throw new TypeError("a timer callback must be a function");
    const id = nextTimerId++;
    const timer = core.createTimer(
      () => {
        if (!repeat) liveTimers.delete(id);
        callback(...args);
      },
      Number(delay) || 0,
      undefined,
      repeat,
      true,
    );
    liveTimers.set(id, timer);
    return id;
  };
  const clearTimer = (id) => {
    const timer = liveTimers.get(id);
    if (timer !== undefined) {
      liveTimers.delete(id);
      core.cancelTimer(timer);
    }
  };

  const getRandomValues = (array) => {
    if (!(ArrayBuffer.isView(array)) || array instanceof Float32Array || array instanceof Float64Array || array instanceof DataView) {
      throw new TypeError("crypto.getRandomValues takes an integer TypedArray");
    }
    if (array.byteLength > 65536) {
      throw new DOMException("crypto.getRandomValues: at most 65536 bytes", "QuotaExceededError");
    }
    op_fc_random_fill(new Uint8Array(array.buffer, array.byteOffset, array.byteLength));
    return array;
  };
  const randomUUID = () => {
    const b = new Uint8Array(16);
    op_fc_random_fill(b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    const h = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
    return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
  };

  // ── the dispatcher the host calls ─────────────────────────────────────

  const describe = (value) =>
    value === null ? "null" : typeof value === "object" ? (value.constructor?.name ?? "an object") : typeof value;

  // The entry's handler: a function `(request) => Response`, or an object
  // with a `fetch(request)` method. Refused at load when neither.
  const dispatcher = (namespace, entrypoint) => {
    const entry = namespace[entrypoint];
    let handler;
    if (typeof entry === "function") handler = entry;
    else if (entry !== null && typeof entry === "object" && typeof entry.fetch === "function") {
      handler = (request) => entry.fetch(request);
    } else {
      const error = new TypeError(
        entry === undefined
          ? `the bundle does not export '${entrypoint}'`
          : `the export '${entrypoint}' is ${describe(entry)}, not a function or an object with a fetch method`,
      );
      error.name = "EntrypointError";
      throw error;
    }
    return async (method, url, headers, body) => {
      Request._internal = true;
      let request;
      try {
        request = new Request(url, { method, headers: [] });
      } finally {
        Request._internal = false;
      }
      const list = Headers._list(request.headers);
      for (const [name, value] of headers) list.push([name.toLowerCase(), value]);
      request._initBody(body.byteLength || !(method === "GET" || method === "HEAD") ? body : null);
      const response = await handler(request);
      if (!(response instanceof Response)) {
        throw new TypeError(`the handler must return a Response, not ${describe(response)}`);
      }
      return [response.status, Headers._list(response.headers), response._peekBytes() ?? new Uint8Array(0)];
    };
  };

  // ── install ───────────────────────────────────────────────────────────

  const globals = {
    Headers,
    Request,
    Response,
    URL,
    URLSearchParams,
    TextEncoder,
    TextDecoder,
    DOMException,
    fetch,
    atob,
    btoa,
    console,
    setTimeout: (callback, delay = 0, ...args) => schedule(callback, delay, args, false),
    clearTimeout: clearTimer,
    setInterval: (callback, delay = 0, ...args) => schedule(callback, delay, args, true),
    clearInterval: clearTimer,
    structuredClone: (value) => core.structuredClone(value),
    crypto: Object.freeze({ getRandomValues, randomUUID }),
  };
  for (const [name, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, name, { value, writable: true, enumerable: false, configurable: true });
  }
  // WebAssembly is off inside JS functions: a `WebAssembly.Memory` is
  // allocated outside both the heap limit and the ArrayBuffer budget, so it
  // would escape the function's memory limit (WASM functions are the
  // `component` runtime).
  delete globalThis.WebAssembly;
  Object.defineProperty(globalThis, "self", { value: globalThis, writable: true, enumerable: false, configurable: true });

  Object.defineProperty(globalThis, "__fcHost", {
    value: Object.freeze({
      config,
      secrets,
      log: Object.freeze({
        log,
        trace: (message) => log("trace", message),
        debug: (message) => log("debug", message),
        info: (message) => log("info", message),
        warn: (message) => log("warn", message),
        error: (message) => log("error", message),
      }),
      events,
      invocation,
      dispatcher,
    }),
    writable: false,
    enumerable: false,
    configurable: true,
  });
})(globalThis);

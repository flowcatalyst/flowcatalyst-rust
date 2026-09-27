// The JS runtime's test guest: one bundle, its behaviour chosen by the
// request path (a component has one entrypoint, and so, here, does a
// bundle). Hand-written, no bundler: it imports only the host's modules.
import { get as configGet } from "flowcatalyst:function/config";
import { get as secretGet } from "flowcatalyst:function/secrets";
import { log, info } from "flowcatalyst:function/log";
import { emit } from "flowcatalyst:function/events";
import { context } from "flowcatalyst:function/invocation";
import * as all from "flowcatalyst:function";

// Top-level state: in the snapshot, fresh for every request.
let counter = 0;
const table = Array.from({ length: 100 }, (_, i) => i * i);
console.log("guest loaded");

const json = (value, init) => Response.json(value, init);

async function handle(request) {
  counter++;
  const url = new URL(request.url);
  const q = url.searchParams;
  switch (url.pathname) {
    case "/echo": {
      const body = await request.text();
      const headers = {};
      for (const [k, v] of request.headers) headers[k] = v;
      return json({
        method: request.method,
        url: request.url,
        path: url.pathname,
        query: Object.fromEntries(q),
        queryAll: q.getAll("y"),
        headers,
        body,
        context: context(),
      }, { headers: [["x-guest", "echo"], ["x-guest", "twice"]] });
    }
    case "/counter":
      return json({ counter, table: table.length, random: Math.random() });
    case "/config":
      return json({ value: configGet(q.get("key")) ?? null, viaAll: all.config.get(q.get("key")) ?? null });
    case "/secret":
      return json({ present: secretGet(q.get("key")) !== undefined, length: (secretGet(q.get("key")) ?? "").length });
    case "/emit": {
      const event = {
        type: q.get("type") ?? "app:orders:order:shipped",
        dedupId: q.get("dedup") ?? "d-1",
        subject: "order/1",
        data: q.has("nodata") ? undefined : { id: 1, ok: true },
      };
      if (q.has("correlation")) event.correlationId = q.get("correlation");
      if (q.has("badData")) event.data = () => 1;
      return json(await emit(event));
    }
    case "/http": {
      try {
        const response = await fetch(q.get("url"), { headers: { "x-from": "guest" } });
        return json({ status: response.status, body: await response.text(), header: response.headers.get("x-upstream") });
      } catch (e) {
        return json({ error: e.name, code: e.code, message: e.message });
      }
    }
    case "/log":
      console.log("line one\nline two");
      console.error("an error line");
      log("warn", "a warn line");
      info("an info line");
      return new Response("logged");
    case "/spin":
      for (;;) {}
    case "/busy": {
      const until = Date.now() + Number(q.get("ms") ?? 200);
      while (Date.now() < until) {}
      return new Response("busy");
    }
    case "/sleep":
      await new Promise((resolve) => setTimeout(resolve, Number(q.get("ms") ?? 50)));
      return new Response("slept");
    case "/alloc": {
      const hoard = [];
      for (;;) hoard.push(new Array(100000).fill(hoard.length));
    }
    case "/buffer": {
      try {
        const buffers = [];
        for (let i = 0; i < 1000; i++) buffers.push(new Uint8Array(1 << 20));
        return new Response("no limit");
      } catch (e) {
        return json({ error: e.name, message: e.message });
      }
    }
    case "/throw":
      throw new Error("the guest threw: secret-ish internals");
    case "/reject":
      return Promise.reject(new TypeError("rejected"));
    case "/not-response":
      return { status: 200 };
    case "/never":
      return new Promise(() => {});
    case "/big":
      return new Response(new Uint8Array(Number(q.get("bytes"))));
    case "/status":
      return new Response(null, { status: Number(q.get("code")), headers: { "set-cookie": "a=1" } });
    case "/binary": {
      const bytes = new Uint8Array(await request.arrayBuffer());
      return new Response(bytes.reverse(), { headers: { "content-type": "application/octet-stream" } });
    }
    case "/globals":
      return json({
        deno: typeof Deno,
        host: typeof globalThis.__fcHost,
        process: typeof process,
        require: typeof require,
        wasm: typeof WebAssembly,
        uuid: /^[0-9a-f-]{36}$/.test(crypto.randomUUID()),
        encoded: new TextDecoder().decode(new TextEncoder().encode("héllo")),
        base64: btoa("hi") + atob("aGk="),
        url: new URL("../b?x=1#h", "https://example.test/a/c").href,
        cloned: structuredClone({ a: [1, 2] }),
      });
    case "/top-level-api":
      return new Response("ok");
    default:
      return new Response("not found", { status: 404 });
  }
}

export default handle;
export const named = (request) => new Response(`named ${new URL(request.url).pathname}`);
export const objectStyle = { fetch: (request) => new Response(`object ${request.method}`) };
export const notAHandler = 42;

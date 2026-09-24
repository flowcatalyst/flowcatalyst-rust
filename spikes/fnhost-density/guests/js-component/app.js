// (d) StarlingMonkey (SpiderMonkey) JS guest via componentize-js: a wasi:http/proxy handler with
// the same echo/spin/alloc behaviours, written against the web-standard fetch event.
let calls = 0;
// Guard: the module body runs at build-time pre-initialisation AND again at runtime, which would
// register the listener twice ("respondWith can't be called twice").
if (!globalThis.__fcListener) {
  globalThis.__fcListener = true;
  addEventListener("fetch", (event) => event.respondWith(route(event.request)));
}

async function route(req) {
  calls++;
  const url = new URL(req.url);
  const json = (v, status = 200) =>
    new Response(JSON.stringify(v), { status, headers: { "content-type": "application/json" } });
  switch (url.pathname) {
    case "/echo": {
      const body = await req.text();
      return json({ method: req.method, path: url.pathname + url.search, headers: [...req.headers], body, calls });
    }
    case "/spin": {
      if (url.searchParams.get("spin") === "false") return json({ spun: false });
      let n = 0;
      for (;;) { n = (n + 1) | 0; }
    }
    case "/alloc": {
      const mb = parseInt(url.searchParams.get("mb") || "1", 10);
      const a = new Uint8Array(mb * 1024 * 1024).fill(7);
      let sum = 0;
      for (let i = 0; i < a.length; i += 4096) sum += a[i];
      return json({ allocatedMb: mb, sum });
    }
    default:
      return json({ error: "no such path" }, 404);
  }
}

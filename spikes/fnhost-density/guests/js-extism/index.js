// (d) QuickJS guest through extism-js: the same echo/spin/alloc behaviours as the Rust fixture.
function reply(status, body) {
  return JSON.stringify({ status, headers: { "content-type": ["application/json"] }, body });
}
function query(req, name) {
  const v = req.query && req.query[name];
  return v && v.length ? v[0] : undefined;
}
function echo() {
  Host.outputString(reply(200, Host.inputString()));
  return 0;
}
function spin() {
  const req = JSON.parse(Host.inputString());
  if (query(req, "spin") === "false") { Host.outputString(reply(200, '{"spun":false}')); return 0; }
  let n = 0;
  for (;;) { n = (n + 1) | 0; }
}
function alloc() {
  const req = JSON.parse(Host.inputString());
  const mb = parseInt(query(req, "mb") || "1", 10);
  const a = new Uint8Array(mb * 1024 * 1024).fill(7);
  let sum = 0;
  for (let i = 0; i < a.length; i += 4096) sum += a[i];
  Host.outputString(reply(200, JSON.stringify({ allocatedMb: mb, sum })));
  return 0;
}
module.exports = { echo, spin, alloc };

# Functions

A FlowCatalyst function is a small unit of your own code that the platform runs for you: a WASI 0.2
component (built in Rust with [`fc-function-pdk`](../../crates/fc-function-pdk/README.md)) that answers
HTTP requests, event deliveries and schedules. This page covers what the manifest's limits mean and how
a function reaches a database. The engine and hosting design is in
[`../function-runner-plan.md`](../function-runner-plan.md); a complete function is
[`examples/function-hello-rust`](../../examples/function-hello-rust/).

```sh
fc-dev fn init --runtime wasm --lang rust hello && cd hello
fc-dev fn build
fc-dev fn deploy target/wasm32-wasip2/release/hello.wasm shop.default.hello
fc-dev fn invoke shop.default.hello --path /hello/world
```

## Limits

`limits` in the manifest bounds every invocation of a version. An absent limit takes the platform
default, clamped to the owning client's ceiling (the client's function policy).

| Key | Default | What happens past it |
|---|---|---|
| `maxDurationMs` | 30 000 | An endpoint's `timeoutMs` defaults to it. The guest is stopped wherever it is: `504 FUNCTION_TIMEOUT`. |
| `maxConcurrency` | 32 | Calls in flight per host; one more is `429 BUSY` with `Retry-After: 1`. |
| `wasmMemoryMb` | 64 | Growing linear memory past it fails the allocation (a trap in Rust): `500 FUNCTION_ERROR`. |
| `maxFuel` | none | The guest is stopped wherever it is: `500 FUNCTION_FUEL_EXHAUSTED`. Rust hosts only (see below). |

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

- `maxFuel` applies to `runtime: wasm` and `component` only; on a `jvm` function it is
  `400 LIMIT_NOT_APPLICABLE`. It has no platform default and no client ceiling.
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

A function reaches the PostgreSQL databases its manifest declares under `db[]`, through the host's shared
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

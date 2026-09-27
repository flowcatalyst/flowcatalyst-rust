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

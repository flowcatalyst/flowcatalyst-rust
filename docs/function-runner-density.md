# Function runner density spike (F0)

Status: F0 done, 2026-09-24. This gates H4 (`docs/function-runner-plan.md` §5).
Code: `spikes/fnhost-density/`, a throwaway workspace of its own that is **not** in the root workspace.
Raw output: `spikes/fnhost-density/results/*.txt`. Every result line there sits under the exact command that
produced it.

**The answer, in brief.** For H4, use **plain wasmtime (49) with WASI 0.2 components and `wasi:http/proxy`**,
plus a small typed `flowcatalyst:function` WIT package. Use one `Engine`, the pooling allocator, epoch
deadlines, `StoreLimits`, and precompiled `.cwasm` loaded by `mmap`. Don't use the `extism` crate: per function it
costs 4–4.7× the memory of plain wasmtime, and it can't route WASI output. Its kernel memory cap also traps
rather than degrading, and it pins an older wasmtime (43). If the Extism ABI is ever needed again, run it on plain
wasmtime with the kernel implemented natively in the host (variant b′ below): it is the fastest wasm variant
measured, and it reproduces every Java behaviour. The owner ruled on 2026-09-24 that functions don't need Java
compatibility. So this recommendation is made on merit, not parity.

---

## 1. Method

**Machine.** Apple M4 Pro (10 performance and 4 efficiency cores, 14 logical), 48 GiB RAM, 16 KiB pages. macOS
15.6.1 (arm64), on AC power. There was no core pinning: macOS has no `taskset`, so threads float across P- and
E-cores. Toolchain: rustc 1.98.1 in release profile. The runs were native processes; no Docker was involved.
Other desktop load was light but not controlled.

**Engines (variants).**

| id | what | versions |
|---|---|---|
| **a** | the `extism` crate, used through its public API only. One `CompiledPlugin` per function; each builds its own wasmtime `Engine` and compiles the kernel again, because that is how the crate works. One `Plugin` per concurrent call. | `extism =1.30.0` (latest stable, 2026-06-04), which pulls in wasmtime 43.0.2 |
| **b** | plain wasmtime running the Extism kernel from Java's `extism-endive` (`extism-runtime.wasm`, 3,508 B). Every `extism:host/env` import is a host function that forwards to the store's kernel instance. | wasmtime 49.0.1 and wasmtime-wasi 49.0.1 (p1) |
| **b′** | the same as b, but the kernel's contract (alloc, length, load/store, input/output/error, reset) is implemented **in the host**. There is no kernel instance: a bump allocator over a host `Vec<u8>`, capped at `wasmMemoryMb`. Added because b's per-8-byte host→wasm forwarding made b look slower than it needs to be. | wasmtime 49.0.1 |
| **c** | WASI 0.2 **components** plus `wasi:http/proxy` on wasmtime (async on tokio), with a `bindgen!` host for `flowcatalyst:function/host`. | wasmtime, wasmtime-wasi and wasmtime-wasi-http 49.0.1 |
| **d** | JS guests: QuickJS through **extism-js** 1.6.1, run on b′ and a; and StarlingMonkey through **componentize-js** 0.23 / jco 1.35, run on c. | |
| **e** | a baseline of V8 isolates through **deno_core** 0.412 (V8 150.4), one isolate per function, with a startup snapshot that contains the handler. | |

All wasm variants (b, b′, c) use:

- one `Engine` and one epoch ticker thread (1 ms);
- `Module`/`Component` plus `InstancePre`/`ProxyPre` per function;
- the pooling allocator (`max_memory_size` 64 MiB, which is `wasmMemoryMb`'s default);
- `StoreLimits` capping **each** memory at `wasmMemoryMb`;
- epoch interruption for deadlines: a trap on b/b′, and a 1 ms async yield plus `tokio::time::timeout` on c.

**Guests.**

- a, b, b′: Java's `fc_test_guest.wasm`, copied byte-identical, with its sha256 `313b11e7…f792` verified at every
  start against the copied `SHA256SUMS`.
- c: `guests/comp-echo`, a Rust `wasm32-wasip2` component of 150 KB (the fixture is 159 KB). It has the same
  behaviours as the fixture, selected by path: `/echo`, `/spin`, `/alloc?mb=`, `/http?url=`, `/config?key=`,
  `/log`.
- JS guests: `guests/js-extism` (2.48 MB) and `guests/js-component` (14.4 MB).
- Rebuild the guests with `scripts/build-guests.sh`.

**The echo payload.** For a, b and b′ it is Java's ABI request JSON (≈0.9 KB), and the fixture echoes it back.
For c it is an HTTP POST with a 24 B body plus two headers, and the guest returns JSON of the method, path,
headers and body. The work per call is similar but not identical.

**What each measurement means.**

- **Memory.** macOS `proc_pid_rusage`, sampled after a 300 ms settle.
  - `fp` is `ri_phys_footprint`: dirty plus compressed anonymous memory, the closest analogue of a container's
    anonymous RSS. It is the primary metric.
  - `rss` is `ri_resident_size`, which also counts clean, file-backed and mapped pages. Examples are the `.cwasm`
    code of precompiled functions and the V8 binary.
  - Per-function cost is the **marginal slope** between the largest N points (500→2000, or 100→1000 when
    precompiled), not total ÷ N. That keeps the fixed baseline and one-time warm-up out of it.
  - Idle means loaded (compiled, with `InstancePre` ready) and not yet instantiated. Warm means one live
    instance per function after one call.
- **Load / compile.** Wall time per function for `Module::new`/`Component::new` plus `instantiate_pre`, or
  `Engine::precompile_*` once and then `deserialize_file` per function. Each function gets its own `.cwasm` file
  copy, because distinct functions are distinct artifacts. Cranelift's parallel compilation was on.
- **First call.** Instantiate plus one `echo` call on a loaded function, as p50/p99 over the N functions,
  measured in process with no HTTP.
- **Steady.** One function, C closed-loop workers (C = 1 or 64), 1 s warm-up, then 10 s measured.
  - a, b and b′ use OS threads.
  - c uses tokio tasks on the multi-thread runtime (14 workers).
  - Pooled means one instance per concurrent call, reused, as Java does. Fresh means instance-per-request.
- **Noisy neighbour.** Function A calls `echo` at a fixed 500 req/s, open loop, with latency measured from the
  actual dispatch instant. A runs 10 s alone, then 10 s while function B runs at concurrency `--b` (28, 8 or 4):
  - `spin`: never returns; stopped at a 100 ms deadline, the instance discarded, and repeated.
  - or `alloc`: allocates and touches 16 MiB in guest memory.
- **Bounds.** No run exceeded about 8 GB footprint. Extism at N=2000 peaked at 6.3 GB warm; its RSS slope had
  been checked at N=100 first.

**The exact commands** are the scripts in `spikes/fnhost-density/scripts/`. Every run in `results/` is preceded by
its `$ …` command line. In outline:

```
cargo build --release                                  # in spikes/fnhost-density (host + v8)
scripts/density.sh <engine> 1 10 100 500 1000 2000     # engine = extism | kernel | native | component
scripts/density-pre.sh <engine> 100 1000
scripts/steady.sh <engine> [--fresh]
scripts/neighbour.sh <engine> [28 8 | 4]               # FC_TICK_US=100 for the 100 µs epoch-tick rows
scripts/egress.sh ; scripts/js.sh ; scripts/v8.sh
```

---

## 2. Raw numbers

### 2.1 Density: the Extism fixture (a, b, b′) and the Rust component (c)

Footprint (`fp`) in MB, with the process total at each N. "Engine" is the process after creating the engine,
before any function is loaded.

| N | a extism idle / warm | b kernel idle / warm | b′ native idle / warm | c component idle / warm |
|---:|---:|---:|---:|---:|
| engine | 2.8 | 10.1 | 3.9 | 4.2 |
| 1 | 21.5 / 21.8 | 29.2 / 29.4 | 28.4 / 28.5 | 24.5 / 24.8 |
| 10 | 46.7 / 49.2 | 34.1 / 35.5 | 36.1 / 37.2 | 39.7 / 41.0 |
| 100 | 234.8 / 272.2 | 93.3 / 107.6 | 88.7 / 99.8 | 102.8 / 115.5 |
| 500 | 1,097 / 1,294 | 335 / 410 | 334 / 392 | 395 / 465 |
| 1000 | 2,278 / 2,686 | 640 / 787 | 647 / 760 | 750 / 888 |
| 2000 | 5,484 / 6,345 | 1,271 / 1,580 | 1,270 / 1,511 | 1,494 / 1,792 |

**Marginal cost per function** (the 500→2000 slope, compiled in process):

| | a extism | b kernel | b′ native | c component |
|---|---:|---:|---:|---:|
| idle fp per fn | **2,995 KB** | 642 KB | **639 KB** | **750 KB** |
| warm fp per fn (plus one instance) | 3,448 KB | 799 KB | 763 KB | 906 KB |
| idle functions per GiB (fp) | 350 | 1,634 | 1,641 | 1,398 |
| warm functions per GiB (fp) | 304 | 1,312 | 1,373 | 1,158 |

**Loaded from a precompiled `.cwasm`** (`deserialize_file`, which is an mmap). The slope is 100→1000, and the
`.cwasm` is 553 KB for the modules and 563 KB for the component.

| | b kernel | b′ native | c component | a extism (wasmtime on-disk cache instead) |
|---|---:|---:|---:|---:|
| idle fp per fn (anonymous) | 59 KB | 53 KB | 146 KB | 1,037 KB |
| idle RSS per fn (includes mapped code) | 472 KB | 465 KB | 573 KB | 1,429 KB |
| warm fp per fn | 208 KB | 168 KB | 290 KB | 1,476 KB |
| idle fns per GiB, fp / RSS | 17,700 / 2,170 | 19,800 / 2,150 | 7,190 / 1,790 | 1,010 / 720 |

**Load time per function** (mean, with p99 in brackets):

| | a | b | b′ | c |
|---|---:|---:|---:|---:|
| compile from `.wasm` (N=100…2000) | 16–23 ms (30–76) | 17–28 ms (37–91) | 16–24 ms (30–68)¹ | 15–44 ms (34–145) |
| precompile once | — | 30–45 ms | 16–25 ms | 17–28 ms |
| load from `.cwasm` | 1.3–1.8 ms (1.6–3.9)² | 0.22–0.27 ms (0.8–1.1) | 0.11 ms (0.35) | 0.17–0.19 ms (0.59–0.66) |

¹ The b′ N=1000 compiled run was disturbed: its load p50 was 47 ms and RSS fell below footprint, a sign that
the macOS memory compressor was active. Its first-call p99 of 5.9 ms is treated as an outlier. Every other N
agrees.
² extism has no public `deserialize`. Its closest equivalent is wasmtime's on-disk compilation cache, which it
enables by default. The engine-per-plugin and kernel setup are still paid per function.

**First call (instantiate plus `echo`), and the second call on the same warm instance:**

| N | a first p50 / p99 | b first p50 / p99 | b′ first p50 / p99 | c first p50 / p99 |
|---:|---:|---:|---:|---:|
| 100 | 0.24 / 0.31 ms | 0.14 / 0.23 ms | 0.11 / 0.20 ms | 0.18 / 0.47 ms |
| 1000 | 0.25 / 0.35 ms | 0.16 / 0.65 ms | 0.12 / 5.9 ms¹ | 0.27 / 2.5 ms |
| 2000 | 0.33 / 3.7 ms | 0.14 / 0.70 ms | 0.10 / 0.16 ms | 0.18 / 0.84 ms |
| 1000, from `.cwasm` | 0.24 / 1.0 ms | 0.14 / 0.19 ms | 0.09 / 0.19 ms | 0.26 / 4.1 ms |
| second call (N=2000) | 0.04 / 0.06 ms | 0.06 / 0.14 ms | 0.02 / 0.05 ms | 0.05 / 0.12 ms |

A lazy cold call from an artifact cached on disk is **deserialize + instantiate + call**. At N=1000 that is
about 0.2 ms p50 for b′ and about 0.45 ms p50 (≈4.7 ms p99) for c.

### 2.2 Steady `echo` (µs)

| | c=1 p50 / p99 | c=1 calls/s | c=64 p50 / p99 | c=64 calls/s |
|---|---:|---:|---:|---:|
| a extism | 13.3 / 22.0 | 73k | 51 / 10,087 | 152k |
| b kernel (forwarded) | 45.8 / 108 | 19k | 6,984 / 271,508 | 1.7k |
| **b′ native kernel** | **5.9 / 7.4** | 168k | 8.4 / 1,500 | 753k |
| **c component, pooled** | **10.0 / 36.0** | 85k | 40.7 / 1,831 | 339k |
| b′ instance-per-request | 22.0 / 97.6 | 23k | 2,560 / 16,887 | 22k |
| c instance-per-request | 34.9 / 340 | 17k | 533 / 13,097 | 49k |

- c=64 on 14 cores is 4.6× oversubscribed, so its p99 is mostly OS or tokio scheduling. Use it for throughput,
  not latency.
- b collapses under concurrency: each 8-byte guest load or store is a host→wasm re-entry into the kernel
  instance. This is why b′ exists; b is not a design to pursue.
- On macOS, instance-per-request at c=64 is limited by the pooling allocator's decommit (`madvise`, batch 1)
  and its slot lock. Linux (memfd copy-on-write, batched decommit) is expected to do better. Re-measure there
  before choosing instance-per-request as the default.

### 2.3 Noisy neighbour (A = `echo` at 500 req/s; µs; degradation = busy ÷ alone − 1)

| engine | B | B mode | A alone p50 / p99 | A busy p50 / p99 | Δp50 | Δp99 |
|---|---:|---|---:|---:|---:|---:|
| a extism | 28 | spin | 62 / 316 | 135 / 27,113 | +118% | +8,488% |
| a extism | 28 | alloc | 81 / 1,024 | 100 / 5,009 | +23% | +389% |
| a extism | 8 | spin | 74 / 376 | 91 / 11,015 | +22% | +2,826% |
| a extism | 8 | alloc | 76 / 465 | 122 / 3,385 | +59% | +628% |
| b kernel | 28 | spin | 130 / 574 | 112 / 1,784 | −14% | +211% |
| b kernel | 28 | alloc | 153 / 3,195 | 7,258 / 56,635 | +4,636% | +1,673% |
| b kernel | 8 | spin | 223 / 780 | 136 / 5,677 | −39% | +628% |
| b kernel | 8 | alloc | 149 / 1,073 | 6,622 / 20,704 | +4,345% | +1,830% |
| b′ native | 28 | spin | 19 / 283 | 36 / 3,775 | +91% | +1,235% |
| b′ native | 28 | alloc | 78 / 435 | 52 / 753 | −34% | +73% |
| b′ native | 8 | spin | 70 / 589 | 24 / 3,495 | −66% | +494% |
| b′ native | 8 | alloc | 46 / 2,342 | 116 / 8,542 | +152% | +265% |
| b′ native | 4 | spin | 74 / 4,465 | 31 / 701 | −59% | −84% |
| b′ native | 4 | alloc | 39 / 4,037 | 51 / 542 | +31% | −87% |
| c component | 28 | spin | 84 / 462 | 3,488 / 45,116 | +4,061% | +9,667% |
| c component | 28 | alloc | 42 / 216 | 1,685 / 17,816 | +3,943% | +8,159% |
| c component (100 µs tick) | 28 | spin | 49 / 417 | 786 / 50,111 | +1,492% | +11,918% |
| c component (100 µs tick) | 28 | alloc | 186 / 10,250 | 927 / 52,308 | +399% | +410% |
| c component | 8 | spin | 86 / 437 | 103 / 7,862 | +19% | +1,698% |
| c component | 8 | alloc | 46 / 268 | 191 / 7,253 | +313% | +2,605% |
| c component | 4 | spin | 40 / 116 | 70 / 320 | +75% | +175% |
| c component | 4 | alloc | 42 / 190 | 57 / 878 | +35% | +362% |
| c component (100 µs tick) | 4 | spin | 37 / 732 | 40 / 896 | +8% | +22% |
| c component (100 µs tick) | 4 | alloc | 43 / 2,270 | 34 / 381 | −22% | −83% |

**How to read this.**

- **The noise floor is large.** A's p99 *alone* ranged from 0.1 to 4.5 ms between runs of the same code,
  because nothing is pinned on this desktop: P/E-core migration, wake-up latency, and background load. So only
  large effects are real.
- **B at or above the core count.** With B at 28 (2× the cores) or 8 (spin: 8 of 14 cores burning), every
  engine degrades A's p99 into the milliseconds: 3.5–8.5 ms for b′, 7–50 ms for c, and 3–27 ms for a.
- c degrades the most when saturated. Guest execution shares the tokio workers with A, and a spinning guest
  gives a worker back only at an epoch yield. A 100 µs tick didn't help at B=28.
- **B at 4 (≈30% of the cores).** Every A number stays sub-millisecond and inside the noise floor. There is no
  measurable degradation for b′ or c.
- **The trade in (a).** The sync engines use one OS thread per call and get OS preemption, but pay for it in
  thread count.

### 2.4 JS guests (d) and the V8 baseline (e)

| | artifact | idle fp per fn | idle RSS per fn | load | first call p50 / p99 | steady c=1 p50 / p99 | c=64 calls/s |
|---|---:|---:|---:|---:|---:|---:|---:|
| QuickJS (extism-js), on b′, compiled | 2.48 MB | 5.7 MB | 10.4 MB | 117 ms | 0.42 / 1.4 ms | 7.8 / 9.8 µs | 739k |
| QuickJS (extism-js), on b′, `.cwasm` (4.9 MB) | | 0.63 MB | 4.3 MB | 0.41 ms | 0.42 / 1.7 ms | | |
| QuickJS (extism-js), on a extism | | 9.9 MB | 21.1 MB | 118 ms | 0.92 / 1.2 ms | 32.9 / 40.2 µs | |
| StarlingMonkey component, on c, compiled | 14.4 MB | 38.8 MB | 147 MB | 735 ms | 2.6 / 15.5 ms | | |
| StarlingMonkey component, on c, `.cwasm` (34.7 MB) | | 2.1 MB | 25.9 MB | 2.3 ms | 1.4 / 4.1 ms | 589 / 823 µs (per request)³ | 2.8k |
| V8 isolate (deno_core), with snapshot | — | 1.65 MB | 1.54 MB | 0.6–0.7 ms (create) | 0.014 / 0.04 ms | 2.3 / 2.9 µs | 3.47M⁴ |
| V8 isolate, without a snapshot | — | 3.5 MB | 3.3 MB | 2.5 ms | 0.023 / 0.04 ms | | |

- V8 at N=1000 with a snapshot: 1,620 MB footprint in total, 1.65 MB per isolate, which is ≈620 isolates per
  GiB. Cold (create + first call) is 0.72 ms p50 and 1.2 ms p99.
- ³ StarlingMonkey runs only as instance-per-request here. With a reused instance, its response body didn't
  finish until the store was dropped, and the call hung. The `wasmtime serve` model drops the store after every
  request, which is why it works there. Cause not investigated; this is a known limit of the spike.
- The QuickJS component backend (`jco componentize --backend quickjs`) wouldn't build. Its own link check
  rejected `wasi:http@0.2.10` (`io-error` resource mismatch).
- ⁴ V8 at c=64 means 64 threads, each with its own isolate, calling in-thread.
- The handlers differ, so compare orders of magnitude only. QuickJS copies input to output; StarlingMonkey parses
  the request and builds JSON; V8 parses and stringifies JSON.

---

## 3. Against Java's published numbers

Java sources: `docs/function-runner-report.md` (B1–B4) and `docs/plan/wasm-and-js-functions.md` (W0), at
`0118cdca`.

| Java figure | Java | Rust spike | comparable? |
|---|---|---|---|
| B1 per loaded function, *typical* JVM jar | 5.4–5.9 MB RSS, 4.38 MB metaspace; baseline ~110–135 MB | wasm, compiled in process: 0.64–0.75 MB fp (b′, c); from `.cwasm`: 0.05–0.15 MB fp, 0.47–0.57 MB RSS; engine baseline 4–10 MB | **No.** B1 measured JVM jars, not wasm, on Docker Desktop at `--memory 2g/4g`, as container RSS. It is only a sizing reference: a Rust host holds ≈8× more functions per GiB than Java does *typical* jars. |
| W0: metaspace per compiled Rust wasm module on Endive | ~3.8 MB | 0.64 MB fp (b′), 0.75 MB (c) | **Roughly.** Similar guest (a Rust extism-pdk module of 138 KB vs 159 KB), different engine and metric (metaspace vs footprint). About 5–6× less in Rust. |
| B2 cold first call | lean jar 3.18 / 9.78 ms (p50/p99), typical 58 / 97 ms, end to end via curl and HTTP | 0.10–0.27 ms p50, ≤0.84 ms p99 at N=2000, in process, no HTTP | **No.** B2 includes HTTP, curl and Docker networking. Directionally, a wasm cold call is well under 1 ms before HTTP. |
| W0 load (compile) | ~0.2 s Rust, ~0.9 s JS (Endive, wasm → JVM bytecode) | 15–44 ms Rust (Cranelift), 117 ms QuickJS, 735 ms StarlingMonkey; 0.1–2.3 ms from `.cwasm` | **Roughly.** Same kind of work, different compilers. |
| W0 first call | ~0.3 ms Rust, ~3 ms JS | 0.1–0.3 ms Rust, 0.42 ms QuickJS, 1.4–2.6 ms StarlingMonkey | **Roughly** (both in process). |
| W0 steady call | ~20 µs Rust, ~340 µs JS (compiled) | 5.9 µs (b′), 10 µs (c), 13 µs (a); QuickJS 7.8 µs, StarlingMonkey 589 µs per request, V8 2.3 µs | **Roughly.** W0's JS handler is not the same code, so JS rows are an order of magnitude at best. |
| B4 noisy neighbour | A p50 +34%, p99 +167% (8.0 → 10.8 ms, 11.9 → 29.5 ms). `--cpus 2`, B at `maxConcurrency` 8 saturating both CPUs, JVM `/io` endpoint | saturated (B = 8 or 28 on 14 cores): A p99 goes from sub-ms to 3.5–50 ms, i.e. +200% to +12,000%; B = 4: within noise | **No.** Different quota (cgroup 2 CPUs vs 14 unpinned cores), harness and workload, and B4's A is an HTTP endpoint with 5 ms of I/O. Both show the same thing: a saturating neighbour costs the p99 of a shared host. |

---

## 4. Decision 5: HTTP egress parity, and the other Extism-crate gaps

Each row was proven by running Java's guest exports (`http`, `kalloc`, `alloc`, `log`, `config`, `secret`)
through `scripts/egress.sh`; the output is in `results/egress.txt`. The loopback test server answers:

- `/ok`: 200 with `x-upstream: a` and `x-upstream: b`;
- `/redirect`: 302;
- `/slow`: after 2 s.

| Java behaviour | a: extism crate as-is | a: with the spike's workarounds | b / b′: wasmtime + kernel | c: components |
|---|---|---|---|---|
| Replace or intercept `http_request` | Built-in: a denied host is a **trap**; redirects are followed (ureq's default); no https rule; timeout is the manifest remainder only | **Yes.** A host `Function` in namespace `extism:host/env` named `http_request` / `http_status_code` / `http_headers` **shadows** the built-in: the crate links user functions after its own, with `allow_shadowing(true)`. Per-plugin state is keyed by `CurrentPlugin::id()`. | Yes (we define every import) | `WasiHttpHooks::send_request` |
| https only, except loopback | — | ✅ status 0, `{"error":"scheme 'http' is not permitted…"}` | ✅ same | ✅ `error-code.HTTP-request-denied` (typed) |
| Allowlist denial as status 0 with a body, no trap | — (built-in traps) | ✅ | ✅ | ✅ typed error, not status 0 (by design) |
| No redirects | — | ✅ 302 seen by the guest | ✅ | ✅ hyper never follows |
| Timeout = min(call, remaining deadline, 30 s) | partly | ✅ from `time_remaining()`; ended at 1,000 ms, not 2 s | ✅ 1,002 ms | ✅ typed `ConnectionReadTimeout` at 1,008 ms |
| Response headers flattened with `", "` | ❌ `BTreeMap` insert, so the last value wins | ✅ `"a, b"` | ✅ `"a, b"` | n/a: `fields.get` returns the list, typed |
| Kernel memory capped (Java `theKernelsOwnMemoryIsCappedByWasmMemoryMb`: 200 with `grantedMb` 1…16) | ❌ `max_pages` is one **shared** budget for guest plus kernel growth, and over-budget growth **traps** (`oom`), so the call fails | ✅ *only* through a wasmtime config side effect: `with_wasmtime_config(memory_reservation = cap, memory_may_move = false)` gives `grantedMb` 15. That config applies to every memory in the plugin's engine and forces explicit bounds checks. | ✅ `StoreLimits` per memory: `grantedMb` 15 | n/a (no kernel); the guest's memory is capped and an over-cap alloc is a clean trap |
| Guest over its memory cap: a clean failure, then the next call works | ✅ (trap, then a fresh instance) | ✅ | ✅ | ✅ |
| WASI stdout/stderr to the function's logger (INFO/WARN, 8 KiB split) | ❌ discarded, or the **process** stdout if `EXTISM_ENABLE_WASI_OUTPUT` is set. There is no per-plugin hook, and host functions can't read guest memory, so `fd_write` can't be shadowed usefully. | ❌ **not reproducible** without a fork | ✅ custom `StdoutStream` | ✅ same |
| `log_*` to the function's logger | via `tracing` only; `get_log_level` answers OFF unless a subscriber is installed | ✅ shadow `log_*` and `get_log_level` | ✅ | ✅ typed `host.log(level, msg)` |
| `config_get` for declared keys only; `fc_secret_get` returns offset 0 when missing | ✅ manifest config / host fn | ✅ | ✅ | ✅ typed `option<string>` |

**Verdict.**

- The extism crate *can* be made to do Java's HTTP egress, by shadowing its imports. That relies on the
  link-order and shadowing details of its private `relink()`, and no public contract.
- It **cannot** route WASI output per function.
- It can cap the kernel gracefully only through a wasmtime config side effect.
- Plain wasmtime (b/b′) reproduces every row. So does the component path, with typed errors in place of
  status 0.

---

## 5. Guest contract options

| | Extism ABI (b′: wasmtime + native kernel) | WASI 0.2 components + `wasi:http` (c) | V8 isolates (e) |
|---|---|---|---|
| **Density, Rust guest** | 639 KB/fn compiled; from `.cwasm` 53 KB fp / 465 KB RSS, i.e. 1.6k fns/GiB compiled, 2.1k–19.8k from `.cwasm` | 750 KB/fn compiled; from `.cwasm` 146 KB fp / 573 KB RSS, i.e. 1.4k compiled, 1.8k–7.2k from `.cwasm` | — |
| **Density, JS guest** | QuickJS: 0.63 MB fp / 4.3 MB RSS from `.cwasm` | StarlingMonkey: 2.1 MB fp / 26 MB RSS (34.7 MB `.cwasm` per function; no sharing) | 1.65 MB per isolate (≈620/GiB); the engine binary is shared |
| **Latency, Rust** | first call 0.1 ms; steady 5.9 µs | first call 0.2–0.3 ms; steady 10 µs pooled, 35 µs per request | — |
| **Latency, JS** | 7.8 µs (QuickJS, trivial handler) | 589 µs (StarlingMonkey, per request) | 2.3 µs (JIT); create 0.6 ms |
| **Portability** | Extism host SDKs in many languages; not Spin or wasmCloud | the WASI standard (W3C WebAssembly CG, with the Bytecode Alliance implementing it): a pure `wasi:http/proxy` component runs on `wasmtime serve`, Spin and wasmCloud unchanged. One that imports `flowcatalyst:function/host` needs that interface provided or virtualized there. Fastly Compute has its own ABI; check its `wasi:http` support before relying on it. | Deno Deploy / Supabase style, JS/TS only |
| **Typing of host interfaces** | bytes and JSON; host functions pass untyped `i64` offsets; errors by convention (status 0) | WIT: typed records, variants, `option`, `result`, resources, versioned packages; `bindgen!` on both sides | JS objects; host ops are hand-written |
| **JS/TS authors** | extism-js: an Extism-specific `Host.inputString` API; a 2.5 MB QuickJS interpreter | componentize-js: web-standard `fetch` event, `Request`, `Response`, `URL`, TS through bundling; a heavy 14 MB artifact, slow per-request instantiation here; toolchain rough (WIT-version pinning needed, a reused instance hung) | best: modern JS/TS, npm through bundling, JIT |
| **Rust authors** | `extism-pdk` macros; a simple model | `wasm32-wasip2` is a first-class rustc target; the `wasi` crate, `wstd` or `wit-bindgen`; typed host calls; std I/O works | n/a |
| **Java host compatibility** | runs on Java's Endive host today | ❌ Endive can't run components (no longer required) | ❌ |
| **Operations** | we own a small kernel shim (about 150 lines here) plus the ABI docs | wasmtime, wasmtime-wasi and wasmtime-wasi-http only (monthly releases; LTS available); `wasmtime serve` is a reference to diff against | a second engine: V8 build size (60 MB binary), a different sandbox and security model |

---

## 6. Recommendation for H4

**Engine: plain wasmtime (current stable, pinned), not the extism crate, as-is or forked.**

- The crate builds an `Engine` per plugin and compiles the kernel per plugin. That costs 3.0 MB per function vs
  0.64 MB on one engine: 4.7× the memory, and 350 vs 1,640 functions per GiB.
- It pins wasmtime 43 and the legacy `wasi-common`.
- It can't route WASI output, and it traps when kernel growth exceeds `max_pages`.
- Forking it to fix those means owning most of it; b′ is about 150 lines.

**Guest contract: WASI 0.2 components with `wasi:http/proxy`, plus a `flowcatalyst:function` WIT package**
(the sketch is in `spikes/fnhost-density/wit/flowcatalyst-function.wit`, with `config-get` wired end to end).

- Its density is within 17% of the fastest Extism variant when compiled, and all of it is sub-MB when loaded from
  `.cwasm`.
- Its latency is at most 10 µs per pooled call.
- Its interfaces are typed.
- The same artifact runs on `wasmtime serve`, Spin and wasmCloud, provided our own host interface is kept optional.
- Rust gets a first-class target.
- There is nothing of our own to maintain (no kernel).
- An egress denial is a typed `wasi:http` error instead of the status-0 convention.

**H4 configuration to carry forward:**

1. One `Engine` with the pooling allocator and epoch interruption.
2. Compile once per version at publish time (or on the host's first sight) into `.cwasm`, cache it, and load it
   with `deserialize_file`. That takes about 0.2 ms, against 15–45 ms to compile.
3. `ProxyPre` per version, and `StoreLimits` per store.
4. Pooled instances per version, up to `maxConcurrency`. Discard an instance on a trap or a deadline.
5. Keep instance-per-request as an option. Re-measure it on Linux first, because on macOS it costs 3.5× per call.
6. Guest execution must not share unbounded tokio workers with the listener. Either:
   - run guests on their own runtime or thread pool; or
   - cap the host-wide "executing guests" permit below the core count.

   At B=4 of 14 cores, A's p99 stayed inside the noise floor. At B≥8 it didn't, for any engine.

**JS.**

- StarlingMonkey works but is heavy (26 MB RSS per function, 0.6 ms per request).
- The QuickJS component backend didn't build.
- V8 isolates beat both on latency (2.3 µs) and on RSS density.

Treat the JS runtime as its own decision in the G track. Options:

- componentize-js, after investigating reuse and the body-completion hang;
- QuickJS components (componentize-qjs or Javy);
- or V8 isolates as a second engine, if JS volume justifies it.

This doesn't block H4, which is Rust components first.

---

## 7. Acceptance bar

The bar comes from the language assessment.

| Bar | Result | Met? |
|---|---|---|
| ≥ 2,000 idle functions per GiB | **Compiled in process:** b′ 1,641, c 1,398, a 350. **From `.cwasm` (the H4 design):** anonymous footprint b′ 19,800, c 7,190; counting mapped code as resident, b′ 2,150, c 1,790. | **Met for the recommended design** by anonymous footprint (the mapped `.cwasm` pages are clean and reclaimable). **Not met** if every mapped code page must stay resident (c 1,790), or when compiling in process. Note the 16 KiB pages here; Linux's 4 KiB pages should lower per-function RSS. Re-check on Linux. |
| First-call p99 < 5 ms | c: 0.47 / 2.5 / 0.84 ms at N = 100 / 1000 / 2000; 4.1 ms from `.cwasm` at N=1000 (deserialize + first call ≈ 4.7 ms p99). b′: ≤ 0.2 ms (one disturbed run: 5.9 ms). a: ≤ 3.7 ms. | **Met** (c from `.cwasm` with a thin margin). StarlingMonkey JS: 4.1 ms first call, but 15.5 ms compiled; V8: 1.2 ms cold. |
| Neighbour p99 degradation < +30% | With B saturating the cores (8 or 28 of 14): not met by any engine (+200% to +12,000%). With B capped at 4 of 14: within noise for b′ and c (c at a 100 µs tick: +22%). | **Not met under saturation. Met only with a concurrency cap** (a host-wide cap on executing guests below the core count, plus a per-function `maxConcurrency`), which H4 must implement. Java's B4 also missed this bar (+167%). |

---

## 8. Caveats

- This is one run per point on an unpinned macOS desktop, with 16 KiB pages and no cgroups. Density slopes were
  stable across N. Latency p99 and neighbour numbers are noisy (see §2.3). Redo H7 on Linux x86_64 and arm64
  with pinned cores before quoting absolute numbers.
- Every function is the same fixture compiled separately. Real functions differ in size, but not in how they
  are loaded.
- The measurements are in process: there is no HTTP listener. H5/H7 will add hyper's cost.
- Variant c's `/echo` is not byte-identical work to the Extism echo (see §1).
- In the spike's component call path, the handler runs in its own task and the caller awaits the outparam, as
  `wasmtime serve` does. Joining the handler and the response in one task worked for the Rust guest but hung
  StarlingMonkey.

---

## 9. Fuel metering overhead (owner decision #13, 2026-09-27)

Every invocation on the host is metered in wasmtime fuel (`Config::consume_fuel`), and its peak linear memory
recorded through the store's `ResourceLimiter`; `limits.maxFuel` stops a guest past its budget
(`500 FUNCTION_FUEL_EXHAUSTED`). Epoch interruption still enforces the wall-clock deadline and gives the
1 ms time slices; fuel is only counted. A running guest also refuels every 10 M fuel
(`fuel_async_yield_interval`), which is where compiled code writes its register-held count back, so a
guest stopped at its deadline mid-loop still reports what it spent (to within 10 M).

**Method.** `crates/fc-fnhost-core/tests/wasm_fuel.rs`, `measure_fuel_metering_overhead` (ignored; run in
release, `cargo test --release -p fc-fnhost-core --test wasm_fuel -- --ignored --nocapture --test-threads 1`).
Two complete hosts (artifact cache, reconciler, `WasmLoader`, listener) run side by side, one engine with
`consume_fuel` off and one with it on, and the calls **alternate** between them through the real
listener (loopback HTTP included), so background load hits both alike. Same machine as §1, but **not**
idle: other build jobs held the load average at 200+ throughout, so compare the two columns of a row,
not rows with §2. Three runs; the table is the third, the ranges span all three.

| Workload (guest) | calls | off p50 / p99 | on p50 / p99 | p50 overhead (3 runs) |
|---|---:|---:|---:|---:|
| `echo /x` — a trivial handler: request fields to JSON | 4,000 | 0.113 / 0.195 ms | 0.114 / 0.205 ms | +0.3 … +1.8% |
| `spin ?n=20000000` — a tight counting loop (the worst case: one fuel check per iteration) | 150 | 9.08 / 9.36 ms | 10.90 / 12.03 ms | **+19.3 … +20.0%** |
| `spin ?hash=200` — FNV over a 64 KiB buffer, 200× (loads, stores, arithmetic) | 150 | 14.19 / 14.51 ms | 14.19 / 14.75 ms | −0.1 … +0.2% |
| `alloc ?mb=32` — allocate and touch 32 MiB | 600 | 2.49 / 3.14 ms | 2.49 / 3.29 ms | +0.1 … +0.3% |

| Static cost | off | on |
|---|---:|---:|
| `.cwasm` of the `spin` guest | 397,080 B | 446,336 B (+12.4%) |
| `.cwasm` of the `echo` guest | 433,416 B | 482,680 B (+11.4%) |
| first call (lazy: compile + instantiate + call) | 22–72 ms | 25–58 ms (within noise) |

**Reading it.**

- Metering is **not material** for real handlers: within noise (≤ 2%) for request/JSON work, memory
  fills and mixed arithmetic. Only a loop that does almost nothing per iteration pays visibly (~20%),
  because the per-iteration fuel check is then a large share of the iteration. So metering stays **on
  unconditionally**, as the plain instruction count; no coarser accounting was needed.
- Density: fuel adds ~12% to compiled code, i.e. about +50 KB of mapped `.cwasm` per function. The
  anonymous footprint (§2.1's 146 KB per function from `.cwasm`) is unchanged; the RSS-with-mapped-code
  figure moves from ≈573 KB to ≈620 KB per function (≈1,690 instead of ≈1,790 functions per GiB when
  every code page stays resident).
- `EngineSettings::consume_fuel` exists only for this measurement. Metered and unmetered code differ, so
  it is part of the `.cwasm` fingerprint: switching it never loads a stale file.

---

## 10. The JS runtime as built (H9, `runtime: js`, 2026-09-27)

Owner decisions 6 and 27 chose V8 isolates (`deno_core`) for JS/TS functions. `crates/fc-fnhost-js` is the result
(`docs/function-runner-plan.md` H9). It is **not** the spike's shape (e), one long-lived isolate per function: it is
the `wasi:http` model, **a fresh isolate per request**. The bundle is compiled from a V8 code cache made at load, and
its top-level code runs in every isolate.

**Versions.** `deno_core =0.412.0`, V8 150.4 (the prebuilt static library `librusty_v8.a`: 147 MB on macOS arm64,
185 MB on Linux arm64, 187 MB on Linux x86_64; a 39 MB download per target, cached under `target/`).

### 10.1 Two findings that shaped it

1. **No snapshot per version.** V8's snapshot creator writes the read-only heap space V8 shares between all isolates
   of a process. Creating one while other isolates ran crashed the host (a request isolate allocating into
   `ReadOnlySpace`, `StringForwardingTable` checks failing, a protection fault in
   `ReadOnlySpace::RepairFreeSpacesBeforeSerialization` even for a creator started from an existing snapshot). A host
   that loads a function while serving others always has isolates running, so it makes at most one snapshot, the
   **base** (deno_core's JS plus the bootstrap, warmed by one request so its bytecode is in it), before any other
   isolate exists. The price is the bundle's top-level code in every request (§10.3).
2. **No snapshot at all on macOS.** Creating and disposing isolates from a snapshot aborts the process on macOS arm64
   in most runs of 2,000-3,000 isolates (`pointer being freed was not allocated`, `BackingStore::~BackingStore` from
   `Heap::TearDown`). It reproduces with deno_core alone and its own snapshot, with V8's default allocator and with
   our platform; never without a snapshot (0 of 32 runs) and never on Linux arm64 (0 of 26). So isolates are made
   from the base snapshot on **Linux** (0.7 ms) and without one elsewhere (2.5 ms: deno_core's start-up and the
   bootstrap each time); `FC_FN_JS_SNAPSHOT=true|false` overrides. Production hosts run Linux; macOS is fc-dev's.

Also: deno_core's V8 platform lets a delayed V8 task outlive its isolate (a tokio task keeps the queue it lands in),
and a task's destructor reaches into its isolate. With isolates per request that is every request that allocates, so
`fc-fnhost-js` runs its own platform (`src/platform.rs`): immediate tasks run on the isolate's thread at each poll,
delayed ones are held, and everything left is destroyed before the isolate is.

### 10.2 Method

`crates/fc-fnhost-js/tests/js_density.rs` (ignored tests), release:

```
cargo test --release -p fc-fnhost-js --test js_density -- --ignored --nocapture --test-threads 1
```

- *Loaded functions*: N copies of a bundle as distinct warm functions in one host (`limits.wasmMemoryMb` 32), memory
  after N=100 and after N=1000; the slope is the per-function cost. macOS: `proc_pid_rusage` (`ri_resident_size`,
  `ri_phys_footprint`) as in §1; Linux: `/proc/self/status` `VmRSS` and `RssAnon` (anonymous: the analogue of fp).
- *In flight*: 64 requests to one function, each awaiting a 1.5 s timer (64 isolates alive), against the same
  process idle.
- *Latency*: the phases of one request in process (no HTTP); `invoke` in process (worker hand-off and watchdog
  included); and through the real listener, loopback HTTP included, `FC_FN_MAX_EXECUTING=4`.
- *Bundles*: `hello.mjs` (the TypeScript template built by esbuild, 988 B; code cache 1,320 B), and the same padded to
  256 KiB and 1 MiB with generated exported functions, the way a bundle with inlined npm dependencies grows.
- *Machines*: Linux arm64 in Docker Desktop's VM on the M4 Pro of §1 (14 vCPUs, 4 KiB pages, `rust:1-bookworm`): the
  production configuration (base snapshot). And the M4 Pro itself (macOS 15.6, 16 KiB pages): fc-dev's
  configuration (no snapshot), at low load (load average ≈ 5). One run per point: treat absolute latencies as
  indicative; H7 should redo them on dedicated hosts.

### 10.3 Numbers

**Memory**

| | Linux arm64 (snapshot), RSS / anonymous | macOS arm64 (no snapshot), RSS / fp |
|---|---:|---:|
| the engine (V8 platform, base snapshot if any, 4 workers): process delta | 18.6 / 3.6 MiB | 4.8 / 1.0 MiB |
| **per loaded function, 1 KiB bundle** (slope 100→1000) | **24 / 24 KiB** | 29 / 29 KiB |
| per loaded function, 256 KiB bundle | 546 / 546 KiB | 457 / 226 KiB |
| **per request in flight** (one live isolate) | **1.84 MiB** | 3.4 MiB |

- An idle JS function holds its bundle, its main module and its code cache, nothing else: **≈ 43,000 idle 1 KiB
  functions per GiB** on Linux, ≈ 1,900 per GiB at 256 KiB each. The WASM component is ≈ 7,200 per GiB anonymous
  from `.cwasm` (§2.1). A JS function's idle cost is about twice its bundle.
- A request in flight costs an isolate: 1.84 MiB from the snapshot (the spike measured 1.65 MB), 3.4 MiB without (the
  spike: 3.5 MB). Memory scales with **concurrent requests**, not loaded functions: 1,000 in flight ≈ 1.8 GiB.
  `FC_FN_MAX_CONCURRENCY` (512 by default) and each function's `maxConcurrency` bound it.

**One request's phases**, in process (1 KiB bundle, 1,800 samples):

| phase | Linux, snapshot: p50 / p99 | macOS, no snapshot: p50 / p99 | macOS, snapshot (`FC_FN_JS_SNAPSHOT=true`) |
|---|---:|---:|---:|
| create the isolate | 0.74 / 0.85 ms | 2.54 / 3.04 ms | 0.68 / 0.84 ms |
| load the bundle from its code cache, run its top-level code | 0.045 / 0.066 ms | 0.078 / 0.110 ms | 0.045 / 0.087 ms |
| the call (dispatcher, `Request`, handler, `Response`) | 0.13 / 0.28 ms | 0.34 / 0.44 ms | 0.11 / 0.28 ms |
| teardown (after the answer is sent) | 0.06 / 0.16 ms | 0.07 / 0.15 ms | 0.06 / 0.17 ms |
| `invoke` in process (hand-off to a worker, watchdog) | 1.07 / 1.49 ms | 2.96 / 3.53 ms | |

**Through the listener** (loopback HTTP included):

| bundle | lazy first call (check + code cache + request) | steady c=1 p50 / p99 | c=16, 4 workers |
|---|---:|---:|---:|
| 1 KiB, Linux | 3.1 ms | 1.09 / 1.41 ms | 4,015 calls/s |
| 256 KiB, Linux | 8.1 ms | 1.81 / 2.24 ms | 2,272 calls/s |
| 1 MiB, Linux | 23.2 ms | 4.39 / 5.84 ms | 870 calls/s |
| 1 KiB, macOS (no snapshot) | 55 ms¹ | 3.24 / 3.80 ms | 1,257 calls/s |
| 256 KiB, macOS | 12.1 ms | 4.23 / 4.92 ms | 958 calls/s |
| 1 MiB, macOS | 29.4 ms | 6.97 / 7.59 ms | 527 calls/s |
| *WASM `echo` component, Linux, same VM* | *19.8 ms compiled, 1.8 ms from `.cwasm`* | *0.12 / 0.19 ms* | *37,986 calls/s* |

¹ The first function of the process: V8's first isolate without a snapshot.

- **About 1 ms of work per request for a small bundle on Linux**, three quarters of it creating the isolate. The
  spike's warm call (2.3 µs) is what a *reused* isolate costs; a fresh isolate per request is what the other ~1 ms
  buys. A WASM component is ≈ 10× cheaper per request.
- **A bundle's size is paid per request**: its top-level code runs in every isolate (on Linux, a 256 KiB bundle of
  declarations costs about +0.7 ms, 1 MiB about +3.3 ms). A snapshot per version would remove this; the read-only-space
  crash rules it out in-process today (§10.1).

### 10.4 One executing budget for both runtimes (2026-09-27)

As first built, `FC_FN_MAX_EXECUTING` was applied per runtime: the WASM guests' runtime and the JS workers each had
that many threads, and the threads were the cap, so a host running both kinds could execute up to twice that many
guests at once. The owner ruled it one host-wide budget. Both runtimes now take permits from one FIFO semaphore
(`fc-fnhost-core/src/exec.rs`), held only while a guest's future is polled: a WASM guest gives its permit back at
every epoch tick, fuel yield and waiting host call; a JS isolate at the end of every event-loop turn (so a computation
holds it until its next `await`). A guest awaiting I/O holds no permit and no thread. Each runtime keeps
`FC_FN_MAX_EXECUTING` threads so either alone can use the whole budget. Each JS worker is a lane: its isolates take
it before they queue for a permit, so a permit is never handed to an isolate whose thread is busy.

Cost, macOS arm64 (M4 Pro, release, the same binaries' ignored tests before and after, run alternately on a machine
shared with other builds; indicative only):

| | before | after |
|---|---:|---:|
| WASM `echo`, c=16 through the listener (`wasm_neighbour`) | 31,200-34,500 calls/s | 32,100-34,400 calls/s |
| WASM, A's p99 beside B spinning, cap 3, B=8 | 0.97-1.04 ms | 0.93-1.84 ms |
| WASM, A's p99 beside B spinning, cap 8, B=8 | 6.6-8.0 ms | 2.0-9.1 ms |
| JS `hello` 1 KiB, c=16, 4 workers (`js_density`) | 1,235 calls/s | 1,200 calls/s |
| JS `hello` + 256 KiB, c=16 | 880-906 calls/s | 868-912 calls/s |
| JS `invoke` in process, c=1, p50 | 3.05 ms | 3.43 ms |

No cost shows above the noise: taking a permit is one atomic when one is free, and the WASM guests' 1 ms re-queue is
what the epoch tick already paid. `crates/fc-fnhost-js/tests/shared_budget.rs` asserts the budget (at most two of two
WASM and two JS CPU-bound guests at once with `FC_FN_MAX_EXECUTING=2`), that I/O holds no permit, and that the
deadline applies while a guest queues.

### 10.5 Build and binary

| fc-server, release, macOS arm64 | without `js` | with `js` (default) | difference |
|---|---:|---:|---:|
| binary | 100.8 MB | 160.1 MB | +59.3 MB |
| stripped | 83.3 MB | 127.2 MB | **+43.8 MB** |
| gzip of stripped | 33.0 MB | 49.1 MB | **+16.1 MB** |

- Build time: a clean release build of fc-server without `js` took 17 m 46 s; adding `js` then took 11 m 47 s
  (deno_core, the V8 link, and the crates whose features unify differently), both on a heavily loaded machine. A clean
  debug build of `fc-fnhost-js`'s tests took 5 m 19 s on macOS and 2 m 31 s on Linux arm64 (Docker).
- V8's prebuilt archive is fetched by the `v8` crate's build script from GitHub at build time (`RUSTY_V8_ARCHIVE` /
  `RUSTY_V8_MIRROR` point it at a local file or a mirror). Prebuilts exist for macOS, Linux (glibc) and Windows on
  x86_64 and arm64; **not for musl**.
- Verified: macOS arm64 (all tests), Linux arm64 (all tests, Docker), Linux x86_64 (all tests serially under
  emulation; three timing assertions failed in parallel under emulation and were widened). Windows is untested.
- fc-server and fc-dev without the `js` feature carry no V8.

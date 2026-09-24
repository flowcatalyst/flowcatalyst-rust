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

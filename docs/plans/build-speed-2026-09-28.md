# Build speed: measurements, settings, and the crate split

Owner request, approved 2026-09-28: make the development loop fast. It bears
on whether Rust is fast enough to own.

- **Part A** (this branch, `perf/build-speed`): measure; apply the low-risk
  build settings; design the crate split. Build configuration, measurement
  scripts, and this document only. **Behaviour and release output are
  unchanged.**
- **Part B** (after the phase 2 authorization branch lands): merge the test
  binaries, split `fc-platform`, clean up imports, and convert `async_trait`
  where it pays.

The scripts in `scripts/build-bench/` produce every number here; part B reruns
them unchanged and compares.

## Summary

**Where the time goes.** fc-platform is one 150,000-line crate whose
macros expand to 477,000 more lines. An incremental `check` after any
one-line edit spends most of its time on whole-crate passes that don't get
cheaper with the edit's size: macro expansion, parsing, name
resolution, and loading and saving the incremental cache. Type checking
and borrow checking are incremental and nearly free. So a leaf edit and a
core edit cost the same, about 5.7 s of CPU (6–12 s wall on this loaded
machine). The one-test loop (edit, rebuild one test, run it) is 10.8 s of
CPU (13–24 s wall). Rebuilding all 63 fc-platform test binaries after an
edit is 155 s of CPU (36–41 s wall) and 64 links. On a cold build, the
vendored OpenSSL build script is the critical path. A worktree's `target/`
reaches 20 GB on macOS after `cargo test -p fc-platform`. On Linux each
test executable is 524 MB, so the 63 of them come to 33 GB.

**Build settings (applied).** One change is applied: dependencies build
with `debug = "line-tables-only"`. On macOS it has no measurable effect:
cargo already uses unpacked debug info there, so the debug-info settings
don't matter. On Linux it makes each test executable 13 % smaller, and
backtraces keep file:line. `opt-level = 1` for dependencies saves 20 % of
cold-build CPU but makes CPU-bound tests 25 % slower, so it is not applied.
`split-debuginfo` is already the macOS default and can't be set per OS.
On Linux it halves the size of each test executable: recommended for CI as
an environment variable. Linux linkers: GNU ld takes 7–8 s to link one test
binary, the repo's lld 1–2.5 s, mold 0.6–0.8 s. Cranelift is not worth
recommending: codegen is a quarter of an incremental rebuild, and
Cranelift needs nightly.

**Developer setup.** Build with the system OpenSSL
(`OPENSSL_NO_VENDOR=1`): −10 % CPU on a cold build, and it takes the
longest build script off the critical path. Keep one `target/` per worktree
and clean them up. sccache is optional: a second worktree gets 65 % of its
Rust compiles from the cache, but gains only 5 % in wall time, because the
critical path isn't cacheable. It gets no hits at all if `CARGO_TARGET_DIR`
or any per-worktree `CARGO_*` variable is set. On Linux, use mold.

**Crate split (design).** 47 of fc-platform's 95 modules (134,000 lines)
form one cycle, through four hubs:

- every `routes.rs` takes `PlatformContext`, which names every aggregate;
- the kernel (`AuthContext`, the middleware, database seeding) imports IAM
  types;
- the permission catalogue lives in `role`;
- the IAM modules reference each other.

The proposal is seven crates: core (12k lines), iam (54k), auth (21k),
messaging (29k), scheduled-jobs (6.5k), functions (16k), and a thin
`fc-platform` assembly crate (12k) that re-exports every module at its
current path. The breaks are 17 moves or inversions. The checker script confirms zero
violating references, and each of the four coherence breaks it finds (two
inherent impls, two orphan `From` impls) has a fix. Step 2
adds six narrow lookup traits, which cut messaging, scheduled-jobs and
functions loose from iam. Expected effect: an average edit rebuilds 40–47 %
of today's front-end work on the critical path. Edits in functions, auth
or scheduled jobs drop to about 20 %. Core edits stay expensive.

**Part B.** In order:

1. Merge the test binaries into one per crate. Expected: 155 s → about
   35 s of CPU after an edit, and about 11 GB less disk on macOS (more on
   Linux).
2. Split out core.
3. Split out functions and scheduled-jobs.
4. Split out messaging.
5. Split out iam and auth.
6. Add the step 2 traits.
7. Clean up imports as each crate moves, then enable
   `clippy::absolute_paths`.
8. Convert `async_trait` last, and only if a pilot shows a gain. The
   profile says async trait solving is the front end's biggest cost, and
   native `async fn` makes those futures bigger.

## 1. Method

**Machine.** macOS 15 (Darwin 24.6), Apple Silicon, 14 cores, 48 GB. Rust
1.98.1 (LLVM 22.1.8), Xcode's `ld` (ld-prime, `PROGRAM:ld PROJECT:ld-1230.1`).
Production and CI build on Linux.

**Load.** Two to four other agents were building on the machine the whole
time. The 1-minute load average ranged from 40 to over 500 (14 cores). Every
row records the load before the run. Every wall time here is inflated by that
load and varies a lot from run to run. So:

- incremental scenarios run five times and report the median and the range;
- `build s` is cargo's own total from `--timings`, which leaves out time spent
  blocked on another cargo's lock (one run waited 760 s for a lock; its wall
  time is in the CSV, but not in the medians);
- `cpu s` (user + sys of the whole build, from `/usr/bin/time`) is much less
  sensitive to load than wall time; the settings comparisons use it too;
- `fc-platform front end` is rustc's front-end section for fc-platform
  (parse, expand, resolve, type-check, borrow-check) from cargo's per-unit
  `sections`; the rest of a unit is codegen.

Treat single numbers as ±30 %. The ratios between scenarios measured back to
back are more reliable than the absolute times.

**Scripts** (`scripts/build-bench/`):

| Script | What it does |
|---|---|
| `bench.sh` | The scenarios: `cold-check`, `cold-build`, `cold-test`, `incr-check`, `incr-build`, `test-one`, `test-all`, `size`. Each edit inserts a comment line at the top of a file (a real content and span change) and restores the file afterwards. Appends to `target/build-bench/results.csv`, keeps every `--timings` report. Refuses to build below 35 GB free. |
| `experiment.sh <label>` | One settings experiment: `cold-test`, `test-one` ×5, `test-all` ×2, `size`, optionally `incr-check` ×5, under `disk-guard.sh`. |
| `timed-link.sh` | Linker wrapper `bench.sh` sets as the host linker; logs every link's duration, so link time is split from compile time. |
| `disk-guard.sh` | Kills a benchmark when free space drops below 35 GB or `target/` passes 50 GB. |
| `summary.py` | Median [min–max] per label / scenario / subject (`--md` for this document). |
| `timings.py` | Key numbers from `--timings` reports: slowest units, each `fc-*` unit's front end / codegen split. |
| `module_graph.py` | The module dependency graph inside a crate from its `crate::` / `super::` paths (use trees, re-exports resolved), strongly connected components, every reference with file:line. |
| `crate_split_check.py` + `crate-split.txt` | Checks a proposed split against that graph: references that would point up or sideways (cycles), inherent impls and orphan-rule breaks. |
| `async_trait_inventory.py` | Every `#[async_trait]` trait in the workspace, split by whether it is used as `dyn`. |
| `sccache-bench.sh` | sccache across two real worktrees (created under `target/`, removed afterwards). |
| `linux-docker.sh` + `linux-inner.sh` | The Linux runs in Docker: a cold test build, the one-test loop relinked with GNU ld, lld and mold, test-executable sizes; `--dev-debug-deps`, `--split-debuginfo` for the variants. |
| `results/2026-09-28-*` | This document's raw data: every CSV row, the `target/` and Linux size breakdowns, the sccache statistics. |

Rerun, e.g.:

```sh
BENCH_LABEL=baseline scripts/build-bench/bench.sh cold-check fc-server
BENCH_LABEL=baseline scripts/build-bench/bench.sh incr-check crates/fc-platform/src/client/repository.rs fc-server 5
scripts/build-bench/experiment.sh baseline --with-check
scripts/build-bench/summary.py --md
python3 scripts/build-bench/module_graph.py crates/fc-platform/src --json /tmp/graph.json > /tmp/graph.txt
python3 scripts/build-bench/crate_split_check.py /tmp/graph.json scripts/build-bench/crate-split.txt
```

The test used for the one-test loop is `scheduled_job_cron_golden_test`
(small, no Docker). The leaf file is `crates/fc-platform/src/client/repository.rs`.

## 2. Baseline (`main` at `ae0cd0d0`)

All on `main` as it is (the `baseline` rows; later rows of the same
scenario are the settings experiments, section 4). `build s` is cargo's own
total, `cpu s` user + sys, `load` the 1-minute load average before each run;
median [min–max].

| Scenario | Runs | build s | cpu s | fc-platform s (front end) | link s | load |
|---|---:|---|---|---|---|---|
| cold `cargo check -p fc-platform` | 1 | 125.7 | – | 37.5 (36.6) | – | 71 |
| cold `cargo check -p fc-server` | 1 | 173.4 | – | 38.9 (38.3) | – | 53 |
| incremental `check -p fc-server`, leaf edit (`client/repository.rs`) | 5 | 12.4 [7.1–34.9] | – | 10.5 [5.7–31.2] (9.2) | – | 61 [47–160] |
| same, measured later with CPU time (final settings) | 5 | 7.4 [6.1–7.9] | 5.7 [5.6–6.2] | 5.8 [5.2–6.4] (4.6) | – | 159 [126–198] |
| incremental `check -p fc-server`, core edit (`usecase/unit_of_work.rs`) | 5 | 9.1 [8.6–17.4] | – | 7.7 [7.2–15.8] (6.0) | – | 80 [61–115] |
| incremental `check -p fc-server`, fc-router edit (`health.rs`) | 5 | 1.5 [1.3–2.5] | – | not rebuilt | – | 41 |
| cold `cargo test -p fc-platform --test scheduled_job_cron_golden_test --no-run` | 2 | 340 / 458 | 1,882 | 66 / 98 | 15 (131 links, all but one build scripts) | 74 / 456 |
| **test loop**: leaf edit, rebuild that one test, run it | 10 | 23.5 [13.0–59.9] | 10.8 [9.7–15.9] | 22.5 [12.1–58.6] | 0.7 [0.3–2.3] | 174 [128–248] |
| leaf edit, `cargo test -p fc-platform --no-run` (all 63 test binaries relink) | 4 | 38.1 [36.1–41.0] | 154 [146–163] | lib 10.7–16.7 (front end 7.2–11.3) | 98 [81–114] summed over 64 links | 185 [144–246] |

The test loop splits into: fc-platform rebuilt (the lib unit, 12–58 s, of
which front end 8–35 s and codegen 4–5 s), the test crate compiled and
linked (1.2–1.7 s, link 0.3–0.9 s), the test run (0.7–1.2 s; 0.5 s alone on
a quiet machine).

**A core edit costs the same as a leaf edit today**: one crate, so any edit
re-runs the same whole-crate front end (section 3). After the split they
differ (section 6.4).

**`target/` after `cargo test -p fc-platform --no-run`: 20–21 GB** (this
worktree had also run `cargo check -p fc-server`):

| Part | Size |
|---|---:|
| `target/debug/deps` | 13 GB, of which the 64 test executables 6.3 GB (100–160 MB each) |
| `target/debug/incremental` | 9.2 GB, of which the 63 test crates 5.7 GB and fc-platform's variants 3.0–3.9 GB |
| `target/debug/build` | 0.25 GB |

A test executable on macOS carries no DWARF (it stays in the object files);
`go_routes_test` is 159 MB: 72 MB of code (`__TEXT`) and 84 MB of symbol
table and debug map (`__LINKEDIT`, 280,000 symbols). Each of the 63 carries
its own copy of fc-platform and its dependencies.

Other worktrees on this machine are 52–137 GB. `fc-authz/target/debug` (55
GB) has 827 incremental directories, 16 of them for fc_platform alone: every
`check` / `build` / `test` / `clippy` and every feature selection (`-p`
vs `--workspace`) builds its own variant (section 3).

## 3. Where the time goes

**Cold builds: vendored OpenSSL is the critical path.** In the cold `check
-p fc-server`, fc-platform starts the moment `webauthn-rs` is ready, which
waits for `openssl`, which waits for `openssl-sys`'s build script: it
compiles OpenSSL from source (`openssl = { features = ["vendored"] }` in
fc-platform, for the Windows release build). That script ran 28→131 s of the
173 s build, and 227 s of the 340 s cold test build. `aws-lc-sys`'s build
script (the aws-lc C library, for rustls' default crypto provider) is next,
at 82–122 s. Neither is a Rust compile, so sccache does not skip them
(section 5). After those: `cranelift-codegen` and `wasmtime` (dev-dependencies
through fc-fnhost-core, for the function-host tests: 144 and 75 s),
`utoipa-gen` (41–65 s), `aws-sdk-s3`, `sqlx-postgres`, two builds of `syn`.

**Incremental builds: fc-platform's front end, and most of it fixed cost.**
`-Z time-passes` on an incremental `check` of fc-platform after a leaf edit
(`RUSTC_BOOTSTRAP=1` on the stable compiler, for measurement only):

| Pass | Run 1 (load 380) | Run 2 (load 320) |
|---|---:|---:|
| total | 12.5 s | 9.7 s |
| macro expansion (`expand_crate`) | 6.2 s | 2.9 s |
| saving the incremental cache (`serialize_dep_graph`) | 1.6 s | 1.5 s |
| name resolution | 0.6 s | 0.7 s |
| type check (incremental: mostly reused) | 0.4 s | 0.6 s |
| borrow check (mostly reused) | 0.3 s | 0.3 s |
| the rest (parse, loading the dep graph, metadata) | ≈3.4 s | ≈3.7 s |

Type checking and borrow checking are incremental and nearly free. Parsing,
**macro expansion**, name resolution and loading/saving the dependency graph
are not: they run over the whole crate on every edit, and they are most
of the time (8–9 s of 9.7). They scale with the crate's size, which is why the
split pays.

**Macro expansion volume** (`-Z macro-stats`): fc-platform's 150,175 lines
expand to **477,577 lines (24 MB) of macro output**, 3.2 times the source.

| Macro family | Lines | Share | Biggest |
|---|---:|---:|---|
| serde | 176,079 | 37 % | `#[derive(Deserialize)]`: 590 uses, 141,407 lines (240 per use); `Serialize` 736 uses, 27,717 |
| tracing | 124,342 | 26 % | ~1,600 log statements; about 56,000 lines of it is the `log`-crate compatibility code, on because `sqlx-core` 0.8 enables `tracing/log` (and axum's default `tower-log`) |
| utoipa | 98,990 | 21 % | `routes!` 39,462 (16,359 recursive expansions from 228 calls), `#[utoipa::path]` 397 uses, 24,628; `ToSchema` 451 uses, 20,106; `OpenApi` 15 uses, 2,470 |
| async_trait | 24,981 | 5 % | 214 uses |
| other | 53,185 | 11 % | `Debug`, `Clone`, `format!`, `json!`, `try_join!`, `FromRow`, … |

**A full (non-incremental) front end** of fc-platform, `-Z self-profile`
and measureme's `summarize` (45 s of CPU on a quieter machine):

| Query (self time, and with its children) | Self | Total |
|---|---:|---:|
| `evaluate_obligation` (trait solving: 498,729 obligations) | 8.8 s (20 %) | 9.2 s |
| `typeck` | 8.5 s (19 %) | 10.3 s |
| `mir_borrowck` | 4.8 s (11 %) | 20.7 s |
| `check_well_formed` | 0.5 s | 11.9 s |
| `layout_of` | 0.2 s | 8.9 s |
| `check_coroutine_obligations` (an `async fn`'s future is `Send`, …) | 0.4 s | 7.6 s |
| `lower_to_hir` | 1.9 s | 2.6 s |
| `expand_crate` + `expand_proc_macro` | 2.9 s | 3.4 s |
| `coherent_trait` (all 746 traits) | 0.01 s | 0.7 s |

The cold front end is dominated by **trait solving and the async state
machines**: proving each handler's and use case's future `Send`, computing
coroutine layouts, borrow-checking the generated state machines. (`-Z
time-passes` files much of this under "coherence checking", the first pass
that forces it; coherence itself is 0.7 s.) This is why section 8 expects
little from native `async fn` in traits: it makes those futures bigger and
visible to more callers, the opposite of what this profile asks for.

**Tests.** `cargo test -p fc-platform --no-run` after a leaf edit rebuilds
fc-platform twice (the lib, and the lib as its own unit-test binary: 1,109
unit tests in 249 `#[cfg(test)]` modules; 97 s cold, 9–13 s incremental,
in parallel), then all 63 test crates, each 8–12 s under load, then 64
links (81–114 s summed). 146–163 s of CPU against 10–16 s for one test.

**Duplicate dependency builds.** Cargo unifies features per command: `check
-p fc-server`, `test -p fc-platform` and `--workspace` each resolve a
different feature set for many dependencies (and host builds differ from
target builds), so each gets its own copy. In this one worktree 200 of 896
rlibs exist in 2–5 variants (`tokio` and `serde` 5 times). Switching between
commands rebuilds them, and every variant stays in `target/`. Cargo's fix,
`resolver.feature-unification = "workspace"`, is still unstable in 1.98
(`-Zfeature-unification`; stable cargo warns and ignores it). The stable
workaround is a `cargo-hakari` workspace-hack crate (section 9).

## 4. Build settings (part A)

### 4.1 macOS (the development machines)

Each setting was measured with `experiment.sh` (a cold test build, the
one-test loop ×5, the all-tests relink ×2, `target/` size; opt-level also
the incremental check ×5). Baseline = `main`'s `.cargo/config.toml`
(`debug = 1` everywhere, dependencies at `opt-level = 2`).

| Setting | Cold test build: wall / CPU | One-test loop: CPU / link | All tests: CPU | `target/` | Test run |
|---|---|---|---|---|---|
| baseline | 340 s / 1,882 s | 10.8 s / 0.7 s | 154 s | 20 GB | 0.51–0.79 s |
| dependencies `debug = "line-tables-only"` | 331 s / 1,986 s | 9.4 s / 0.3 s | 163 s | 21 GB | 0.68–0.74 s |
| + workspace `debug = "line-tables-only"` | 333 s / 1,963 s | 10.0 s / 0.4 s | 164 s | 21 GB | 0.72–1.2 s |
| dependencies `opt-level = 1` (with line-tables-only) | 317 s / 1,591 s | 10.4 s / 0.4 s | 158 s | 19 GB | 0.85–1.13 s |

Incremental `check -p fc-server` after a leaf edit, back to back: opt-level 2
5.7 s CPU [5.6–6.2], opt-level 1 5.4 s [5.3–6.1] (the proc macros' opt-level
makes no measurable difference to expansion).

What this says:

- **Debug info: no measurable effect on macOS**, in time or in disk. Cargo
  already passes `-C split-debuginfo=unpacked` on macOS (checked in
  `cargo build -v`: DWARF stays in the object files and the linker only
  writes a debug map), and `debug = 1` was already "limited" (line tables,
  no variables). The 20 GB is code, symbols and incremental caches, not
  DWARF (section 2). The CPU figures vary ±5 % between identical
  configurations (the two line-tables-only rows have the same dependency
  settings).
- **`split-debuginfo = "unpacked"`**: already the macOS default; setting it
  would change nothing there. On Windows (MSVC) `unpacked` is not supported
  on stable, and a profile setting can't be made per-OS, so it is not set in
  the repo. For Linux see 4.2.
- **Dependencies at `opt-level = 1`**: 20 % less CPU on a cold build (1,591
  vs 1,882–1,986 s), but only 4–7 % less wall time (the critical path is
  the OpenSSL build script and fc-platform's front end, not dependency
  codegen), and CPU-bound test code 25 % slower (the cron golden test,
  which walks 2,000 schedules through `chrono-tz`: 0.85–1.13 s vs
  0.68–0.79 s). No effect on incremental builds. **Kept at 2**: cold builds
  are what sccache and fewer duplicate builds address, and test runtime is
  paid on every run.
- **Backtraces**: with dependencies at `line-tables-only`, frames compiled
  inside a dependency still show file:line:column (checked with a backtrace
  captured through `serde_json`'s non-generic `Display for Value`:
  `serde_json-1.0.151/src/value/mod.rs:232:33`), and workspace frames are
  unchanged.

### 4.2 Linux (CI, the Docker builder, production is arm64)

`linux-docker.sh` runs the same scenarios in Docker on this machine
(`rust:1-bookworm`, Rust 1.98.1, aarch64 like the production tasks; GNU ld
2.40, lld and mold from Debian). Rust 1.98 links `x86_64-unknown-linux-gnu`
with its bundled `rust-lld` by default (since 1.90), but not aarch64; this
repo's config asks for the system `lld` on both.

**Linkers**, relinking the one-test binary (the cron test, 350 MB) after a
leaf edit, three runs each, `main`'s settings (the quietest of the three
Linux runs):

| Linker | Link time |
|---|---:|
| GNU ld (bfd), the default on aarch64 without config | 7.2–8.4 s |
| lld (this repo's config) | 1.2–2.5 s |
| mold | 0.6–0.8 s |

The repo's lld is already 4–6× faster than GNU ld; mold halves it again.
mold stays a per-developer choice (section 5): it is an extra system
package, and CI and the release workflow are set up for lld. **The Docker
builder has neither**: it doesn't copy `.cargo/config.toml` (section 9), so
the arm64 image links with GNU ld (the amd64 one gets rustc's bundled
rust-lld by default). That is a release-output change, so it is
reported, not made.

**Debug info on Linux** (test executables carry their DWARF):

| Settings | A test executable (`go_routes_test`) | DWARF in it | `target/debug` (5 test binaries) |
|---|---:|---:|---:|
| `main` (`debug = 1` everywhere) | 524 MB | 384 MB | 12 GB |
| dependencies `line-tables-only` (applied) | 456 MB (−13 %) | 316 MB | 11 GB |
| + `split-debuginfo = "unpacked"` | 258 MB (−51 %) | 130 MB | 9.6 GB |

So a Linux `cargo test -p fc-platform --no-run` writes about 63 × 524 MB =
**33 GB of test executables** today: that, times feature-set variants, is
the 50–130 GB `target/` directories. Line tables for dependencies take 13 %
off; split DWARF (`.dwo` files next to the objects, only a skeleton and the
line tables in the executable; backtraces keep file:line) takes half.
**Recommended for CI**, as an environment variable on the test job so no
other platform is affected: `CARGO_PROFILE_DEV_SPLIT_DEBUGINFO: unpacked`
(not applied here: it can only be verified in CI). Merging the test
binaries (section 7) removes most of the rest.

### 4.3 Applied

`.cargo/config.toml` (one setting, plus corrected comments):

- `[profile.dev.package."*"] debug = "line-tables-only"`: neutral on macOS;
  on Linux, where every test executable carries its dependencies' DWARF,
  13 % smaller test executables (524 → 456 MB each, measured). The release
  profile is untouched.
- The linker comments: macOS uses Xcode's ld-prime (nothing to configure;
  the old "uncomment lld" block is gone); Linux keeps the system `lld`
  (unchanged), with mold as a per-developer option.

Not applied: `opt-level = 1` (above), workspace `line-tables-only` (no
gain, and it drops the function-level info `debug = 1` keeps),
`split-debuginfo` (default on macOS already; Windows can't use `unpacked`),
anything in the Dockerfile or CI workflows (they change the shipped binary
or can't be verified here; see section 9 and 4.2).

## 5. Recommended developer setup

In order of payoff. None of this goes in the repo's config: CI and other
machines don't have these tools, and each is a local choice.

**1. Build with the system OpenSSL** (macOS: `brew install openssl@3`).
The vendored OpenSSL build script is the longest step of every cold build
and sits on its critical path (section 3); sccache can't skip it.

```sh
# ~/.zshrc (or direnv's .envrc in each worktree)
export OPENSSL_NO_VENDOR=1
export OPENSSL_DIR="$(brew --prefix openssl@3)"
```

Measured, two alternating pairs of cold `cargo check -p fc-server`: 180 →
161 s and 192 → 187 s wall, 1,119 → 1,002 s CPU (−10 %). openssl-sys's
build script drops from ~114 s to 0.4 s, and the critical path moves to
aws-lc-sys (85–95 s of C), so the wall-clock gain is capped at about 20 s
until that one goes too. Dev binaries then link Homebrew's `libssl`;
release builds (CI, Docker) don't read your shell and still vendor it.

**2. One `target/` per worktree, and clean them.** Don't point several
worktrees at one shared `CARGO_TARGET_DIR`: cargo locks the build directory,
so concurrent builds wait for each other ("Blocking waiting for file lock on
build directory"), and every worktree's workspace crates still get their
own artifacts. Delete a merged worktree's `target/` (or `cargo clean`);
stick to one command set per worktree where possible (`cargo check
--workspace --all-targets`, `cargo test -p …`), because every different
`-p` / `--workspace` / `clippy` combination builds its own copy of many
dependencies (section 3).

**3. sccache: optional.** Measured with two real worktrees
(`sccache-bench.sh`): the second worktree's cold test build got **65 % of
its cacheable Rust compiles from the cache** (411 of 634; proc macros,
binaries and our incremental crates are never cacheable) and 21 % of C/C++. That cut the
summed compile time by 15 %, but the wall time by only 5 % (405 → 385 s,
under heavy load), because the critical path is the OpenSSL build script
and fc-platform itself, neither of which sccache caches. It adds nothing measurable
to incremental builds (it passes incremental compiles through: the one-test
loop took 16–19 s through sccache at load 190–310, against 13–24 s without
at 130–250). (The `cpu s` column can't be compared for sccache runs: the
compiles run in the sccache server, outside cargo's process tree.) Worth it when many
worktrees build at once and CPU is the bottleneck; not a big win for one
developer.

```toml
# ~/.cargo/config.toml (per developer, not the repo)
[build]
rustc-wrapper = "sccache"   # brew install sccache
```

```sh
export SCCACHE_CACHE_SIZE=30G   # the default 10G holds about three dependency sets
```

Two traps, both measured: sccache hashes every `CARGO_*` environment
variable rustc sees, so **setting `CARGO_TARGET_DIR` (or any per-worktree
`CARGO_*` value) gives zero hits**; and `SCCACHE_BASEDIRS` (0.18) does not
make Rust hits path-independent. Worktrees that each use their own default
`target/` and the same shell environment share the cache.

**4. Linux developers: mold** (`apt install mold`), 2–3× faster links than
the repo's lld (section 4.2):

```sh
export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
export CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=unpacked   # test executables half the size (4.2)
```

`RUSTFLAGS` replaces the repo's `[target.…] rustflags` (a
`~/.cargo/config.toml` entry would not: cargo joins `rustflags` arrays from
all config files and the repo's `-fuse-ld=lld` comes last, so it wins).

**5. macOS: nothing to set for the linker.** Xcode's ld-prime is the
default and links the 150 MB test executables in 0.3–0.9 s; unpacked debug
info is cargo's default.

**Not recommended:** nightly toolchains (Cranelift, the parallel front end:
section 9), `opt-level = 1` for dependencies (section 4.1), a shared
`CARGO_TARGET_DIR`.

## 6. The crate split (design)

### 6.1 The module graph today

`module_graph.py` over `crates/fc-platform/src`: 95 nodes (each top-level
module; `shared` split into its 60 files, because it is a grab bag; the
inline modules in `lib.rs`), 710 non-test edges, 150,175 lines.

**One strongly connected component holds 47 of the 95 nodes and 134,192 of
the 150,175 lines.** Every aggregate reaches every other one through a few
hubs:

1. **Wiring.** Every aggregate's `routes.rs` takes `&PlatformContext`
   (`shared::platform_context`), and `PlatformContext` names the states of
   `auth`, `mfa`, `portal`, `dispatch_job`, `service_account`, the email and
   rate-limit services, and the `Repositories` bundle, which names every
   repository. So every aggregate depends on every aggregate.
2. **The kernel reaches up.** `shared::authorization_service` (which also
   holds `AuthContext`, which `usecase::ExecutionContext` uses) imports
   `Principal`, `PrincipalRepository`, `RoleRepository`, `ApplicationRepository`,
   `AccessTokenClaims` and `role_names`. `shared::middleware` names
   `AuthService`. `shared::database` seeds the platform application, event
   types and built-in roles, and runs the scheduled-job cron migration.
3. **The permission catalogue lives in `role::entity`.** `client`, `event`,
   `dispatch_job`, `function`, `app_docs`, … name `role::entity::permissions`.
4. **IAM is one knot.** `auth` ↔ `principal` ↔ `mfa` ↔ `portal`,
   `application` ↔ `service_account` ↔ `auth` (OAuth clients), `principal` ↔
   `role` ↔ `application`.

Outgoing edges per module (reference counts, the hubs above left out:
`usecase`, `shared::error`, `tsid`, `enum_str`, `api_common`, `middleware`,
`platform_context`, `authorization_service`):

| Module | Lines | Depends on |
|---|---:|---|
| `audit` | 1,725 | shared::sdk_audit_batch_api (2), platform_config (1), principal (1) |
| `client` | 3,001 | application (11), role (1) |
| `application` | 5,064 | role (11), auth (9), service_account (8), client (4), shared::encryption_service (2), principal (1) |
| `platform_config` | 2,592 | shared::encryption_service (4), application (1) |
| `principal` | 9,267 | role (14), service_account (10), auth (9), application (6), mfa (6), email_domain_mapping (5), identity_provider (5), developer_credential (4), client (3), portal (3), shared::caller_reach (3), audit (2) |
| `role` | 5,949 | application (4), shared::application_roles_sdk_api (4), principal (2), shared::role_sync_service (2) |
| `service_account` | 5,692 | shared::encryption_service (10), auth (9), principal (5), role (5), client (3), connection (1), shared::caller_reach (1), shared::secret_ref (1) |
| `identity_provider` | 2,032 | email_domain_mapping (11), role (3), shared::secret_ref (3), shared::encryption_service (2), principal (1) |
| `email_domain_mapping` | 2,572 | identity_provider (6), principal (2), role (2) |
| `auth` | 20,153 | principal (35), portal (17), shared::rate_limit_store (15), login_attempt (11), identity_provider (7), role (7), shared::encryption_service (5), email_domain_mapping (4), mfa (4), shared::rate_limit_middleware (4), password_reset (3), application (2), service_account (2), shared::branding (2), shared::email_service (2), shared::secret_ref (2), developer_credential (1), platform_config (1) |
| `mfa` | 3,810 | auth (12), principal (8), shared::email_service (4), shared::rate_limit_store (4), audit (3), role (3), email_domain_mapping (2), identity_provider (2), login_attempt (2), platform_config (1), portal (1), … |
| `webauthn` | 2,025 | auth (6), email_domain_mapping (3), login_attempt (2), principal (2), shared::rate_limit_middleware (1) |
| `portal` | 6,223 | auth (20), identity_provider (5), shared::branding (4), shared::rate_limit_middleware (4), shared::rate_limit_store (4), client (3), shared::email_service (3), platform_config (2), principal (2), role (1), … |
| `event_type` | 4,630 | seed (1) |
| `event` | 2,112 | shared::caller_reach (5), role (2), shared::batch_api (2), dispatch_job (1) |
| `subscription` | 4,038 | shared::caller_reach (7), connection (4), dispatch_job (4), service_account (4), dispatch_pool (2) |
| `connection` | 2,174 | shared::caller_reach (4), service_account (2), subscription (2), application (1) |
| `dispatch_pool` | 2,485 | shared::caller_reach (7) |
| `dispatch_job` | 5,106 | shared::caller_reach (5), subscription (5), connection (4), service_account (4), application (2), role (2), scheduler (2), shared::batch_api (2), shared::sdk_dispatch_jobs_api (2), client (1), dispatch_job_actions (1), principal (1) |
| `scheduler` | 1,846 | dispatch_job (1) |
| `scheduled_job` | 6,651 | service_account (3), client (2), application (1), function (1), shared::caller_reach (1), shared::database (1), shared::webhook_signer (1) |
| `function` | 16,379 | subscription (16), scheduled_job (14), role (10), dispatch_pool (8), application (4), event_type (4), client (3), event (2), service_account (2), shared::batch_api (2), repository (1), … |
| `shared::authorization_service` | 2,086 | principal (5), auth (3), role (3), application (2), shared::caller_reach (1) |
| `shared::database` | 1,489 | application (2), role (2), event_type (1), scheduled_job (1), seed (1) |
| `shared::platform_context` | 259 | mfa (8), auth (7), portal (3), shared::email_service (3), dispatch_job (2), service_account (2), rate limits, secret_ref, repository, server_setup, … |

The full edge list, with every reference's file:line, is `module_graph.py`'s
output (`--json` for the machine-readable form).

### 6.2 Proposed crates

Seven crates. `fc-platform` stays the name of the assembly crate and
**re-exports every module at its current path** (`pub use
fc_platform_iam::principal;` …), so `fc-server`, `fc-dev`, `fc-web`, the
harnesses and the integration tests keep compiling against
`fc_platform::principal::…` unchanged.

Direct dependencies between the new crates:

| Crate | Step 1 | Step 2 (section 6.3) |
|---|---|---|
| `fc-platform-core` | – | – |
| `fc-platform-iam` | core | core |
| `fc-platform-auth` | iam | iam |
| `fc-platform-messaging` | iam | **core** |
| `fc-platform-scheduled-jobs` | iam | **core** |
| `fc-platform-functions` | messaging, scheduled-jobs | messaging, scheduled-jobs |
| `fc-platform` (assembly) | auth, functions | auth, functions, iam |

In step 2 an IAM edit no longer rebuilds messaging, scheduled-jobs or
functions.

| Crate | Modules | Lines | Depends on |
|---|---|---:|---|
| `fc-platform-core` | `usecase`; `shared::{error, tsid, enum_str, jsonb_text, api_common, rejection, capped_body, log_throttle, encryption_service, secret_ref, secret_backfill, database (minus seeding), indexes, email_service, webhook_signer, rate_limit_store, rate_limit_middleware, middleware, caller_reach}`; from `shared::authorization_service`: `AuthContext`, `Credential`, `checks`; from `role::entity`: `permissions`, `matches_pattern`; from `principal::entity`: `UserScope`, `PrincipalType` | 12,400 | fc-common, fc-function-model (`UnknownEnumValue`, and the `From<ValidationError>` impls), fc-http-listener (`PeerAddr`); sqlx, axum, utoipa |
| `fc-platform-iam` | `client`, `application`, `application_openapi_spec`, `platform_config`, `cors`, `audit`, `app_docs`, `principal`, `role`, `service_account`, `developer_credential`, `identity_provider`, `email_domain_mapping`, `login_attempt`, `password_reset`; from `auth`: OAuth clients, anchor domains, auth configs, IdP role mappings (entity, repository, operations, admin APIs), `auth_service` (JWT), `password_service`, `signing_keys`, the reset emailer; from `mfa`: `entity`, `repository`, `notify`; from `portal`: `entity`, `repository`, `policy`, the OAuth-client plane helpers; `shared::{authorization_service (AuthorizationService, ApplicationAccessService), role_sync_service}` | 54,000 | see above |
| `fc-platform-auth` | the sign-in flows: `auth::{oauth_api, oidc_login_api, auth_api, password_reset_api (endpoints), refresh_*, authorization_code*, oidc_*, pending_auth_repository, jwks_cache, login_backoff, session_cookie}`, `mfa` (login, self-service, admin APIs), `webauthn`, `portal` (login, OIDC, token), `shared::{branding, client_selection_api, me_api}` | 20,900 | see above |
| `fc-platform-messaging` | `event_type`, `event`, `subscription`, `connection`, `dispatch_pool`, `dispatch_job`, `dispatch_job_actions`, `scheduler`, `process`, `seed`, `service_account::signing_reach`; `shared::{batch_api, dispatch_process_api, dispatch_queue, sdk_dispatch_jobs_api, projections_service}` | 28,600 | see above |
| `fc-platform-scheduled-jobs` | `scheduled_job` (+ `java_fixed_offset_seconds` from `function::schedule_check`) | 6,500 | see above |
| `fc-platform-functions` | `function` | 16,200 | see above |
| `fc-platform` (assembly) | `router`, `repository::Repositories`, the lib.rs re-export modules, every aggregate's `routes.rs` (wiring: builds states and use cases from `PlatformContext`), `shared::{platform_context, server_setup, routes, openapi_contract, openapi_api, filter_options_api, sdk_sync_api, sdk_sync_go_api, sdk_audit_batch_api, application_roles_sdk_api, bff_*, debug_api, go_read_aliases_api, health_api, monitoring_api, public_api, well_known_api, router_config_api, platform_config_api, profile_only, bootstrap_admin, default_processes, integrity_scan}`, the seeding from `shared::database` | 11,600 | see above |

`crate-split.txt` is this table in machine-checkable form, and
`crate_split_check.py` reports **zero violating references** for it (step 1),
with the moves and inversions below. It also checks coherence (every
`impl Type` in the crate that defines `Type`, every trait impl local by the
orphan rule) and finds four breaks, the last three rows of the table below
plus `impl AuthContext` (its constructors, second row).

### 6.3 Cycles and how each is broken

Every item below is one part B change. "Move" means the file (or the items)
change crate, not behaviour.

| Cycle | Break | Kind |
|---|---|---|
| every `routes.rs` → `PlatformContext` → every aggregate | `*/routes.rs` (28 files, 3,695 lines) move to the assembly crate; they are wiring. Domain crates export their states, handlers and `__path_*` items (`pub`). | move |
| `usecase::ExecutionContext` → `AuthContext` → `Principal`, `AccessTokenClaims`, `role_names` | `AuthContext`, `Credential` and `checks` move to core with `UserScope` and `PrincipalType` (the enums it carries) and the permission catalogue. `AuthContext::for_session(&Principal, …)` and `from_claims*(&AccessTokenClaims, …)` become IAM free functions (an inherent impl can't live outside core). | move + small refactor |
| `shared::authorization_service::AuthorizationService` (repository-backed) in the kernel | Moves to iam with `ApplicationAccessService` and `ApplicationScope`. | move |
| `shared::middleware` → `AuthService`, `AuthorizationService` | Core defines `trait TokenAuthenticator` (validate a bearer/cookie token into an `AuthContext`); iam implements it; the binaries pass `Arc<dyn TokenAuthenticator>` to `AuthLayer`. | invert (trait) |
| `role::entity::permissions` used by every aggregate | The catalogue (`permissions`, `matches_pattern`) moves to core; `role::entity::roles` (built-in roles) stays in iam. | move |
| `shared::database` → seeding (`seed_platform_application`, `seed_platform_event_types`, `seed_builtin_roles`) | The seed functions move to the assembly crate (they run at startup, from the binaries). | move |
| `shared::database` migration runner → `scheduled_job::cron_migration::run` | The runner takes a list of code migrations (`&[(&str, fn(&PgPool) -> BoxFuture<…>)]`) the assembly registers. | invert (parameter) |
| iam `application`, `service_account` → `auth::oauth_*`, `auth::operations` | OAuth clients, anchor domains, auth configs and IdP role mappings are IAM aggregates: their entity, repository, operations and admin APIs move to iam. `auth` keeps the flows. | move |
| iam `principal` → `auth::password_service`, `auth::password_reset_api::PasswordResetEmailer`, `mfa::{MfaRepository, notify}`, `portal::policy` | These move to iam (the endpoints stay in auth). | move |
| iam `auth::oauth_clients_api` → `portal::{PortalAppRepository, validate_oauth_client_plane, resolve_oauth_client_portal_app, trimmed_or_none}` | Portal apps are the OAuth client's portal plane: `portal::{entity, repository}` and those helpers move to iam. | move |
| iam `service_account` → `connection::ConnectionRepository` (`signing_reach`) | `service_account::signing_reach` checks a connection's signers, and only messaging uses it: it moves to messaging. | move |
| iam `role::bff` → `shared::role_sync_service` | `role_sync_service` moves to iam. | move |
| iam `app_docs` → `shared::sdk_sync_api::SyncResultResponse` | The DTO moves to core (`api_common`). | move |
| scheduled-jobs → `function::schedule_check::java_fixed_offset_seconds` | Moves to `scheduled_job` (or core). | move |
| functions `trigger_sync` → `repository::Repositories` | Takes the repositories it uses. | invert (parameter) |
| `function/mod.rs`: `impl From<ValidationError> for UseCaseError` / `PlatformError` | Orphan once split. Move the two impls to core (core depends on `fc-function-model`, which is small) or replace them with a `map_err` helper. | coherence |
| `impl UserScope` in `principal/entity.rs` | Moves to core with `UserScope`. | coherence |

**Step 2: flatten.** With step 1, an IAM edit rebuilds messaging,
scheduled-jobs and functions too. What they use from iam is narrow: 34
references to 19 items, all lookups (`ClientRepository`,
`ApplicationRepository`, `ServiceAccountRepository`, `PrincipalRepository`,
`OutboundCredentialsResolver`, `ApplicationAccessService`,
`ApplicationScope`). Six narrow traits in core (`ClientDirectory`,
`ApplicationDirectory`, `ServiceAccountDirectory`, `PrincipalDirectory`,
`OutboundCredentials`, `ApplicationAccess`), implemented in iam and injected
by the assembly as `Arc<dyn …>`, remove the edge. Check it with
`crate_split_check.py` after changing the two `crate` lines in
`crate-split.txt` to depend on `fc-platform-core` only.

**Not proposed:** splitting iam further. Tenancy (`client`, `application`,
`platform_config`, …) and identity (`principal`, `role`, `service_account`)
reference each other both ways (`application` creates service accounts and
OAuth clients; `principal`, `role` and `service_account` read applications
and clients). Breaking that needs a real redesign, not moves.

### 6.4 Expected gain

In an incremental build, a crate's fixed cost (expanding every macro,
resolving, hashing, loading and saving the dependency graph) scales with its
size, and cargo rebuilds every crate downstream of an edit (no early cut-off:
a changed rlib rebuilds its dependents even when its interface is the same).
So an edit costs roughly the lines on the critical path from the edited
crate to the binary.

Model: the incremental front end of a crate scales with its lines (the
fixed passes in section 3), downstream crates rebuild in dependency order,
siblings in parallel. Lines on the critical path (and CPU: all rebuilt
lines) as a share of today's 150,000, per edited crate:

| Edit in | Step 1: critical path | Step 1: CPU | Step 2: critical path | Step 2: CPU |
|---|---:|---:|---:|---:|
| core | 82 % | 100 % | 66 % | 100 % |
| iam | 74 % | 92 % | 58 % | 58 % |
| auth | 22 % | 22 % | 22 % | 22 % |
| messaging | 38 % | 38 % | 38 % | 38 % |
| scheduled-jobs | 23 % | 23 % | 23 % | 23 % |
| functions | 19 % | 19 % | 19 % | 19 % |
| assembly | 8 % | 8 % | 8 % | 8 % |
| **average, weighted by lines** | **47 %** | **55 %** | **40 %** | **43 %** |

The model is optimistic for the assembly crate (every `routes!` and the
`OpenApi` derives move there with `routes.rs`, heavy expansion for 11,600
lines) and ignores each crate's fixed cost of loading its dependencies'
metadata (a few hundred milliseconds). Applied to the measured numbers
(section 2, CPU seconds, which are the stable ones):

| Loop | Today | After the split (step 1 → 2) |
|---|---:|---:|
| incremental `check -p fc-server`, leaf edit in functions / auth / scheduled jobs | 5.7 s CPU, 6–12 s wall | ≈1.5–2 s |
| same, edit in messaging | same | ≈2.2 s |
| same, edit in iam | same | ≈4.2 → 3.3 s |
| same, edit in core | same | ≈5 s (no gain; core should change rarely) |
| one-test loop (build + run), leaf edit | 10.8 s CPU, 13–24 s wall | ≈5–6 s CPU |

Codegen (for `build` and `test`) follows the same shape: codegen is
already incremental per codegen unit, so it is mostly the edited crate's.

### 6.5 Mechanics

- Directory layout: `crates/fc-platform-core`, `crates/fc-platform-iam`, …
  next to `crates/fc-platform`. Workspace dependencies stay in the root
  `Cargo.toml`; each new crate takes only what it uses (fewer dependencies per
  crate is part of the gain: core does not need `webauthn-rs`, `openssl`,
  `aws-sdk-s3`, …).
- `pub(crate)` items used across the new boundaries become `pub` (the checker
  lists every cross-crate reference; `cargo check` finds the rest).
- The convention tests that scan source (`uow_convention_test`,
  `route_auth_convention_test`, `route_wiring_convention_test`,
  `handler_write_convention_test`, `permission_convention_test`,
  `junction_cascade_convention_test`) read `crates/fc-platform/src`: they
  must scan every `crates/fc-platform*/src`.
- `fc-web` (outside the workspace) and every binary keep using
  `fc_platform::…` through the re-exports.
- The `tests/data` and golden files stay with the tests.
- Order: core first (it breaks the kernel cycles and is the base of
  everything), then functions and scheduled-jobs (leaf crates, the least
  risk, the largest share of recent work), then messaging, then iam + auth,
  then step 2.

## 7. Integration tests: one binary per crate

**Today.** fc-platform has 63 integration-test files, each its own crate and
binary (the workspace has 129). Each links fc-platform and its ~600
dependencies.

Measured (section 2), after a leaf edit in fc-platform:

| | One test binary | All 63 (`--no-run`) |
|---|---:|---:|
| build (wall) | 13–24 s | 36–41 s |
| CPU | 10–16 s | 146–163 s |
| links | 1, 0.3–0.9 s | 64, 81–114 s summed |
| test executables on disk | – | 6.3 GB |
| test crates' incremental caches | – | 5.7 GB |

Most of the extra 140 s of CPU is the 63 test crates: each recompiles its
own copy of the generic code it instantiates (the router, `TestApp`, serde
and tower glue) and links its own 100–160 MB executable. On Linux each
executable also carries its DWARF (section 4.2), which is where the 50–130
GB `target/` directories come from.

**Expected, merged into one binary:** one test crate (32,700 lines)
recompiled incrementally after a lib change and one link, instead of 63 and
64. Estimate: `cargo test -p fc-platform --no-run` after a leaf edit from
36–41 s to about 20–25 s wall and from ~155 s to ~35 s of CPU; `target/`
about 11 GB smaller on macOS (6.3 GB of executables and 5.7 GB of test
incremental caches become one executable of ~0.2 GB and one cache of ~0.5
GB), more on Linux. Running the suite also gets faster: one process start,
one thread pool.

**Plan.** `crates/fc-platform/tests/it/main.rs` with one module per current
file (`mod client_admin; mod go_routes; …`), `support` becoming
`crate::support` instead of `#[path = "support/mod.rs"] mod support;`, and
`autotests = false` plus one `[[test]] name = "it"` in `Cargo.toml`. Test
names become `client_admin::…`; filters (`cargo test -p fc-platform --test it
client_admin`) still select a file's tests. The same for fc-fnhost-core (18),
fc-router (16), fc-server (6), fc-queue (5), fc-fnhost-js (5), … : 129
binaries become about 20.

**Per-binary state the tests rely on, and what to do about it:**

| State | Where | Merged-binary risk | Fix |
|---|---|---|---|
| `std::env::set_var("FLOWCATALYST_APP_KEY", …)` | 16 files, **three kinds of key**: `AAAA…=` (8 files), `MDEy…=` (6), a random `generate_key()` (2: `audit_redaction_test`, `portal_identity_test`) | a test encrypts under one key and reads back after another test set a different one | one key constant in `support`, set once (`Once`); better, `TestApp::setup` passes the key in the platform config instead of the process env |
| `remove_var("FLOWCATALYST_APP_KEY")` / `_PREVIOUS` | `route_table_snapshot_test` | removes the key under every other test | keep it a separate binary, or build its router with an explicit "no key" config |
| `set_var("FLOWCATALYST_DEV_MODE" / "FC_FN_SIGNATURES" / "FC_FN_ARTIFACT_STORE")` | `function_host_e2e_test` | switches dev mode and signature checks off for everything | keep it a separate binary (it is also the heaviest: it runs the JS host), or inject the settings |
| `remove_var("FC_RL_PASSWORD_RESET_EMAIL_PER_HOUR")` | 2 files | none: nothing sets it | none |
| `static APP_KEY: Once` | `portal_identity_test` | none | none |
| `#[ignore = "requires Docker"]` | 287 tests in 46 files | none: `-- --ignored` still selects them | none |
| a fresh Postgres container per test (`TestApp::setup`) | 37 files use `support` | none: each test keeps its own container | none |
| LocalStack / Redis containers | `dispatch_scheduler_test` | none | none |
| `tracing_subscriber` init | `dispatch_fan_out_test` | none: it already uses `try_init()` | none |

**Docker parallelism and isolation.** Today cargo runs the 63 binaries one
after another, each running its own tests in parallel (one thread per core).
In one binary the same thread pool runs every test, so concurrency is the
same (at most one container per test thread) and the tail of each binary no
longer idles the machine. Isolation stays per test: each test starts its own
container. To cap containers on a loaded machine, `RUST_TEST_THREADS=8`.
[cargo-nextest](https://nexte.st) would run each test in its own process
(env changes can't leak) and has test groups (`[test-groups] docker =
{ max-threads = 8 }`); worth adopting, but not required for the merge.

## 8. `async_trait` → native `async fn` in traits

`async_trait_inventory.py`: fc-platform has 219 `#[async_trait]` attributes
in `src/` (the 389 mentions of `async_trait` include the `use` lines); the
table counts the tests' impls too.

| Crate | Used as `dyn` (keep, or hand-written boxed-future traits) | Generic only (can convert) |
|---|---|---|
| fc-platform | 7 traits, 21 impls: `ArtifactBlobStore`, `DispatchPublisher`, `EmailService`, `RateLimitStore`, `RefreshTokenStore`, `SecretProvider`, `SecretStore` | 4 traits, 191 impls: `UseCase` (143), `Persist` (42), `UnitOfWork` (3), `LockedRead` (3) |
| fc-router | 6 traits, 34 impls (`Mediator`, `ConsumerFactory`, …) | – |
| fc-queue | 2 traits, 35 impls (`QueueConsumer`, `QueuePublisher`) | 2 traits, 4 impls |
| fc-fnhost-core | 9 traits, 31 impls | 1 trait (`Invoker`), 7 impls |
| fc-sdk | 6 traits, 16 impls | 3 traits, 4 impls (public SDK API: a semver decision) |
| fc-outbox, fc-common | 5 traits, 21 impls | – |
| **Workspace** | **35 traits, 193 attribute sites** | **10 traits, 216 attribute sites** |

Step 2 of the crate split adds six `dyn` traits (the directories); they
stay `async_trait` (or `Pin<Box<dyn Future + Send>>` by hand).

**Expected build effect: small, and possibly negative.** `-Zmacro-stats`:
the 214 `async_trait` expansions in fc-platform produce 25,000 lines, 5 % of
the crate's macro output (section 3), so the expansion saving is a few
percent of the incremental front end. Against that, the self-profile shows
the cold front end is dominated by trait solving over async futures
(`evaluate_obligation`, `check_coroutine_obligations`, `layout_of`): native
`async fn` exposes each use case's concrete future type to every handler
that awaits it, where `async_trait` erases it behind `Box<dyn Future>`, so
the handlers' futures get bigger and there is more `Send` proving to do.
That can cost as much as the expansion saves. The runtime gain (one
allocation less per call) is real but immaterial against a database round
trip.

Plan: convert `Persist`, `LockedRead` and `UnitOfWork` (48 impls) first and
measure with `bench.sh`; convert `UseCase` (143 impls) only if that shows a
gain. The trait methods need `Send` futures (axum handlers are `Send`):
declare them `fn execute(&self, …) -> impl Future<Output = …> + Send` and
keep writing `async fn` in the impls.

## 9. Other build-time items

- **Vendored OpenSSL** (the cold-build critical path, section 3).
  `openssl = { features = ["vendored"] }` is in fc-platform so the Windows
  release build needs no vcpkg. Developers can skip it today with
  `OPENSSL_NO_VENDOR=1` (section 5). A repo-level fix, vendoring only on
  Windows (`[target.'cfg(windows)'.dependencies]`), changes how the Linux
  release binaries and the image link OpenSSL (dynamically, against the
  runtime image's `libssl`): an owner decision, not a build setting.
- **aws-lc-sys** (82–122 s of C, rustls' default provider through reqwest
  and the AWS SDK). Switching the provider to `ring` is a crypto change:
  out of scope. sccache caches part of its C compiles (section 5).
- **utoipa.** `#[utoipa::path]` (397), `ToSchema` (451), `routes!` and the
  `OpenApi` derives are 21 % of the macro output. The derives are the
  price of the generated OpenAPI document (Go's contract) and stay. The
  split moves every `routes!` into the assembly crate, which every edit
  rebuilds (it is downstream of everything), so those 39,000 lines are
  expanded on every edit either way: keep the rest of the assembly crate
  thin. `routes!` is recursive `macro_rules` (16,359 expansions for
  228 calls): cost grows with the tokens per handler path, and the paths
  stay fully qualified (the route-auth scanner reads them). Acceptable.
- **serde `Deserialize`** is the single largest expansion (141,000 lines,
  30 %). 347 types derive both `Serialize` and `Deserialize`; a type that is
  only ever serialized (most responses) doesn't need `Deserialize`.
  An audit could remove a share of it; worth doing per crate during the
  split, with the event persistence snapshot tests as the guard.
- **tracing's `log` compatibility** (about 56,000 lines) comes from
  `sqlx-core` 0.8 enabling `tracing/log`; it goes when sqlx stops enabling
  it. Nothing to do now.
- **axum `Router` types are not a problem.** axum 0.8's `Router<S>` is type
  erased (each route is boxed into a `BoxCloneSyncService` when added), and
  so is utoipa-axum's `OpenApiRouter`: no nested generic type grows with the
  route count, and `.boxed()` / `into_make_service` boundaries would change
  nothing. Each of the ~400 handlers monomorphizes its own `Handler` impl
  and future; that is inherent.
- **Profiling the front end** (section 3): `RUSTC_BOOTSTRAP=1 cargo rustc
  -p fc-platform --lib --profile check -- -Zself-profile=<dir>` (the
  bootstrap variable is for measurement only; it rebuilds ~80 dependency
  units, so use a separate `CARGO_TARGET_DIR`), then measureme's
  `summarize summarize <file>.mm_profdata` (`cargo install --git
  https://github.com/rust-lang/measureme summarize`). Trait solving over
  async futures is the largest cost; very large `async fn` bodies (the
  2,000–3,000-line API files) are where it concentrates, and splitting a
  huge handler into smaller functions (or boxing a rarely-used branch's
  future) is the lever if one shows up.
- **Feature-unification duplicates** (section 3). Until
  `-Zfeature-unification` is stable, a `cargo-hakari` workspace-hack crate
  makes every command build one feature set: fewer rebuilds when switching
  between `check -p`, `test -p` and `--workspace`, and a smaller `target/`.
  It is a generated crate with every dependency's union of features (a
  supply-chain review item: it adds no new crates, only features). Worth a
  trial in part B; measure `target/` size and rebuilds when switching
  commands.
- **The Docker image is not built with the release profile in
  `.cargo/config.toml`.** The Dockerfile copies `Cargo.toml`, `crates/`,
  `bin/` … but not `.cargo/`, so the image's fc-server is built with
  cargo's default release profile (no thin LTO, 16 codegen units), unlike
  the release binaries and fc-dev (built from a checkout). It also links
  arm64 with GNU ld (the image has no lld). Changing either changes the
  shipped binary: reported for an owner decision, not changed here.
- **Cranelift** (nightly only). Codegen is a quarter of an incremental
  fc-platform rebuild (front end 13–35 s, codegen 4–5 s, section 2), and
  none of a `check`. Cranelift's faster codegen would save perhaps 2 s of a
  15–25 s test loop, at the price of a nightly toolchain (the workspace
  needs 1.96+, so the installed `nightly-2026-01-01` can't even build it)
  and a second codegen backend to trust. **Not worth recommending.** The
  nightly feature that would matter here is the parallel front end
  (`-Z threads=8`), and the split gets most of that benefit on stable by
  giving cargo several crates to build at once.

## 10. Imports (owner request, 2026-09-28)

**The problem.** The code uses inline absolute paths where `use` imports at
the top of the file are the norm (CLAUDE.md, "Imports"). Counted over
`crates/`, `bin/` and `harness/` (comments and strings stripped):

| Path | Inline (outside any `use`) | In function-local `use` | Raw occurrences, `use` lines included |
|---|---:|---:|---:|
| `std::collections::…` | 163 | (in the next row) | 343 |
| other `std::…` | 1,385 | 103 | 2,802 |
| `crate::…` | 2,391 (+ 291 inside `routes!`, which stay) | 292 | 5,037 |

**Plan.**

1. Do the cleanup **during the crate split**, crate by crate: moving a file
   to a new crate rewrites its `crate::` paths anyway (to
   `fc_platform_core::…` etc.), so each file is edited once. A codemod does
   most of it: collect each inline path, add a `use` for its last segment
   (or its parent module for functions, `use crate::x::y; y::f()`), replace
   the path, `cargo fmt`, `cargo check`. Name collisions (`Result`, `Error`,
   same-named DTOs in two modules) stay qualified by the parent module or get
   an alias; those are the hand-done part.
2. Then enable clippy's `absolute_paths` lint so the paths can't creep back:
   `clippy.toml` at the workspace root with `absolute-paths-max-segments = 2`
   (and `absolute-paths-allowed-crates` for any crate whose items read better
   qualified), and `absolute_paths = "warn"` under each crate's
   `[lints.clippy]` as that crate is cleaned. Confirm on the pinned
   toolchain that the lint covers `crate::` paths; if it only covers
   external crates, a grep-based convention test (like the existing ones)
   covers `crate::`.
3. **Handler paths inside `routes!(…)` stay fully qualified**: the
   route-auth scanner reads them. The lint skips code from macro expansion,
   which may already cover them; where it doesn't, the 28 `routes.rs` files
   get `#[allow(clippy::absolute_paths, reason = "route-auth scanner reads handler paths")]`
   on their route functions.

**Estimate.** About 4,300 sites in about 670 of the 1,056 source files. The codemod plus review
is 1–1.5 days; collisions and the lint rollout another 0.5–1 day. Done
during the split it adds perhaps a day in total, instead of a separate pass
that conflicts with every open branch. Not done in part A: the phase 2
authorization branch is editing these files.

## 11. Part B: order of work and expected numbers

Part B starts after the phase 2 authorization branch lands (it edits the
same files). Each step keeps behaviour byte-identical and runs the gates of
`docs/plans/platform-uniformity-2026-09-28.md` (check, clippy at baseline,
workspace tests, convention tests, the Docker suite, fc-web, the parity
harness, the event persistence snapshots), then reruns
`scripts/build-bench/` and records the numbers here.

| # | Step | Expected effect | Size |
|---|---|---|---|
| 1 | **Merge the test binaries**: `tests/it/main.rs` per crate (fc-platform first), one app-key constant in `support`, `route_table_snapshot_test` and `function_host_e2e_test` kept separate (or their env injected) | `cargo test -p fc-platform --no-run` after an edit: 36–41 s → ~20–25 s wall, ~155 s → ~35 s CPU; `target/` −11 GB on macOS, more on Linux; CI faster | 0.5–1 day |
| 2 | **fc-platform-core**: move the kernel (section 6.2), the permission catalogue, `AuthContext`/`checks`; the `TokenAuthenticator` trait; the migration-hook parameter; seeding to the assembly; import cleanup in the moved files | breaks the kernel cycles; small incremental gain alone | 2–3 days |
| 3 | **fc-platform-functions** and **fc-platform-scheduled-jobs** (leaf crates, the least risk); `routes.rs` files move to the assembly as their crates split out | edits there: incremental check ≈ 19–23 % of today | 1–2 days |
| 4 | **fc-platform-messaging** (incl. `signing_reach`) | 38 % for messaging edits | 1–2 days |
| 5 | **fc-platform-iam** + **fc-platform-auth** (the `auth` / `mfa` / `portal` file moves) | auth edits 22 %; the average across all edits ≈ 47 % | 2–3 days |
| 6 | **Step 2 directories**: six narrow traits in core, injected by the assembly | iam edits 74 % → 58 %; average ≈ 40 % | 1 day |
| 7 | `clippy::absolute_paths` on every new crate (section 10) | keeps the imports clean | with 2–5 |
| 8 | `async_trait`: convert `Persist`, `LockedRead`, `UnitOfWork`; measure; `UseCase` only if it gains (section 8) | ± a few % | 0.5 day + 1 day |
| 9 | Optional: cargo-hakari trial; `Deserialize` audit; profile the largest async API files (section 9) | fewer duplicate builds; less expansion | – |

Before / after, for the record part B keeps (CPU seconds are the robust
figures on this shared machine; wall in brackets):

| Loop | Before (part A) | Expected after part B |
|---|---|---|
| incremental `check -p fc-server`, average edit | 5.7 s (6–12 s) | ≈2.5 s (≈3–5 s) |
| one-test loop, leaf edit | 10.8 s (13–24 s) | ≈5–6 s (≈7–12 s) |
| `cargo test -p fc-platform --no-run` after an edit | 155 s (36–41 s) | ≈35 s (≈20–25 s) |
| `target/` after `cargo test -p fc-platform` (macOS) | 20–21 GB | ≈9–10 GB |
| cold test build in a new worktree | 340 s, 1,882 s CPU | part B changes little here: system OpenSSL (−10 % CPU) and sccache (−15 % compile time) are the cold-build levers (section 5) |

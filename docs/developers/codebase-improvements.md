# Rust Codebase — Comprehension & Targeted Refactors

## What this file is

A brief for an agent (or contributor) tasked with improving the Rust
codebase's comprehensibility and reducing ceremony where the type system or
ecosystem has moved on since the code was written. Scope is **comprehension
+ targeted refactors only** — no architectural changes, no wire-format
changes, no scope-trimming proposals.

If you're picking up scheduled work, check `docs/architecture/architecture-direction.md`
and `docs/sdk-parity-plan.md` first; both may overlap. Coordinate, don't
collide.

## Sibling repos (read-only references)

- **Go port:** `/Users/andrewgraaff/Developer/flowcatalyst-go/` —
  the operator-friendly port. Use it as a mirror to check what the same
  code looks like with less ceremony. See in particular:
  - `pkg/fcsdk/doc.go` — template for a top-level package doc
  - `internal/router/pool.go` and `lifecycle.go` — channel-ownership + select
    wakeup comments worth porting to Rust task-spawn sites
  - `internal/platform/application/operations/delete.go` (58 lines) vs
    `crates/fc-platform/src/application/operations/delete.rs` (166 lines)
    — the Rust version is more thorough (business-rule checks), but the
    Go version's per-operation file layout is a useful comparison

- **TypeScript:** `/Users/andrewgraaff/Developer/flowcatalyst/` —
  has `HANDOFF.md` at the root with parallel TS improvements queued.

## Observed comprehension blockers

Measured by reading and comparing the three implementations:

- `crates/fc-platform/src/application/api.rs` — **1671 lines**, one file.
  Past ~800 lines, comprehension drops sharply.
- `crates/fc-router/src/mediator.rs` — **623 lines**, mixes the HTTP
  delivery, webhook signing, response handling, retry, warning emission,
  and host-pool integration in a single file.
- Most `#[async_trait]` uses are now obsolete — Rust 1.75 (December 2023)
  shipped native `async fn` in traits. The macro is still needed for
  `dyn Trait` over async methods, but a substantial fraction of impls in
  the codebase don't dyn-dispatch and can drop the macro entirely.
- Speculative `Arc<Self>` / `Weak` patterns. Some are load-bearing (the
  sweep task in `mediator.rs` holding a `Weak<MediatorInner>` to break a
  cycle is correct). Others are defensive without a documented cycle.
  Each one is a comprehension tax for readers who must reason about
  why the indirection exists.
- Module-level `//!` documentation is patchy. Crates intended as library
  APIs (`fc-sdk`, `fc-common`) deserve a self-contained
  module doc on `lib.rs` that's the first thing `cargo doc` surfaces.
- Per-task lifecycles are documented inconsistently. Some `tokio::spawn`
  sites carry a comment explaining when the task exits; many don't.

## Prioritized work

Each item is a separate PR. Don't bundle. For items (3) and (4), open a
tracking issue listing every file you intend to touch before starting the
sweep.

### 1. Migrate `#[async_trait]` → native `async fn` in traits

**Scope.** Audit every `#[async_trait]` use:

```bash
rg -tn rust "#\[async_trait\]" crates/
```

For each trait:
- If the trait is used via static dispatch (generic bounds: `fn foo<T: MyTrait>()`),
  the macro can be removed. Replace `async fn` in the trait body — no other
  change needed.
- If the trait is used via `dyn Trait` (`Arc<dyn Mediator>`, `Box<dyn Repo>`),
  the macro **may** still be required depending on the trait's return-type
  shape. Rust 1.75 supports native async fns in traits but does *not* yet
  make them object-safe in all cases without `Pin<Box<...>>` returns or
  `trait_variant`. Verify each case:
  - First try removing the macro and `cargo check`.
  - If the compiler complains about object safety, add `#[trait_variant::make(Send)]`
    or revert and add a `// async-trait still required: dyn dispatch` comment
    so the next reader knows why.

**Why this matters.** Every `#[async_trait]` impl rewrites the function
signature to return `Pin<Box<dyn Future + Send + 'async_trait>>`. That's
invisible to the reader (the macro hides it) but it's an extra concept on
the surface. Removing the macro where it isn't needed reduces the trait
surface from "what does this macro do" to "this is just a trait."

**Caveat.** Don't migrate `fc-router::Mediator` or any other trait actively
used over `dyn` boxes without confirming object safety. The trait_variant
crate or revert path is fine — just leave a one-line comment explaining
why the macro stayed.

**Verify.** `cargo build`, `cargo test`, `cargo clippy -- -D warnings`.

### 2. Split `crates/fc-router/src/mediator.rs` (623 → ~150-200 each)

Suggested split:

- `mediator.rs` — `HttpMediator`, `HttpMediatorConfig`, builder, and the
  `Mediator` trait. Public surface only.
- `mediator/signing.rs` — `sign_webhook`, `SIGNATURE_HEADER`,
  `TIMESTAMP_HEADER`, `MediationPayload`. The webhook-signing wire spec is
  cohesive and worth pulling out.
- `mediator/response.rs` — `MediationResponse` and the status-code dispatch
  (`is_success` / `400` / `401` / `403` / `404` / `429` / `5xx` branches).
- `mediator/retry.rs` — the retry loop + backoff schedule.
- `mediator/inner.rs` — `MediatorInner`, `Arc<Self>` setup, `spawn_sweep_task`.

**Don't change behaviour.** Move definitions; update `mod` declarations;
keep tests green. The signing function MUST remain byte-identical (there's
a golden test vector at `crates/fc-router/tests/golden/`).

**Verify.** `cargo test -p fc-router` green, including the
webhook-signing-parity test.

### 3. Split `crates/fc-platform/src/application/api.rs` (1671 → split per route group)

Tracking-issue first. Suggested splits (verify by reading the file):

- `api/crud.rs` — list, get, create, update, delete
- `api/lifecycle.rs` — activate, deactivate
- `api/provisioning.rs` — service-account attach, OAuth client create
- `api/client_config.rs` — enable/disable for client, list configs

Keep request/response DTOs adjacent to their handlers (consider
`api/dto.rs` only if the DTOs are reused across handler files; otherwise
inline).

**Why.** `api.rs` is the most-edited file in the platform crate and the
hardest to navigate. The TS port has the same problem (1033-line
`applications.ts`) — both are queued for a similar split. See
`HANDOFF.md` in the TypeScript repo, item 3b.

**Verify.** All admin API integration tests pass; OpenAPI generation still
produces the same `openapi.yaml` (utoipa annotations follow the
definitions).

### 4. Audit `Arc<Self>` / `Weak` usage

`rg -tn rust "Weak<|Arc<Self>" crates/` and for each match, document the
cycle being broken:

```rust
// `Weak<MediatorInner>` is held by the sweep task spawned in
// `spawn_sweep_task`. The task lives for the process; without the Weak,
// `MediatorInner` could never be dropped because the task would keep
// itself alive via Arc.
let weak = Arc::downgrade(&inner);
```

Where there's no documentable cycle, downgrade to `Arc<T>` alone (or
remove `Arc` if the type doesn't need shared ownership). The most common
pattern to inspect:

- `Arc<Self>` returned from `new()` for types that aren't actually shared
  across tasks.
- `Weak` references that are upgraded immediately and never re-checked —
  usually a sign that `Arc` would have been fine.

**Output of this work** is one PR with a documentation pass (comments at
each `Weak::*` and `Arc<Self>` site) and zero or more follow-up PRs that
remove genuinely-unneeded indirection. Keep the documentation pass
separate from the removal PR so reviewers can audit the reasoning
without also reviewing behaviour changes.

### 5. Module-level `//!` docs

Audit each crate's `src/lib.rs` and major `mod.rs` files:

```bash
for f in crates/*/src/lib.rs; do
  head -3 "$f" | grep -q "^//!" || echo "MISSING: $f"
done
```

For each crate without a module doc — or with a one-liner — add a
`docs/architecture/<crate>.md`-style summary inline:

- **What this crate is.** One sentence.
- **Mental model.** 3-5 lines on the moving parts and how they relate.
- **Public surface.** The 3-5 most important exports a caller needs.
- **Where to look first.** Pointer to the entry-point file.

Use `pkg/fcsdk/doc.go` in the Go port as a structural reference — it
covers the same ground for the Go SDK and reads top-to-bottom in five
minutes.

### 6. Document tokio task lifecycles

For every `tokio::spawn(...)` site in the codebase, add a comment block
above it answering three questions:

1. **What does this task own?** (resources, channels, state)
2. **When does it exit?** (ctx-cancel? channel close? signal?)
3. **Who joins it?** (parent's `JoinHandle`? abandoned?
   `JoinSet` somewhere?)

Reference shape — the Go port did the same exercise for goroutines; see
`/Users/andrewgraaff/Developer/flowcatalyst-go/internal/router/pool.go`
and `lifecycle.go` for the comment pattern. Translate to tokio task
terminology (replace "channel" with "mpsc::Receiver", "goroutine" with
"task", "ctx.Done()" with "shutdown.recv().await", etc.).

Same exercise for every `select!` macro:

```rust
tokio::select! {
    // Shutdown signal: graceful exit.
    _ = shutdown.recv() => return,
    // Sweep timer: prune idle host pools.
    _ = sweep_interval.tick() => self.sweep().await,
    // Reconfigure signal: reload pool sizing.
    Some(new_cfg) = reconfig_rx.recv() => self.apply_config(new_cfg),
}
```

One short phrase per arm. Turns the macro from a puzzle into prose.

### 7. Per-crate `README.md`

Most crates lack a top-level README. Add a one-page landing for each:

```
crates/fc-router/README.md
  - One-paragraph crate purpose
  - Link to docs/architecture/message-router.md
  - "How to run" (cargo command + required env vars)
  - "Key entry points" (3-5 file paths)
```

Don't duplicate `docs/architecture/<crate>.md` — link to it. The README
is the 30-second landing; the architecture doc is the 30-minute deep dive.

### 8. Cargo workspace lint config

In each crate's `lib.rs`, add:

```rust
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]
```

(For long-lived public-API crates only: `fc-sdk`, `fc-common`.
The platform / router crates don't need this —
they're applications, not libraries.)

This converts "I'll document it later" into a compiler warning. Treat
warnings as errors in CI (`-D warnings`) so it actually sticks.

## What to skip / leave alone

- **Don't restructure `fc-platform`'s use-case pattern.** The
  `UseCase` trait + `UnitOfWork` shape is the wire spec for the SDKs;
  changing it forces work in the Go, TypeScript, and Laravel ports.
- **Don't replace `axum` with another HTTP framework.** It works and
  costs no comprehension.
- **Don't replace `sqlx` with another DB layer.** Same.
- **Don't introduce new async runtimes.** Tokio is the choice.
- **Don't touch wire-format DTO field names.** Any rename has to coordinate
  across Rust + Go + TS + Laravel; out of scope here.
- **Don't migrate from `#[async_trait]` blindly.** Some traits genuinely
  need it for object safety. Check each case (see item 1).
- **Don't merge mega-file splits without verifying `cargo doc` and
  `openapi.yaml` regenerate identically.** The utoipa annotations are
  position-sensitive in some cases.

## Working agreement

- One PR per top-level item. No bundling.
- For items (3), (4), (5): tracking issue listing every file or call site
  before starting the sweep.
- Run `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace -- -D warnings` after every change.
- If a test that was passing before fails after your change, that's a real
  behavioural change — surface it in the PR; don't silently fix the test.
- Pre-commit hooks stay on (`--no-verify` is not acceptable).
- Don't add new dependencies without a tracking issue first.

## Verification checklist (before opening PR)

- [ ] `cargo build --workspace` clean
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo doc --workspace --no-deps` builds with no warnings
- [ ] OpenAPI generation produces an identical (or intentionally-diff'd)
      `openapi.yaml`
- [ ] Webhook-signing golden test vectors still pass (parity with Go and
      TS SDKs)
- [ ] No new direct dependencies in `Cargo.toml` unless justified
- [ ] Behavioural diff explained in PR description if any

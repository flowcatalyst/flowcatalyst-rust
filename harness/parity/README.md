# API parity harness: Go vs Rust

Owner decision #29 (`docs/owner-decisions-2026-09-25.md`): the Java repo's API
parity runner, ported into this workspace. It runs the same scenario files
against Go's `fc-server` and Rust's `fc-server`, each on a byte-identical
clone of one Go-created seed database, then normalises and diffs every
response. Go is the reference for existing behaviour (the Direction section of
the decisions file), so every difference is either a Rust defect, a harness
problem, or a deliberate deviation backed by an owner decision and listed in
`expected-diffs.json`.

The design is Java's (`flowcatalyst-javalin/docs/spec/parity-harness.md`);
this README covers what is specific to the port.

## Prerequisites

- Docker, with the `postgres:18` image (or pass `--pg-image`).
- A Go toolchain able to build `../flowcatalyst-go` (1.27+), **or** prebuilt Go
  `fcdev` + `fc-server` binaries in one directory (`--go-bin-dir`).
- A release build of Rust `fc-server`. By default the harness runs
  `cargo build --release -p fc-server` in this workspace; `--rust-bin-dir`
  skips that and uses `<dir>/fc-server`.

## Usage

```sh
# Everything: build Go into target/go-bin, build Rust (release), run all scenarios.
cargo run -p fc-parity --release --

# One group, prebuilt binaries, custom report directory.
cargo run -p fc-parity --release -- \
  --go-bin-dir target/go-bin \
  --rust-bin-dir target/release \
  --only 'smoke/*' \
  --report /tmp/parity-smoke

# The #[ignore]d test entry point (same pipeline; fails if any step is ERROR).
PARITY_ONLY='roles/*' cargo test -p fc-parity --test parity_run -- --ignored --nocapture
```

| Flag | Env | Default |
|---|---|---|
| `--go-src <dir>` | `PARITY_GO_SRC` | `../flowcatalyst-go` beside this workspace |
| `--go-bin-dir <dir>` | `PARITY_GO_BIN_DIR` | unset: build from `--go-src` |
| `--rust-bin-dir <dir>` | `PARITY_RUST_BIN_DIR` | unset: `cargo build --release -p fc-server` |
| `--only <glob>` | `PARITY_ONLY` | all scenarios. Java `PathMatcher` glob over the path relative to `scenarios/`, e.g. `smoke/*`, `{auth,webauthn}/*` |
| `--report <dir>` | | `target/parity-report` |
| `--scenarios <dir>` | | `harness/parity/scenarios` |
| `--expected-diffs <file>` | | `harness/parity/expected-diffs.json` |
| `--pg-image <image>` | `PARITY_PG_IMAGE` | `postgres:18` |

`PARITY_LOG` sets the harness's own log filter (default `fc_parity=info`).

The report directory gets `report.md` (a per-file OK / ACCEPTED / DIFF /
ERROR table, every non-OK step with its diff, stale allow-list entries,
coverage), `report.json` (the same, with the raw records of every non-OK
step), and the servers' output: `go-seed.log`, `go.log`, `rust.log`.

Exit code is non-zero on any `DIFF`, any `ERROR`, any stale allow-list entry
(full runs only), any false `covers` claim, or coverage below the threshold
(0.0 for now, as in Java).

## How a run works

1. **Go binaries**: `go build -mod=readonly -o target/go-bin/{fcdev,fc-server}`
   from the Go tree. The Go repo is read-only: the harness records its
   `git status --porcelain` before the build and stops if it changed.
2. **Database**: one `postgres:18` container named
   `fc-parity-pg-<pid>-<random>`, published on a random loopback port, removed
   when the run ends (also on Ctrl-C).
3. **Seed** (`src/seed.rs`): `fcdev init --yes` against database `seed`
   (goose migrations, system seed, anchor admin
   `parity-admin@example.com`, client `default`, application `parity`), then
   a Go `fc-server` start-and-stop against it, then
   `CREATE DATABASE parity_go|parity_rust TEMPLATE seed`. `${client.id}`,
   `${app.id}` and `${admin.id}` are read from `seed` before cloning, so they
   are the same on both sides.
4. **Sides** (`src/side.rs`): both `fc-server`s as subprocesses on free ports,
   addressed as `http://localhost:<port>`, platform only, the same RSA key
   (Go `FC_JWT_SIGNING_KEY_PATH`; Rust `FC_JWT_PRIVATE_KEY_PATH` +
   `FC_JWT_PUBLIC_KEY_PATH`), the same `FLOWCATALYST_APP_KEY`,
   `FC_WEBAUTHN_RP_ID=localhost`, default rate limits. Rust boots on a
   database Go created, so every run also exercises the Go-to-Rust
   handover (Rust's migration runner adopting the goose schema, Rust's
   startup seeders).
5. **Scenarios**: each file runs to completion on Go, then on Rust, each with
   its own HTTP client and cookie jar; then every step is normalised and
   diffed (`src/normaliser.rs`, `src/diff.rs`) and checked against the
   allow-list.

### The Go seeder defect, without Java

Java's README notes Go `cb83fd5` could not bootstrap a fresh database: its
seeder wrote `schema_type = 'JSON'`, rejected by migration 051's
`chk_msg_event_type_spec_versions_schema_type`. Java worked around it by
running the Java seeder between two `fcdev init`s. This port recognises the
same failure and instead installs a `BEFORE INSERT` trigger on
`msg_event_type_spec_versions` rewriting `'JSON'` to `'JSON_SCHEMA'`, re-runs
`fcdev init` (idempotent), and drops the trigger. No Java, and every row is
still written by Go. Go HEAD `73a6918` already writes `JSON_SCHEMA`, so the
first `fcdev init` succeeds and the trigger path is dormant; the report's
header says which path the run took.

## Files

| File | What |
|---|---|
| `scenarios/**` | Java's 45 scenario files, verbatim; provenance and re-sync steps in `scenarios/PROVENANCE.md` |
| `surface.json` | Java's hand-listed outside-lockfile surface (same commit as the scenarios) |
| `lockfile-operations.json` | `(method, path, operationId)` of Go's `api/openapi.lock.json`, with the Go commit. Refresh with `cargo run -p fc-parity -- extract-lockfile [--go-src <dir>]` |
| `expected-diffs.json` | The Go-vs-Rust allow-list. Every entry's `ruling` cites an owner decision in `docs/owner-decisions-2026-09-25.md` |

## The allow-list

Same format and matching as Java (`scenario`/`step` may be `"*"`, `scenario`
may end in `*`; `pointer` may start `**/` or end `/**`; `"!go-expect"`
accepts Go missing a step's own `expect.status`). Java's entries are Go-vs-Java
rulings and were **not** carried over; this file starts empty and an entry is
added only when a Rust owner decision makes the difference deliberate. A
difference with no decision behind it stays a `DIFF`.

## Differences from Java's harness

- Rust instead of Java on the second side; `go`/`rust` in every report column.
- PostgreSQL runs in Docker, not embedded (zonky).
- Coverage uses Go's lockfile (256 operations) instead of Java's (253); the
  scenarios' `covers` ids resolve against it. `surface.json` is Java's.
- No `__Host-fc_session` → `fc_session` rename in the normaliser or in
  `cookie:` captures: that was a Java ruling; Rust uses `fc_session`
  (decision #26), so a rename would be a finding.
- A scenario header `Cookie` replaces the jar's cookie, and `""` sends none
  (the scenarios' stated intent); Java appends it.
- Both sides are addressed as `localhost` with RP id `localhost`
  (webauthn-rs refuses an IP RP id); Java used `127.0.0.1`.
- The Go seeder workaround uses a trigger, not the Java seeder.

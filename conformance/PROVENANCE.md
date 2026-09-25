# Provenance

These files are copied unchanged from the Java implementation. They are
vendored on purpose: owner decision #29 (`docs/owner-decisions-2026-09-25.md`)
says the mediation conformance corpus is vendored into this repo with a Rust
runner. That overrides the corpus README's "read, don't copy" advice.

| File | Source |
|---|---|
| `mediation-outcomes.json` | `flowcatalyst-javalin/conformance/mediation-outcomes.json` |
| `README.md` | `flowcatalyst-javalin/conformance/README.md` |
| `go-runner.md` | `flowcatalyst-javalin/conformance/go-runner.md` |

- Source repo: `flowcatalyst-javalin` (sibling checkout `../flowcatalyst-javalin`)
- Source HEAD when copied: `65988b51ce9ebd8d7aa97dfeacc0ecf9d6d52d5d`
- Last source commit to touch the corpus: `f2402b0366e37bf587aa4e8e12648d97982fcc39` (2026-09-02)
- Copied: 2026-09-25
- sha256 of `mediation-outcomes.json`: `c5b18f73aaf436428cfea11ee10055a4991c67eb34bfa4784b0a44a76ec7a398`

## Rules

- **Do not edit these files here.** A change to the corpus is decided in the
  Java repo, then copied again with this note updated. Local edits would make
  the Rust result mean nothing to the other implementations.
- The runner is `crates/fc-router/tests/mediation_conformance_test.rs`. It reads
  `conformance/mediation-outcomes.json` by default. Set `FC_CONFORMANCE_CORPUS`
  to point it at another copy. If a sibling `../flowcatalyst-javalin` checkout
  holds a different corpus, the runner prints a drift notice and still runs
  the vendored copy.
- Where Rust follows the corpus rather than Go, the row is listed in
  `docs/parity/router-deviations-from-go.md`.

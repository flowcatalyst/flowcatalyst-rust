# Provenance of `frontend/`

The platform SPA is the Go platform's production SPA, taken verbatim.

- Source: `flowcatalyst-go`, directory `frontend/`
- Commit: `73a6918e3534c76338cab28eb9a36d6bd4b16c25` (`73a6918`); the last commit touching
  `frontend/` there is `a8ff165` (2026-09-24, "service accounts: a new account starts with no
  application access")
- Taken: 2026-09-25, as the git-tracked files of that directory (`git archive`)
- Left out: Go's server glue, `embed.go` and `handler.go`. Rust embeds `frontend/dist` itself
  (`bin/fc-dev`, rust-embed) or serves it from `FC_STATIC_DIR` (`fc-platform::router::serve_spa`).

The Rust platform is a drop-in replacement for Go, so the SPA calls the Rust platform's API exactly
as it calls Go's. Rust's older fork of this SPA (full-page create/detail views) was replaced
wholesale; the features Rust had that Go lacks are re-added on top in Go's idiom, in the commits that
follow this one. Anything in this directory that is not Go's is listed below.

## Changes on top of Go's SPA

(Kept current by each commit that diverges from Go.)

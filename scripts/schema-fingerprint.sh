#!/usr/bin/env bash
# Schema fingerprint tooling (parity gate G3, docs/parity-plan-2026-10-18.md section 5).
#
#   scripts/schema-fingerprint.sh dump <postgres-url>   print the fingerprint of a database
#   scripts/schema-fingerprint.sh regen-go-fixture      regenerate the committed Go fixture
#   scripts/schema-fingerprint.sh takeover              Rust migrating a Go-migrated database
#
# regen-go-fixture: starts its OWN throwaway Postgres container (postgres:18-alpine, a random
# loopback port, name fc-fingerprint-<pid>), migrates an empty database with Go's goose set
# exactly as Go ships it, writes
#   crates/fc-migrations/tests/fixtures/go-schema-fingerprint.txt (+ .source: the Go commit)
# and removes the container.
#
# takeover: the same container, a Go-migrated database, then `fc-migrate` (the Rust runner,
# whose pre-tracker backfill and probes decide what runs on a database Go migrated), then the
# fingerprint test in FC_FINGERPRINT_FILE mode. It must pass with nothing but the allowed
# Rust-only additions.
#
# Neither command touches any other database. Needs docker and a Go toolchain (goose is built
# through a copy of go.mod/go.sum in a temp dir, so the Go repo is not modified).
# GO_REPO defaults to ../flowcatalyst-go. The fresh-database check needs no script:
#   cargo test -p fc-platform --test it -- rust_schema_matches_go_fingerprint --ignored
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
sql="$here/scripts/schema-fingerprint.sql"
fixture_dir="$here/crates/fc-migrations/tests/fixtures"
fixture="$fixture_dir/go-schema-fingerprint.txt"
go_repo="${GO_REPO:-$here/../flowcatalyst-go}"

dump() { psql "$1" -X -At -v ON_ERROR_STOP=1 -f "$sql"; }

name="" tmp=""
cleanup() {
  [ -z "$name" ] || docker rm -f "$name" >/dev/null 2>&1 || true
  [ -z "$tmp" ] || rm -rf "$tmp"
}

# Starts the scratch container and creates database $1; sets $url (to that database) and $base.
scratch_pg() {
  name="fc-fingerprint-$$"
  tmp="$(mktemp -d)"
  trap cleanup EXIT
  cid="$(docker run -d --rm --name "$name" -e POSTGRES_PASSWORD=scratch -p 127.0.0.1::5432 postgres:18-alpine)"
  port="$(docker port "$cid" 5432/tcp | head -1 | sed 's/.*://')"
  for _ in $(seq 1 60); do
    docker exec "$name" pg_isready -U postgres >/dev/null 2>&1 && break
    sleep 1
  done
  sleep 2
  docker exec "$name" psql -U postgres -c "create database $1" >/dev/null
  base="postgres://postgres:scratch@127.0.0.1:$port"
  url="$base/$1"
}

go_migrate() {
  cp "$go_repo/go.mod" "$go_repo/go.sum" "$tmp/"
  (cd "$go_repo" && GOFLAGS=-mod=mod go run -modfile="$tmp/go.mod" \
      github.com/pressly/goose/v3/cmd/goose -dir internal/migrate/sql postgres "$1?sslmode=disable" up)
}

case "${1:-}" in
  dump)
    dump "${2:?postgres url}"
    ;;
  regen-go-fixture)
    scratch_pg go_fresh
    go_migrate "$url"
    mkdir -p "$fixture_dir"
    dump "$url" >"$fixture"
    printf 'Go %s, migrations through %s\n' "$(git -C "$go_repo" rev-parse --short HEAD)" \
      "$(ls "$go_repo"/internal/migrate/sql/*.sql | sed 's/.*\///' | sort | tail -1)" >"$fixture.source"
    echo "wrote $fixture ($(wc -l <"$fixture") lines); $(cat "$fixture.source")"
    ;;
  takeover)
    scratch_pg go_takeover
    go_migrate "$url"
    (cd "$here" && cargo run -q -p fc-migrations --bin fc-migrate -- "$url")
    dump "$url" >"$tmp/takeover.txt"
    (cd "$here" && FC_FINGERPRINT_FILE="$tmp/takeover.txt" cargo test -p fc-platform --test it \
        -- rust_schema_matches_go_fingerprint --ignored)
    ;;
  *)
    sed -n '2,20p' "$0"
    exit 2
    ;;
esac

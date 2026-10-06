#!/usr/bin/env bash
# The checked queries' metadata: `.sqlx/` at the repository root.
#
#   scripts/sqlx-prepare.sh           regenerate .sqlx/ (run after adding or
#                                     changing a `sqlx::query!` family macro,
#                                     or a migration; commit the result)
#   scripts/sqlx-prepare.sh --check   fail if .sqlx/ is stale or incomplete
#                                     (CI's `database-tests` job runs this)
#
# Ordinary builds never need a database: with DATABASE_URL unset the macros
# read .sqlx/ (offline mode). Only this script needs one, because only it
# asks Postgres to describe each query.
#
# The schema the queries are checked against is the one a deployment has:
# `fc-migrate` (crates/fc-migrations) runs the application's own migration
# runner (the same SQL, tracker and drift checks `fc-server` runs at start-up)
# on an empty database. The platform's code migration (036, the scheduled-job
# cron rewrite) rewrites data only, so it is not needed for the schema. The
# runner is its own crate with no checked queries so it can be built before
# .sqlx/ is up to date.
#
# Where the database comes from (first that applies):
#   FC_SQLX_DATABASE_URL=<url>  an existing EMPTY (or already migrated)
#                               database, used as is and left running (CI)
#   FC_TEST_PG_BIN=<bin dir>    a private cluster from local PostgreSQL
#                               binaries (initdb + postgres; 15+), stopped on exit
#   otherwise                   a throwaway `postgres:18-alpine` container
#                               (needs Docker), removed on exit
set -euo pipefail

cd "$(dirname "$0")/.."
export FC_SKIP_FRONTEND_BUILD="${FC_SKIP_FRONTEND_BUILD:-1}"

mode=prepare
case "${1:-}" in
    "") ;;
    --check) mode=check ;;
    *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

if ! cargo sqlx --version >/dev/null 2>&1; then
    echo "error: cargo-sqlx is not installed:" >&2
    echo "  cargo install sqlx-cli --version =0.8.6 --locked --no-default-features --features rustls,postgres" >&2
    exit 1
fi

workdir="$(mktemp -d)"
cleanup_cmds=()
cleanup() {
    local i
    for ((i = ${#cleanup_cmds[@]} - 1; i >= 0; i--)); do
        eval "${cleanup_cmds[$i]}" || true
    done
    rm -rf "$workdir"
}
trap cleanup EXIT

free_port() {
    python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'
}

# Waits until something listens on 127.0.0.1:$1.
wait_for_port() {
    local port="$1" i
    for ((i = 0; i < 300; i++)); do
        if python3 -c "import socket,sys; s=socket.socket(); s.settimeout(0.2); sys.exit(0 if s.connect_ex(('127.0.0.1', $port)) == 0 else 1)"; then
            return 0
        fi
        sleep 0.2
    done
    echo "error: PostgreSQL did not start on port $port" >&2
    return 1
}

if [ -n "${FC_SQLX_DATABASE_URL:-}" ]; then
    url="$FC_SQLX_DATABASE_URL"
elif [ -n "${FC_TEST_PG_BIN:-}" ]; then
    bin="$FC_TEST_PG_BIN"
    port="$(free_port)"
    "$bin/initdb" -D "$workdir/data" -U sqlx --auth=trust -E UTF8 --no-locale --no-sync >"$workdir/initdb.log" 2>&1 \
        || { cat "$workdir/initdb.log" >&2; exit 1; }
    "$bin/pg_ctl" -D "$workdir/data" -l "$workdir/postgres.log" -w \
        -o "-p $port -k $workdir -c listen_addresses=127.0.0.1 -c fsync=off" start >/dev/null \
        || { cat "$workdir/postgres.log" >&2; exit 1; }
    cleanup_cmds+=("\"$bin/pg_ctl\" -D \"$workdir/data\" -m fast -w stop >/dev/null")
    wait_for_port "$port"
    url="postgresql://sqlx@127.0.0.1:$port/postgres"
else
    port="$(free_port)"
    cid="$(docker run -d --rm -e POSTGRES_PASSWORD=sqlx -p "127.0.0.1:$port:5432" postgres:18-alpine)"
    cleanup_cmds+=("docker stop \"$cid\" >/dev/null")
    wait_for_port "$port"
    url="postgresql://postgres:sqlx@127.0.0.1:$port/postgres"
    # The server restarts once after initdb; give it a moment to settle.
    sleep 3
fi

echo "== migrating the schema ($url)"
env -u DATABASE_URL cargo run --quiet -p fc-migrations --bin fc-migrate -- "$url"

echo "== cargo sqlx prepare ($mode)"
args=(--workspace)
[ "$mode" = check ] && args+=(--check)
# --all-targets: queries in tests and benches are checked too.
DATABASE_URL="$url" cargo sqlx prepare "${args[@]}" -- --all-targets

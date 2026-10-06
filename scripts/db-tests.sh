#!/usr/bin/env bash
# The database-backed tests: every `#[ignore = "requires Docker"]` /
# `"needs Docker"` test that needs nothing but a container runtime or a
# PostgreSQL (see "Where an integration test's PostgreSQL comes from" in
# crates/fc-platform/tests/it/support/db.rs). CI runs this on every pull
# request and push (the `database-tests` job); run it the same way locally.
#
# Left out on purpose (cargo cannot filter by ignore reason, so each is
# excluded by what selects the tests below):
#   * fc-queue's SQS tests (LocalStack on :4566) and NATS tests (a NATS server);
#   * the measurement and benchmark tests (fc-router throughput_bench,
#     fc-fnhost-core wasm_neighbour / wasm_fuel, fc-fnhost-js js_density);
#   * bin/fc-server's JVM function-host test (Java 25 and Maven) and the
#     harness/ Go-vs-Rust runs (a Go toolchain and release binaries).
#
# Where the databases come from: with neither variable set, each test starts a
# container (testcontainers; needs Docker). `FC_TEST_PG_BIN=<postgres bin dir>`
# or `FC_TEST_DATABASE_URL=<server url>` makes the tests that go through
# `support::start_db` use that server instead (a database each); tests that
# start their own container (Redis, MySQL, LocalStack, a few Postgres ones)
# still need Docker.
set -euo pipefail

cd "$(dirname "$0")/.."
export FC_SKIP_FRONTEND_BUILD="${FC_SKIP_FRONTEND_BUILD:-1}"

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

run() {
    echo "::group::cargo test $*"
    cargo test "$@" 2>&1 | tee -a "$log"
    echo "::endgroup::"
}

# fc-platform: every ignored test in these two binaries is Docker-backed.
run -p fc-platform --test it --test function_host_e2e_test -- --ignored
# fc-fnhost-core: db_postgres and wasm_db need Docker; the other two ignored
# tests (wasm_neighbour, wasm_fuel) are measurements.
run -p fc-fnhost-core --test it -- --ignored --skip wasm_neighbour:: --skip wasm_fuel::
# fc-queue: the Postgres queue only (its tests are behind the `postgres` feature).
run -p fc-queue --features postgres --test it postgres_integration_tests:: -- --ignored
# fc-outbox: the Postgres and MySQL repositories (unit tests in src/).
run -p fc-outbox --lib -- --ignored
# fc-standby: leader election against Redis.
run -p fc-standby --test leader_election_tests -- --ignored
# The binaries: the outbox processor against MySQL, fc-server booting with the
# production task definition's environment.
run -p fc-outbox-processor --test mysql_backend_test -- --ignored
run -p fc-server --test it prod_env_boot_test:: -- --ignored

# A filter that matches nothing passes silently; make that fail.
passed=$(awk '/^test result: ok\./ { sum += $4 } END { print sum + 0 }' "$log")
min="${FC_DB_TESTS_MIN:-350}"
echo "database tests passed: $passed (floor $min)"
if [ "$passed" -lt "$min" ]; then
    echo "error: fewer database tests ran than expected ($passed < $min); a selection above matches nothing" >&2
    exit 1
fi

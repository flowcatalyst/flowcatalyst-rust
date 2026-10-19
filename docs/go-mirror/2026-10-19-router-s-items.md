# Go router S-items mirrored (P3 router half, 2026-10-19)

Five small Go router changes from 2026-10-02 to 10-09, ported with Go's tests first. Nothing is pushed. The Go repo
was only read. The Java hand-off notes in `../flowcatalyst-javalin/docs/go-mirror` have nothing for these five.

| Item | Go | Rust |
|---|---|---|
| 1. Poll-loop acks are urgent | `c34f230a` | `fc-queue/src/sqs.rs` (malformed ack in `poll`), `fc-router/src/manager/routing.rs` (`poll_loop_ack`: external-requeue duplicate, strict-routing reject) |
| 2. HTTP/2 PING keepalive | `8d1abaf3` | `fc-router/src/mediator.rs` (`http2_ping_interval/timeout`, `with_http2_health_check`), `mediator/inner.rs`, `bootstrap/{env,runtime}.rs` |
| 3. Missing SQS queue is not a fault | `c52d1150` | `QueueError::QueueMissing`, `RouterError::QueueMissing`, `SqsQueueConsumer::queue_is_missing`, `manager/{reconcile,consumers}.rs`, `config_sync.rs` |
| 4. NATS pull expiry | `c6b534d6` | `fc-queue/src/nats.rs` (`pull_expiry_ms`, `DEFAULT_PULL_EXPIRY`, `pull_timing`) |
| 5. Dashboard modal | `5cbe502e` | `fc-router/resources/dashboard.html` (Rust embeds its own copy of the router dashboard, not Go's SPA, so the same 4 class edits apply) |

## 1. Urgent poll-loop acks

Rust already acked ordered-group successes urgently (`ack_urgent`). Go's three poll-loop acks are the malformed
message inside the SQS `Poll`, an external-requeue duplicate and a strict-routing reject (both in `route`). All three
are now urgent. In Rust the first is `SqsQueueConsumer::poll` calling `ack_urgent`; the other two go through the new
`poll_loop_ack` (an `ack_urgent` bounded by `BROKER_OP_TIMEOUT`). Other backends' `ack_urgent` defaults to `ack`.
Not changed, as in Go: the "already processed, delete on redelivery" ack in `route_batch_inner` (also on the poll
loop) stays a plain `ack` (not in Go's commit; I found no Go equivalent path).

Tests: `fc-queue` `sqs::poll_ack_urgent_tests::malformed_message_ack_inside_poll_does_not_wait_out_the_linger`
(a hand-rolled awsJson1.0 server; poll must return well inside the 5 s linger) and `fc-router`
`manager::routing::poll_loop_ack_tests` (a consumer recording `ack` vs `ack_urgent`). Mutation-checked: reverting
the SQS call makes the first fail at 5.005 s; making `poll_loop_ack` call `ack` fails both manager tests.

## 2. HTTP/2 PING

`reqwest` `http2_keep_alive_interval` / `_timeout` / `_while_idle(true)` on every deployed-mode (HTTP/2) client,
both `http://` (h2c prior knowledge) and `https://` (ALPN). Defaults 30 s / 15 s, as Go. Env:
`FC_ROUTER_HTTP2_READ_IDLE_TIMEOUT_SECONDS`, `FC_ROUTER_HTTP2_PING_TIMEOUT_SECONDS` (`0` keeps the default); listed in
`docs/parity/router-env-vs-go.md`. `while_idle(true)` matches Go's x/net behaviour of pinging an idle connection.
Test: `mediator::inner::h2_ping_tests` against a raw-TCP h2c peer that completes the handshake and then goes silent:
the delivery fails in about 0.5 s with the check on (10 s deadline), and runs to its own deadline with the default
30 s (control, zero pings). Mutation-checked: removing `http2_keep_alive_interval` makes it run to 10.003 s.
Differences: Go's second subtest (TLS ALPN h2) is not ported; the TLS path sets the same three builder options but
is untested here. Go uses its dev mediator config when `FLOWCATALYST_DEV_MODE` is set; Rust's runtime always builds
`HttpMediatorConfig::production()` (existing difference), and dev (HTTP/1.1) clients have no PING to send.

## 3. Missing SQS queue

The factory asks `GetQueueAttributes(QueueArn)` and returns `QueueMissing` only when SQS answers
`AWS.SimpleQueueService.NonExistentQueue` / `QueueDoesNotExist`; any other failure of the check builds the consumer
as before (so a transient AWS error cannot stop consumption). `QueueManager` keeps `missing_queues`:
`sync_queue_consumers` (now via `start_consumers`) records a missing queue, logs one INFO per streak, raises no
warning and does not fail the reload. A queue that leaves the config or changes is forgotten. `recheck_missing_queues`
runs when `ConfigSync::apply_latest` finds the config unchanged. A poll that reports `QueueMissing` retires the
consumer (`detach_missing_consumer`, in-flight work still resolves it) and re-tracks the queue; the stall watchdog
and `restart_consumer` do the same when a rebuild finds the queue gone. Tests: `manager::reconcile::missing_queue_tests`
(three, Go's two plus config-removal) and `sqs::poll_ack_urgent_tests::a_missing_queue_is_its_own_poll_outcome`.
Mutation-checked: treating missing as a failure, not detaching on a poll-time miss, and a no-match error code each
fail the matching test. Difference: Go still counts a poll-time miss in `pollErrors`; Rust does not count it (it is
not an error here).

## 4. NATS pull expiry

`pull-expiry-ms` on the `nats://` URI, default 3000 ms (was the client's 30 s), applied to the initial subscribe and
every resubscribe through one function. From 1 s up the heartbeat is a third of the expiry (server limit: half). Below
1 s (including 0) Rust uses the old 30 s / 15 s pair, like Go's "leave the library's derivation". Go leaves the
heartbeat unset for 1.0 to 1.5 s; Rust always sets a third. Tests: URI default/override and the timing table, unit
only. The Docker-backed `nats_integration_tests` pass with the new expiry (run one at a time: parallel container
starts hit "connection refused", a pre-existing race). Not verified against a live pull request that the server
sees `expires=3s`, and Go's own caveat holds: this bounds the freeze, it does not remove it.

## 5. Dashboard modal

`crates/fc-router/resources/dashboard.html` is Rust's own file (the router's embedded dashboard), so the change is
the same four class edits Go made: `max-h-[85vh] flex flex-col` on the panel, `shrink-0` on header and footer,
`overflow-y-auto min-h-0` on the body. Not visually checked in a browser.

## Not done

Go's 09-22 HOL reservation schedule (Rust defers by a fixed `CAPACITY_DEFER_SECONDS`) is a separate item in the
parity plan and is still pending.

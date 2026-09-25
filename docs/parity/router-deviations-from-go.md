# Router: where Rust deviates from Go on mediation outcomes

Owner decision #29 (`docs/owner-decisions-2026-09-25.md`): the mediation
conformance corpus is vendored and run by a Rust runner. Where the corpus
calls Go's behaviour a defect, the corpus wins, and each such row is listed
here as a deliberate deviation from Go.

- Corpus: `conformance/mediation-outcomes.json`, copied from
  `flowcatalyst-javalin@65988b51` (last corpus change `f2402b03`). See
  `conformance/PROVENANCE.md`.
- Go: `flowcatalyst-go@73a6918`, read only. Nothing in Go was built or run
  for this document. Every Go claim below comes from reading the source, and
  cites file and line.
- Rust runner: `crates/fc-router/tests/mediation_conformance_test.rs`.
  Result: 28 pass, 1 ruled (`unsupported-mediation-type`), 0 fail.

## 1. Corpus rows where Go differs from the corpus

**None at Go `73a6918`.**

Go's own runner (`internal/router/mediation_conformance_test.go:523-532`)
only logs a mismatch on a row marked `correct: java`, and never fails on one.
A green Go run therefore does not prove those rows pass. Each was checked by
reading the source instead. Every row the corpus records against Go has since
been fixed in Go:

| Case | What the corpus recorded against Go | Go now | Fixed in |
|---|---|---|---|
| `success-carries-real-2xx-status` | `common.Success()` hard-coded status 200 | `common.Success(status)` keeps the real status (`internal/router/mediator.go:505`, `internal/common/mediation.go:68`) | `2468140` |
| `unfollowed-3xx-is-permanent` | 3xx fell to the default arm: `ErrorProcess(30)`, status 0, retried for ever | `ErrorConfig(status)` plus an ERROR warning (`mediator.go:507-518`) | `2468140` |
| `config-error-other-4xx` | the generic 4xx arm only logged, with no warning | warns ERROR (`mediator.go:563-570`) | `2468140` |
| `malformed-target-url` | silent `ErrorConfig`, and a breaker success for a call never made | warns ERROR (`mediator.go:445-451`); `PreFlight` skips the breaker (`mediator.go:341-346`) | `2468140`, `2e2e466` |
| `unsupported-mediation-type` | same as above | warns ERROR (`mediator.go:403-409`); pre-flight, so no breaker record | `2468140`, `2e2e466` |
| `unexpected-status-1xx` | disputed: `ErrorProcess(0, 30)` or `ErrorConnection` | By reading: `net/http` waits for a final response after a non-101 1xx and hits EOF, so `ErrorConnection` (`mediator.go:458-467`). The fate (return to broker, 30s, breaker failure) is the same either way. | n/a (`correct: both`) |

If Go regresses on any of these rows, Rust keeps the corpus behaviour. That
is the standing deviation decision #29 describes.

## 2. A row Rust cannot run as written

| Case | Corpus | Rust | Go |
|---|---|---|---|
| `unsupported-mediation-type` | reaches the mediator; `ErrorConfig`, an ERROR warning, UNDELIVERABLE, no breaker record | Owner rulings X-06 and X-10 make `MediationType` a closed enum with no catch-all, so a message with `"mediationType": "SQS"` fails to parse. The consumer ACK-deletes it as malformed (for example `fc-queue/src/sqs.rs`, "Failed to parse SQS message"). It is logged, **but no warning is raised**. The message's fate is the same (UNDELIVERABLE, no call, no breaker). | warns at `mediator.go:407` |

The runner checks the parse-boundary refusal and reports the case `RULED`.
**Open, for the queue/consumer owner:** the corpus rule is that a permanent
ACK-drop must warn. The malformed-message ACK path in the consumers should
raise a CONFIGURATION/ERROR warning, as Go's mediator does for this case.

## 3. How the runner reads `disposition` (no deviation)

`deferred-ack-false-with-delay` expects `RETRY_IN_PLACE`. Owner ruling R1
(2026-09-17; Go `d879b23`, `pool.go:1446-1453`) instead sends a deferral that
names a delay straight back to a broker that can hold it. Both reference
runners assert the corpus's `disposition` as the outcome's own
classification, not R1's hand-back:

- Java asserts `outcome.disposition()`.
- Go calls `DispositionOf(outcome, 0, NEXT_ON_ERROR, false)`
  (`mediation_conformance_test.go:233-249`).

The Rust runner does the same: its broker answers
`honours_delayed_return() == false`. A separate test,
`deferral_naming_a_delay_goes_back_to_a_broker_that_honours_it`, pins R1.
Rust implements R1 as Go does.

## 4. Deliberate Rust differences from Go in the same delivery path

These are not corpus rows. They are listed so nobody mistakes them for
drift.

| # | Behaviour | Go | Rust | Why |
|---|---|---|---|---|
| D1 | Nack delay when a group is released (unreachable or unavailable target, open breaker) | Head and siblings are nacked with **no delay**. `DispositionOf` leaves `RetryAfter` at zero, `nackDelay(0)` is `nil` (`pool.go:916`, `1024`, `1673`), and SQS makes the message visible at once. | The head gets the outcome's delay (30s for `ErrorProcess`/`ErrorConnection`, 5s for `CircuitOpen`). Siblings get 10s. | A zero delay during an outage becomes a hot redelivery loop. Each loop spends an SQS receive count toward the DLQ, and on an open breaker it returns at once to the same open breaker. Kept as Rust already did it. The owner should confirm. |
| D2 | BLOCK_ON_ERROR, siblings behind a terminally failed head | ACKed, and reported to the platform through the settled hook (`pool.go:939`, `ackBuffered`); ACKed even with no platform URL (the platform reaper is the backstop) | **With `FC_ROUTER_PLATFORM_URL` set: as Go** — ACKed, and the jobs carrying a dispatch token reported to `POST /api/dispatch/settled` (`crates/fc-router/src/settled.rs`). Without it, the whole buffer is taken and **nacked** | A router with no platform URL has nothing telling the platform the rows were dropped, so Rust keeps them on the broker there. Every deployed router sets the URL (`inhance/iac/compute/fc-router.ts`), so production behaves as Go. |
| D3 | A panic during delivery | Recovered in `processOne` and retried in place after 10s, within the budget | The task ends. `WorkerGuard`/`DrainGuard` give back the queue slots, the worker count and the `mediating` entry. The message callbacks' `Drop` nacks the in-hand message and every abandoned buffered one. | No `catch_unwind` around the mediator. Bookkeeping no longer leaks, and the messages go back to the broker. |
| D4 | SQS nack/defer delay clamp | Clamped to what remains of 12h since this consumer first received the message (`internal/queue/sqs/sqs.go:347-376`) | Clamped to a flat 43200s (`fc-queue/src/sqs.rs`, `visibility_for`) | Tracking the time of each receive belongs to the consumer owner. The flat clamp already stops the rejected-call fallback. |
| D5 | `honours_delayed_return` per broker | SQS and Postgres answer true. NATS answers **false** (`internal/queue/nats/nats.go:795`). | Defaults to true for every broker (`fc_common::MessageCallback`) | The router callback lives in `manager/routing.rs`, which another area owns. **Open:** NATS-sourced callbacks should answer false, as Go's do, so a named deferral is kept in place and doesn't spend a NATS redelivery. |

## 5. Open question the corpus doesn't settle

The corpus separates `REJECTED` (the app ran and failed: R-57 5xx) from
`UNDELIVERABLE` (4xx, 3xx, 501, pre-flight). A broker can't tell them apart:
both are acknowledged away. The only behavioural difference is in Java's
ordered groups. Under BLOCK_ON_ERROR, Java blocks the group only on
`REJECTED` and moves past an `UNDELIVERABLE` head
(`OrderedGroups.onHeadFailure`). Go (`pool.go:1390-1398`) blocks on every
`ErrorConfig`, and so does Rust. The corpus doesn't pin the dispatch mode, so
this is not a corpus deviation. Both runners fold the two labels together.
The owner should rule whether a 4xx head should block a BLOCK_ON_ERROR group.

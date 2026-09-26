# Delivery parity run 5 (Go vs Rust)

Run `20260926-083622-r6dv4` · Go `73a6918` · Rust `428f3198` (branch `feat/decisions-33-38`: the four API-area merges,
which touched subscriptions, dispatch pools, events, dispatch-job reads and IAM, plus decisions #33–#38) · full run,
both sides, Rust on a Go-migrated database (the cutover path), debug builds. Harness: `harness/delivery`.

Command: `./target/debug/fc-delivery-harness --report "$PWD/target/delivery-run-5"` after
`cargo build -p fc-router-bin -p fc-outbox-processor -p fc-delivery-harness -p fc-server`.

**Result: 16 PASS, 1 DIFF (Go side, intermittent), exit 1; no stale `expected-diffs.json` entry.** Rust met every
invariant in every scenario and delivered every group in order. Run 4 was 16 PASS, 1 ACCEPTED. A rerun of the one
DIFF scenario alone (`--only platform-down`) passed on both sides.

## Verdicts

| Scenario | Run 4 | Run 5 | Go | Rust |
|---|---|---|---|---|
| plain-events | PASS | **PASS** | 20/20, 20 deliveries | 20/20, 20 deliveries |
| dispatch-jobs-api | PASS | **PASS** | 10/10 | 10/10 |
| ack-false-no-delay | PASS | **PASS** | 5/5, 10 deliveries | 5/5, 10 deliveries |
| ack-false-delay | PASS | **PASS** | 5/5, 10 deliveries | 5/5, 10 deliveries |
| rate-limited-429 | PASS | **PASS** | 5/5, 15 deliveries | 5/5, 15 deliveries |
| server-error-then-success | PASS | **PASS** | 5/5, 15 deliveries | 5/5, 15 deliveries |
| permanent-errors | PASS | **PASS** | 0/6, 10 deliveries | 0/6, 10 deliveries |
| connection-refused | PASS | **PASS** | 0/3 | 0/3 |
| block-on-error-group | PASS | **PASS** | 6/6, 8 deliveries | 6/6, 8 deliveries |
| next-on-error-group | PASS | **PASS** | 6/6, 8 deliveries (4 overtakes by design) | 6/6, 8 deliveries (4 overtakes by design) |
| ordered-group-20 | PASS | **PASS** | 20/20 | 20/20 |
| burst-pool-capacity | PASS | **PASS** | 60/60 | 60/60 |
| router-restart | PASS | **PASS** | 40/40, in order | 40/40, in order |
| worker-restart | ACCEPTED | **PASS** | 40/40, in order | 40/40, in order |
| platform-down | PASS | **DIFF** | 20/20, g1 `1-5,7-10,6` (4 FIFO overtakes) | 20/20, in order |
| outbox-events | PASS | **PASS** | 10/10, outbox empty | 10/10, outbox empty |
| slow-target-timeout | PASS | **PASS** | 3/3, 6 deliveries | 3/3, 6 deliveries |

## The differences from run 4

### worker-restart: ACCEPTED → PASS

Go's intermittent defect (it commits a claim QUEUED before publishing it, so a SIGKILL during its ~1 s serial
publish strands the rest for 75 minutes; `docs/parity/delivery-run-4.md`) did not show this time: the kill landed
outside Go's window. The `#31` entries for it are marked `intermittent`, so they are not stale. Rust delivered all
40 in order, as in run 4.

### platform-down: PASS → DIFF, on Go's side

Go delivered group g1's seq 6 after 7–10; Rust delivered both groups in order, nothing lost, nothing twice.

- **Mechanism (Go router log).** Seq 6's `/api/dispatch/process` call failed while the platform was down
  (`connect: connection refused`, retried in-process at 1 s and 2 s), and Go released the group with head and
  siblings visible at once (no nack delay; `docs/parity/router-deviations-from-go.md` D1). LocalStack's long poll
  does not hold a FIFO group while its head is in flight, so 7–10 were received and delivered first. On SQS FIFO the
  head's in-flight lock would keep the order; the run-4 notes record the same LocalStack fidelity limit.
- **Intermittent.** It depends on whether the platform stop lands while a group head is between attempts. Run 4 and
  the rerun of this scenario alone (`target/delivery-run-5-platform-down`) passed on both sides.
- **Not a Rust regression.** Rust holds siblings no shorter than their head (the run-4 fix), so it keeps the order on
  either broker. No `expected-diffs.json` entry was added: #31 covers Go losing, duplicating or stranding messages,
  and this reordering is a LocalStack effect Go would not show on SQS.
- **Settle time.** Rust settled in 37 s against Go's 9.5 s, in run 5 and in the rerun alike: Rust nacks a head whose
  `/process` call failed with the outcome's 30 s delay (deviation D1, awaiting owner confirmation), Go with none. The
  harness does not compare settle time here; it is the price of not hot-looping redeliveries during an outage.

## Regression check for the API-area merges

The merges changed subscription create/update/sync, dispatch-pool status and validation, event reads, dispatch-job
reads and IAM (service accounts, OAuth clients, principals). Every scenario that drives them (plain-events, the
ordered and error-group scenarios, burst-pool-capacity, dispatch-jobs-api, outbox-events, the three restart
scenarios) passed with the same counts and orders as run 4. The harness's provisioning (the super-admin
`harness-api` service account, `harness-router`, the router-config document) worked unchanged on both sides.

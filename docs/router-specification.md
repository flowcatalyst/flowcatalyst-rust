# FlowCatalyst Message Router — Specification

## 0. Status

This document is **the implementation-neutral contract for the FlowCatalyst message
router appliance**: what any router implementation must do, independent of language or
codebase. It is not a description of any one binary's internals. An implementation is
**conformant** when:

1. it passes the executable conformance corpus
   (`flowcatalyst-javalin/conformance/mediation-outcomes.json`, run through a
   language-neutral runner per `conformance/README.md`), and
2. every **MUST** in this document holds.

**SHOULD** marks a strong default that a deployment may override for a stated reason;
**MAY** marks a genuine option. Plain prose is rationale, not contract — where rationale
cites a ledger ID (`[A-01]`, `[R-57]`, `[X-04]`, …), the ledger entry is the
authority and this text is a summary of it, not a substitute.

**Sources, in authority order:**

1. `flowcatalyst-rust/docs/owner-questions.md` — the ruling ledger. Any ruled item
   (Part A, ruled Part B entries, the X-series, the 2026-09-02 addendum) overrides
   every other source, including this document if the two ever drift.
2. `flowcatalyst-javalin/conformance/mediation-outcomes.json` +
   `conformance/README.md` — the executable outcome contract, normative for mediation
   response handling. This document's §4 is a guide to that corpus, not a replacement
   for running it.
3. `flowcatalyst-go/docs/router-architecture.md` — accurate description of the
   reference implementation's current, ruling-compliant behaviour.
4. `flowcatalyst-javalin/docs/spec/router.md` — the deepest behavioural detail
   available (constants, state tables, edge cases), but **pre-ruling in numerous
   places**: it was extracted from an older Go commit and records many of today's
   ruled behaviours as open questions or "deliberate Java deviations." Every fact
   drawn from it here has been checked against the ledger and, where useful,
   spot-verified against the current Go source (`flowcatalyst-go/internal/router`,
   `internal/common`), which has continued to absorb rulings after that spec was
   written.

**Two items are specified here but knowingly unshipped in every implementation as of
this writing:**

- **The `BLOCK_ON_ERROR` ACK-the-siblings branch [A-01].** §3.2 specifies it as the
  contract. An implementation **MUST NOT** enable it until the platform side exists:
  the ACKed siblings must become visibly pending (marked platform-side), the failed
  head must be marked `FAILED` for human review, and the review verbs (*ignore* /
  *complete* / *resend*) and their re-queue path must exist and be correct — without
  that, an ACKed sibling is silent data loss with no recovery path. Until then, an
  implementation **MUST** keep releasing (NACKing) the untried siblings back to the
  broker instead, exactly as the pre-ruling behaviour did. `flowcatalyst-go`
  demonstrates the shippable shape: the router-side ACK and the platform-facing
  settled-report hook (`POST /api/dispatch/settled`) are both built, but the hook is
  wired as fire-and-forget with its own bounded timeout, and a platform-side reaper
  is the correctness backstop for a router that dies between the ACK and the report
  (see §5.4). Building the router half without the platform half is exactly the
  configuration this MUST forbids.
- **The malformed-routing ACK+notice gate [R-13, R-16].** §2.3 and §9's environment
  table specify a `FC_ROUTER_STRICT_ROUTING`-shaped switch. It **defaults off**
  pending the owner's confirmation that every production producer actually publishes
  `poolCode`, `dispatchMode`, and (for ordered modes) `messageGroupId` on every
  message — flipping it on is an operational decision, not a code change, and turning
  it on prematurely would silently delete messages from producers nobody has
  verified yet.

Status as of the ledger's 2026-09-03 addendum. Nothing in `crates/fc-router` was read
for this document beyond the spot-verifications noted above (it is mid-refactor); the
Go router (`internal/router`) was used for spot-verification because it is the
oldest, most-rulings-absorbed implementation and its source is stable for that
purpose. This document is not a description of Go, however — where the two disagree,
the ledger's ruling is what is written here, and Go's compliance or non-compliance is
noted only where it is informative.

---

## 1. Topology & ownership

### 1.1 The router is a poller, not a server

The router does not expose a delivery API that anything calls to hand it a message.
It **polls** one or more message-queue backends, and for every message it decides
whether, when, and to which HTTP target to deliver it. Nothing pushes work into the
router; the router pulls.

It is a **pull → push relay**: poll a queue, decide whether it may deliver now,
deliver as a signed HTTP POST to the URL the message names, honouring per-pool
concurrency and rate limits, per-endpoint circuit breakers, and per-group FIFO
ordering when the message asks for it. It never inspects or forwards the message
*payload* — the POST body it sends is exactly `{"messageId":"<id>"}` (§2.5); anything
the target needs beyond the id is fetched by the target itself, using the id as a
pointer back into the system of record. It owns no durable state: everything the
router holds in memory (in-flight tracker, group buffers, warnings, breakers,
metrics) is lost on restart, and this is fine because the broker is the durable copy.

It is **not**:

- a queue (it owns no durable state — see above);
- a job scheduler (something else decides *what* to publish and *when*; the router
  only decides *whether it may deliver what it was handed, right now*);
- the webhook subscriber the message ultimately reaches, when a mediation target is
  itself a relay (e.g. the platform's own dispatch-processing endpoint, which loads a
  job and delivers to the real subscriber URL — see §9); the router does not know
  about a subscriber URL one hop past its own mediation target;
- an inbound-webhook verifier — it only *signs* outbound deliveries (§4.1).

### 1.2 Component responsibilities

| Component | Responsibility | Cardinality |
|---|---|---|
| **Manager** | Owns consumers, pools, and publishers. Polls each queue, deduplicates at route time, resolves the target pool per message, submits. | one per process |
| **Pool** | A **passive** worker set — it never touches a broker. Enforces per-pool concurrency and rate limit, holds the per-group FIFO buffer, and turns a mediation outcome into exactly one broker action. | one per pool code |
| **Mediator** | Owns the circuit breakers, the (single, named) retry policy, HTTP delivery, request signing, and HTTP-status→outcome classification. Breaker accounting is centralised here — exactly once per delivery attempt, nowhere else. | one per process, shared by every pool |
| **In-flight tracker** | Duplicate suppression, receipt-handle freshness, and the input to stall detection. | one per process |
| **Config source** | Polls the router's configuration endpoint(s) for pool/queue definitions; merges and reconciles. | one per process |

**The pool/mediator boundary is a load-bearing design line: the mediator decides what
a response *means*; the pool decides what that meaning does to the broker.** The
mediator classifies an HTTP response (or its absence) into one of a small set of
named outcomes (§4.2) and records the breaker consequence. The pool never inspects
raw HTTP status codes — it only ever sees the mediator's outcome, and its whole
resolution logic is a table lookup from outcome to broker action (§4.4). This
separation is what makes the outcome contract testable independent of any queue
backend: the conformance corpus (§11) drives the mediator directly and asserts the
outcome, without a real broker in the loop.

### 1.3 Pools are passive; ack/nack resolves to the source consumer

**A pool does not own a queue.** The manager polls every configured queue and routes
each message to the pool named by its `poolCode`, so a single pool routinely serves
messages from many queues. This means acknowledgement cannot simply "go back to the
pool" — the pool has no broker handle. Instead:

- Every polled message carries a `QueueIdentifier` naming the consumer it came from.
- Every ack/nack action **MUST** be resolved back to that source consumer (never to
  "whichever consumer is currently registered under that name," except the one
  documented exception in §5.3) and issued using the receipt handle for **this
  specific delivery** of the message.
- If the source consumer is no longer resolvable (deregistered with nothing left
  referencing it) the action **MUST** be skipped and logged rather than raise or
  silently misfire against an unrelated queue.

This yields the invariant: **a message is only ever acked or nacked on the queue it
was polled from.**

### 1.4 Breaker recording is centralised

Circuit-breaker success/failure recording **MUST** happen exactly once per mediation
attempt, inside the mediator, and nowhere else. A pool **MUST NOT** record its own
breaker outcome from the mediation result it receives — it only acts on the outcome's
disposition. This is what makes the rule "breaker success on 4xx, breaker failure on
5xx/transport, no breaker touch on 429/circuit-open/deferred/pre-flight-rejection"
(§4.2) a single, auditable code path rather than a rule every call site has to
remember to apply.

---

## 2. Message lifecycle end-to-end

### 2.1 Poll and deduplicate

Each consumer polls its queue on its own cadence. At **route time** (before any
buffering, before any dispatch decision), the manager registers the polled message
with the in-flight tracker. This is the first of **three duplicate-suppression
layers**, each catching a different failure mode:

1. **Route-time register.** A copy of a message already owned by this process (same
   application message id, or the same broker id) is detected here. Depending on
   what changed, the registration resolves to one of:
   - **New** — first sighting; proceed to routing.
   - **Redelivery** — the broker redelivered the same logical copy (matched by
     broker id when the backend has a stable one, else by application id). The
     *owner's* receipt handle is swapped to the fresher one and this arrival is
     dropped: nothing is acked or nacked for it. If the owned message is buffered in
     an ordered group whose drainer has died (e.g. because its originating consumer
     was torn down), the redelivery **MUST** kick the group's drainer back to life
     rather than leave the group stalled forever.
   - **External requeue** — the application id is already tracked, but this arrival
     carries a *different*, non-blank broker id than the owner's. This means some
     external actor (not this router's own redelivery mechanism) republished the
     same logical message onto the broker while the original copy was still being
     processed. This copy **MUST** be ACKed immediately on its own receipt handle
     (deleting the duplicate from the broker) and **MUST NOT** be submitted for
     delivery.
2. **Process-time backstop (`EnsureTracked`).** On the first delivery attempt for a
   message, the router re-checks tracker ownership. This catches the case where the
   route-time entry was reaped (§7's stall/reap housekeeping) while the message was
   still buffered: a *different* broker copy that has since claimed the same
   application id wins ownership, and this attempt is ACKed as a duplicate and
   abandoned rather than delivered twice.
3. **Backend-side guard (where the backend has one).** SQS's own recently-acked-id
   cache (`pendingDelete`) additionally prevents re-processing a message this process
   already deleted moments ago but that the broker redelivered before the delete
   settled.

**Contract every backend owes: `BrokerMessageID` MUST be stable across redeliveries
of the same logical message.** A broker id that changes on every redelivery collapses
layer 1 into treating every redelivery as an *external requeue* — the router acks the
"duplicate" while the original delivery is still in flight, turning at-least-once
delivery into at-most-once (this is exactly the defect NATS's original
consumer-sequence-based id had; see §8.4 — the fix is a broker id built from the
*stream* sequence alone [R-19]).

### 2.2 Route: pool selection

A message names its target pool by `poolCode`. Resolution:

| `poolCode` | Resolution | Warning |
|---|---|---|
| non-empty, matches a configured or synthesised pool | that pool | none |
| non-empty, matches nothing | falls back to `DEFAULT-POOL` | one `ROUTING` warning |
| empty | falls back to `DEFAULT-POOL` | none (this is the documented default, not an error) |

A pool code suffixed `-DEFAULT-POOL` (conventionally `{clientIdentifier}-DEFAULT-POOL`)
is a **per-client fallback pool**, synthesised on demand rather than requiring
explicit configuration [R-59]. An implementation **MUST** synthesise such a pool with
default settings the first time a message names it, and **MUST** evict it once it has
been idle (no message routed to it) past a configurable TTL — its group processors
finish their buffers first, per the drain rules in §5. A pool re-synthesises on demand
if traffic resumes after eviction. An explicitly configured pool of the same code
takes ownership; the synthesis mechanism never overrides a real config entry.

### 2.3 Route: malformed-message handling

Two distinct kinds of malformed input exist, resolved at different layers:

**Malformed routing fields** — an ordered `dispatchMode` with no `messageGroupId`, or
(under the strict gate, §0) an absent `poolCode` or `dispatchMode` altogether. The
platform-layer default ("unspecified/unknown `dispatchMode` ⇒ `NEXT_ON_ERROR`",
[A-09, X-01]) applies **only** at the platform's own write boundaries — API input and
stored rows. **It does not apply at the router's wire.** A message that reaches the
router with `dispatchMode` genuinely absent is not silently promoted to
`NEXT_ON_ERROR` by the router itself [R-16]:

- With the strict gate **on**, any of: empty `poolCode`, empty `dispatchMode`, or an
  ordered `dispatchMode` with no `messageGroupId`, is malformed. The router **MUST**
  ACK the message (never deliver it, never NACK it) and raise a notice naming the
  reason, before any pool or delivery logic runs.
- With the strict gate **off** (today's default everywhere, §0), the pre-ruling
  fallbacks apply instead: empty `poolCode` → `DEFAULT-POOL` (§2.2); an absent or
  unrecognised `dispatchMode` **IS rewritten to `NEXT_ON_ERROR`** at parse time
  [X-01] (an unrecognised value additionally warns — it is a producer bug); and an
  ordered mode with **no message group** dispatches through the unordered
  (IMMEDIATE-equivalent) path [R-13], because ordering is only ever defined
  *relative to a group* (§3.1). The two rules compose: a producer that sends a
  `messageGroupId` but omits `dispatchMode` gets full `NEXT_ON_ERROR` ordering;
  a producer that sends an ordered mode but no group gets concurrent dispatch.
  Only under the strict gate is absence a drop rather than a default [R-16].
- An *unknown, non-empty* `poolCode` is never "malformed" under either setting — it
  always falls back to `DEFAULT-POOL` with the `ROUTING` warning (§2.2); only true
  absence is dropped.

**Malformed payloads** — the broker's own message body fails to decode at all (not a
FlowCatalyst-shaped JSON object). This is a broker-backend concern, not a routing
concern, and is specified in §8.2 [R-17, A-07].

### 2.4 Admit or push back

A pool that is at buffer capacity **MUST** NACK the incoming message with a short
delay rather than block or drop it — this NACK is the **only** backpressure signal
the router sends to a broker (§6). Upstream of that, a consumer **SHOULD** pause its
own polling once every pool *its own last batch fed* is full, rather than polling
indiscriminately and immediately bouncing everything it fetches; judging readiness by
the whole process rather than by the consumer's own downstream pools means one idle
pool would otherwise keep every consumer fetching work it cannot place.

### 2.5 Branch on dispatch mode, deliver, resolve

- `IMMEDIATE` messages dispatch concurrently, one worker per message, bounded only by
  the pool's concurrency semaphore.
- Ordered modes (`NEXT_ON_ERROR`, `BLOCK_ON_ERROR`) enqueue onto their message
  group's FIFO buffer, drained one message at a time by a single drainer per group
  (§3).

Delivery is the mediator's job (§4): check the endpoint breaker, take a rate-limit
token, deliver over HTTP, classify the response into a named outcome. **The pool
always sees the outcome after the mediator's own retry policy has run its course
(§4.3), never a first raw response.**

Resolution turns the outcome into exactly one broker action — acknowledge, retry
in-process, or release — and, for an ordered group, additionally decides whether the
rest of the group may continue (§3, §4.4).

The signed HTTP POST body is exactly `{"messageId":"<Message.ID>"}` — nothing else.
The router never re-serialises or forwards the queue payload; the target is expected
to load whatever it needs using the id as a pointer. Request construction: method
`POST`; `Content-Type: application/json`; `Accept: application/json`;
`Authorization: Bearer <authToken>` when the message carries a (possibly empty)
auth token; `X-FlowCatalyst-Signature` and `X-FlowCatalyst-Timestamp` when the
message carries a signing secret (§4.1 covers the exact signing formula). Header
casing on the wire is not part of the contract — **receivers MUST match
case-insensitively**; an implementation SHOULD emit the canonical
`X-FlowCatalyst-*` spelling [R-45 deferred; keep this as the working convention
until ruled].

---

## 3. Ordering modes

### 3.1 Shared machinery, one point of difference

All three modes share the same FIFO buffer and the same in-order delivery mechanics.
**Ordering is defined only relative to a group** — a message in an ordered mode that
carries no `messageGroupId` is, today, effectively unordered (§2.3); this is what
keeps an ordered *default* mode safe, since most producers set no group and must not
be silently serialised into one lane per pool regardless of how high the pool's
concurrency is configured.

The modes differ in exactly one respect: **what a terminal failure of the group's
head does to the untried messages buffered behind it.**

| Mode | On the head's terminal failure |
|---|---|
| `IMMEDIATE` | No group exists; not applicable — every message dispatches independently. |
| `NEXT_ON_ERROR` (**default** — see §2.3 for the wire-level caveat) | The head is marked failed for human review; the group **continues** with the next message. The failed head is never retried in front of its siblings. |
| `BLOCK_ON_ERROR` | The whole group stops. The untried siblings are removed from the broker (see below) and the group waits, platform-side, for review of the failed head. |

### 3.2 What "terminal failure" means, and the review flow [A-01, X-01]

A head does not fail on a single bad response. It follows the **one named retry
policy** (§4.3) — the bounded HTTP-attempt burst plus the pool's own backoff curve,
now collapsed into a single observable schedule [A-02, A-03] — and only once that
policy is exhausted for a rejecting outcome (§4.4's `REJECTED` disposition) is the
head **terminally failed**.

At that point:

- The failed head is marked for human review (platform-side `FAILED` state). It is
  **never retried independently by the router** past this point [A-02: no terminal
  give-up applies to the *broker-redelivery* path for a message that is unavailable,
  not to a rejected one already handed to review].
- **`NEXT_ON_ERROR`**: only the failed head waits for review. Its siblings, already
  buffered or arriving later, continue through the group normally.
- **`BLOCK_ON_ERROR`**: the whole group stops. The untried siblings currently
  buffered behind the head **MUST** be removed from the broker (ACKed, not
  released/NACKed) rather than left to redeliver on the broker's own timer — see the
  rationale below. **This branch is gated per §0: MUST NOT ship without the platform
  settled path.**
- A human reviewer resolves the failed head with one of three verbs: **ignore**
  (`CancelDispatchJob`-shaped: → cancelled, nothing re-sent), **complete**
  (`CompleteDispatchJob`-shaped: → completed, treated as though delivery succeeded),
  or **resend**. *Resend* is a platform-side use case, not a router action: it marks
  the group's records back to pending, and the platform's own poller/scheduler
  re-publishes them onto the broker in original order. For `BLOCK_ON_ERROR` this is
  what actually re-delivers the siblings the router removed — they are not lost, only
  no longer sitting on the broker; the platform's own store is the system of record,
  and the queue copy was only ever a delivery attempt.

**Why `BLOCK_ON_ERROR` ACKs rather than releases the untried siblings.** Releasing
(NACKing) them reads as the safer choice and is the opposite. They would redeliver on
the *broker's own timer*, unconnected to whether the review has resolved anything —
and by the time they redeliver, the head has already been marked failed and set
aside, so the first sibling becomes the de-facto new head and gets delivered. That is
precisely "add item" applied to an order that was never actually formed: the outcome
`BLOCK_ON_ERROR` exists to prevent. ACKing and re-publishing on explicit review
resolution is what keeps the group's order intact across the failure. This is the
core reasoning the ledger records under [A-01] and the reason the branch carries a
platform-existence precondition rather than being unconditionally safe to ship.

### 3.3 The retryable/unavailable split that decides an ordered head's path

Not every failure of an ordered head means "review this." The router's response
classification (§4.2, §4.4) already splits "the target is down or not ready" from
"the target ran the request and rejected it" — and that split, not the ordering mode,
decides whether a failing head is released back to the broker or handed to review:

- **Unavailable** (transport error, timeout, 502/503/504, unexpected status,
  circuit-open): nothing about the message is wrong. The whole group is **released**
  back to the broker (head and any buffered siblings together, never the head
  alone — releasing the head alone would let successors still buffered here
  redeliver *ahead* of it, reordering the very thing an ordered group exists to
  protect). The broker's own redelivery is the retry mechanism, indefinitely,
  bounded only by whatever retention the broker itself enforces [A-02]. No group is
  held in router memory across an outage of unknown length.
- **Rejected** (an outcome the retry policy has exhausted without success — most
  5xx statuses under [R-57], §4.4): the head follows the review flow of §3.2.

### 3.4 `GroupHolding`: the platform-side predicate

For dispatch jobs specifically, the router never observes delivery failure directly —
its mediation target for a dispatch job is the platform's own processing endpoint,
which records the real outcome and answers `200 {ack:true}` regardless (§9.2). Mode
semantics for dispatch jobs are therefore enforced **platform-side**, at two points
that must agree with each other:

- **Claim time**: the scheduler's poller holds back a `BLOCK_ON_ERROR` job whose
  group has an earlier holding sibling; `IMMEDIATE` and `NEXT_ON_ERROR` are never
  held back at claim time.
- **Delivery time**: the processing endpoint itself re-checks the same predicate and,
  if the group is held, ACKs the queue message and reverts the job to pending
  **without spending any attempt or retry budget** — this gate exists because
  messages already sitting on the queue when the sibling ahead of them stalled would
  otherwise arrive and deliver straight past the hold.

**Holding** means a sibling status of `FAILED`/`ERROR` (`ERROR` retained for legacy
rows; `FAILED` is the live terminal-failure state), **or** a job sitting `PENDING`
with a *future* scheduled-for time (a job mid-retry-backoff). That second case is easy
to lose in a reimplementation: such a job is excluded from the normal claim query by
its own future schedule and is not `FAILED`, so a naive "is anything holding this
group" check that only looks at terminal states misses it — and its successors would
be delivered while it is still waiting out its own backoff. A backed-off job still
owns its place at the front of the group. `QUEUED` and `PROCESSING` deliberately do
**not** count as holding: that is the ordinary flow, where a group's whole eligible
run is claimed in one batch and the router's own per-group FIFO sequences it —
treating the normal in-flight state as holding would collapse every ordered group to
one job per poll.

The comparison is **positional** — over the same ordering the claim query uses, not
plain set membership — because "this group contains a held job" would include the
held job itself the instant its own backoff expired, and the group would never move
again.

The router's own in-process mode handling (§3.1–3.3) governs any producer whose
messages the router can observe delivery failure for directly: operator-submitted
messages, and any producer pointing a message at a real subscriber URL rather than
through a platform relay endpoint.

### 3.5 Ordering guarantees and non-guarantees

Guaranteed: FIFO **attempt** order within `(pool, group)` for ordered messages, as
observed by one router process, across retries.

Not guaranteed: order across pools; order across processes (a message redelivered
after a process restart is re-routed from scratch, with no memory of where it was);
order between `IMMEDIATE` messages, even sharing a group id (`IMMEDIATE` ignores
group entirely); order across broker polls on a backend whose claim semantics only
serialise within one poll batch (see the Postgres backend note, §8.2). Batch order
within a single poll is preserved for *submission*, but delivery order among
`IMMEDIATE` messages after that is arbitrary.

---

## 4. Delivery outcome contract

### 4.1 Request construction and signing

Covered in §2.5 for the body/headers shape. Signing, when a signing secret is
present: `timestamp = now (UTC, millisecond precision, RFC3339-shaped, literal Z)`;
`signature = hex(HMAC-SHA256(key = signing secret, data = timestamp ‖ body))`. The
exact bytes signed are the exact bytes sent — the router constructs the one-field
body first and signs *that*, never re-serialising an inbound payload (documentation
elsewhere describing "the router signs the payload bytes it receives" is stale; the
router constructs its own body). The receiving side accepts a clock skew tolerance
(no ledger ruling narrows this from the reference implementation's window; keep it
generous, on the order of minutes, not seconds).

### 4.2 The named outcomes

The mediator classifies every delivery attempt (including "no attempt was possible")
into one of a small set of outcomes. Each carries a status code (when known), a delay
in seconds (when the outcome implies a wait), and — for one outcome — a group-flush
instruction.

| Outcome | Produced when |
|---|---|
| **Success** | 2xx, and the body is not a JSON object carrying `ack:false` |
| **Success + FlushGroup** | 2xx, body is JSON carrying `{"ack":true,"flushGroup":true[,"delaySeconds":N]}` |
| **Deferred** | 2xx, body is JSON carrying `{"ack":false[,"delaySeconds":N]}` |
| **ErrorConfig** | 4xx (any), an unfollowed 3xx, an unsupported mediation type, a malformed target URL, or (per [R-57]) any 5xx that is not 502/503/504 |
| **ErrorProcess** | 502/503/504, or an unexpected/pre-200 status the client library cannot interpret as final |
| **ErrorConnection** | transport failure — timeout, connection refused, TLS failure, DNS failure, request-build failure |
| **RateLimited** | 429 |
| **CircuitOpen** | breaker was open; no HTTP call attempted |

**A real status code MUST be carried on the outcome, not flattened** [A-04]. A target
answering `201` or `204` is recorded as `201`/`204`, not folded to a generic `200` —
the status is the target's own answer and an operator reading a trace has no way to
recover it once discarded.

### 4.3 The retry policy — a single named object [A-02, A-03]

Delivery retries **MUST** be expressed as one observable schedule, not as two
independently-tuned layers (an in-call HTTP retry burst nested inside a separately
curved pool backoff) that happen to compose. An implementation MAY structure this
internally however it likes, but the *observable* attempt spacing and the outcome/
breaker/metric accounting a caller sees **MUST** match a single named policy record —
this is a refactor-safety ruling, not a request to change the schedule's shape.
Concretely, the reference schedule is:

- A dead 5xx/transport target retries at floor 30s, doubling with jitter-free
  exponential shape, capped at 5 minutes (reached by attempt 12).
- A circuit-open outcome retries at floor 5s on the same curve shape.
- A deferred (`ack:false`) outcome retries on its own curve: 5s, 10s, 20s, 40s,
  60s (cap) — a caller's requested `delaySeconds` **floors** this curve but never
  lifts the 60s cap.
- A 429 uses the error curve, floored at the target's `Retry-After` value (parsed as
  an integer count of seconds; an unparseable or absent `Retry-After` floors at 30s).

**There is no terminal give-up [A-02].** No max-attempts counter, no dead-letter
path. A message that is genuinely unavailable retries until the *broker* expires it
(retention/TTL is the terminal condition, not the router); backoff plus the
per-endpoint circuit breaker are what actually protect a struggling target, since
once a message is released to the broker the *broker's* redelivery cadence, not the
router's curve, governs how often it comes back (§4.4's release note). An
implementation **MUST NOT** introduce a max-attempts or dead-letter mechanism of its
own; a rejected (as opposed to unavailable) message is instead handed to the review
flow of §3.2, which is the router's only path off "retry forever."

### 4.4 The full response table

This is the router's operational contract, merging the reference implementation's
outcome table with the conformance corpus's disposition/breaker/metric columns. The
corpus (§11) is the source of truth for the exact fields; this table is a readable
summary of it, organised by response shape rather than by corpus case id.

| Condition | Disposition | Breaker | Warning |
|---|---|---|---|
| 2xx, no `ack:false` | **DELIVERED** (ack) | success | none |
| 2xx, `{"ack":true,"flushGroup":true}` | **DELIVERED** (ack this message; suppress the rest of its group — §4.5) | success | none |
| 2xx, `{"ack":false[,"delaySeconds":N]}` | **RETRY_IN_PLACE** (deferred curve, floor N) | **neither** | none |
| 400 / 401 / 403 / 404 | **UNDELIVERABLE** (ack-drop) | **success** | ERROR |
| 501 | **UNDELIVERABLE** (ack-drop) | success | **CRITICAL** |
| other 4xx (402, 405–428, 430–499, 418, …) | **UNDELIVERABLE** (ack-drop) | success | ERROR — **MUST warn**, not merely log; a permanent drop with no warning leaves no trace of a deleted message [conformance divergence, §11.2] |
| 3xx (any; redirects are never followed) | **UNDELIVERABLE** (ack-drop) | success | ERROR [R-05, A-06] |
| 500 and every other 5xx except 502/503/504 | **REJECTED** → the review flow (§3.2) | success | ERROR [R-57] |
| 502 / 503 / 504 | **RETURN_TO_BROKER** (release; whole group under an ordered mode) | failure | none |
| transport error / timeout | **RETURN_TO_BROKER** (release) | failure | none |
| unexpected/1xx-class status | **RETURN_TO_BROKER** (release) | failure | none |
| 429 | **RETRY_IN_PLACE** (error curve, floor = `Retry-After`) | **neither** | none |
| circuit open (no call made) | **RETURN_TO_BROKER** (release) | **neither** (no call — nothing to record) | none |
| unsupported mediation type | **UNDELIVERABLE** (ack-drop) | **none at all** [A-11] | ERROR/CONFIGURATION [R-06] |
| malformed target URL (no host) | **UNDELIVERABLE** (ack-drop) | **none at all** [A-11] | ERROR/CONFIGURATION [R-06] |
| duplicate copy (dedup layer) | **DELIVERED** (ack the duplicate; original unaffected) | not applicable | none |
| group suppressed by an earlier flush | **DELIVERED** (ack without any HTTP call — §4.5) | not applicable | none |

Two rows deserve emphasis because they read as counter-intuitive and are exactly
right:

- **A 4xx records a breaker *success*, not a failure** [conformance corpus: "a
  breaker exists to detect an unhealthy *target*, and a target rejecting a bad
  request promptly is working perfectly." Counting it a failure would trip the
  circuit on a producer's own bug and stop delivery to a healthy endpoint]. The same
  applies to every `UNDELIVERABLE` outcome in the table above except the two
  pre-flight rejections.
- **The two pre-flight rejections (unsupported mediation type; malformed target URL)
  touch the breaker not at all** [A-11] — success or failure both imply a call was
  made and something was learned about the target; a rejection before any network
  attempt is evidence about neither. An implementation that records these as breaker
  *successes* (matching an easy misreading of "ack-drop ⇒ like every other 4xx-shaped
  rejection") is wrong: these two rows are the one case in the whole table where the
  ack-drop still records nothing on the breaker at all.

**5xx boundary, precisely [R-57].** The dividing line is: 502/503/504 (plus
transport-level failure) means "the target is not ready," and every other status
≥500 — including the plain `500`, `505`, and anything above — means "the app ran and
answered, badly." The former releases the *whole group* back to the broker with no
warning (an outage is not a configuration mistake); the latter is rejected into the
human-review flow with a warning as the deleted message's only trace. This is
**broader** than "exactly 500 rejects, everything else 5xx retries" — an
implementation that special-cases only `500` (or only `501`) and retries every other
5xx forever is non-conformant.

**Unfollowed redirects are permanent, not retried** [R-05, A-06]. The delivery client
**MUST NOT** follow redirects at all. A 3xx reaching the delivery client is a
permanent, non-retryable `UNDELIVERABLE` outcome: retrying reproduces the same
redirect every time (the target cannot become reachable-as-addressed by retrying),
and *following* it is not a safer alternative — 301/302/303 downgrade a POST to a
body-less GET, so the target would receive nothing at all and the router would
misreport a successful delivery.

**Circuit-breaker key is origin + path, query string excluded** [R-12]. Two messages
whose target URLs differ only in query string share one breaker; a query string is
per-message data and would otherwise fragment the failure signal so a genuinely dead
endpoint never trips.

### 4.5 `flushGroup`: target-initiated group suppression [A-05]

A 2xx response may carry `{"ack":true,"flushGroup":true[,"delaySeconds":N]}`. This
ACKs the message that carried the response **and** suppresses delivery of the rest
of its group for a window sized by `delaySeconds` (clamped: `<= 0` → a sane default
around 60s; too large → capped around 5 minutes, so a target cannot silence a group
indefinitely). Every subsequent message of that group, while suppressed, **MUST** be
ACKed without any HTTP call, without spending a rate-limit token or a concurrency
slot — the flush check runs *before* the rate limiter, which is the whole point:
absorbing a blocked group one message at a time is exactly the cost this feature
exists to avoid.

**Any target may set this flag; there is no per-pool opt-in** [A-05]. This is a
deliberate, logged-to-revisit risk: the router acts purely on the target's say-so,
without verifying it. The safety condition, which **MUST** be documented for every
integrator, is: **a target that does not own the durable copy of what its messages
point at must never set `flushGroup`.** Setting it asserts "I already have these
records and will re-drive them myself" — a target relying on the queue copy as its
only copy of the payload that sets this flag is indistinguishable from data loss.

Rules an implementation MUST hold:

- **Extend-only**: a flush cannot shorten an already-live suppression window; a probe
  landing mid-window cannot pull the expiry in.
- **Ungrouped is a no-op**: `flushGroup` on a message with no group id does nothing
  (there is no group to suppress) and MUST be logged, not silently ignored and not
  treated as an error.
- **`ack:false` takes precedence over `flushGroup`** when a response body somehow
  carries both — a target asking for the message back cannot simultaneously discard
  its group.
- **Suppression is per pool**, not global — the same group id in two different pools
  suppresses independently.
- **A suppressed ACK MUST record its own metric** distinct from an ordinary success,
  so a heavily-flushed pool reads as busy-but-suppressed rather than idle
  [R-53].
- The suppression state **MUST** be exposed on the monitoring surface (which groups
  are currently suppressed, in which pool, until when) with an operator action to
  clear one early [R-52] — otherwise "why is this group quiet?" is unanswerable from
  outside the process.

---

## 5. Reconfiguration & lifecycle

### 5.1 In-place reconfiguration; pools are never rebuilt for a parameter change [X-11]

A configuration tick that changes a pool's rate limit or concurrency **MUST** be
applied to the *live* pool object, never by tearing it down and recreating it.
Concurrency is a plain semaphore; shrinking it **MUST** be admission-only — running
deliveries are never interrupted, and the pool converges to the new, lower
concurrency naturally as in-flight work finishes. (An implementation that blocks
acquiring the excess permits to force an immediate shrink is non-conformant — this
was a real defect found during Rust's port: a bounded blocking wait of up to 60s to
force convergence, instead of admission-only convergence.) Buffer capacity, similarly
derived from concurrency, adjusts the same way: an admission check only, briefly
overfull is acceptable.

Only a genuine **removal** (the pool code, or the queue name, no longer appears in
config) or a **change** (same queue name, different connection/URI/visibility)
triggers new lifecycle machinery — and both of those **drain rather than abort**:

- **A removed pool** stops admitting new work immediately (synchronously, as part of
  applying the config) but drains its existing buffer and active workers in the
  background; a config apply never blocks waiting for a pool to finish draining. The
  pool **MUST** remain visible on the monitoring surface (blocked-groups,
  group-flush state) for as long as anything is still draining through it.
- **A removed or changed queue's consumer** stops *polling* immediately but **is not
  torn down**: it stays resolvable for ack/nack until every buffer entry referencing
  it — mid-delivery or sitting in a pool's group buffer — has resolved, so a message
  already in flight when the queue disappeared from config still gets to ack/nack
  cleanly instead of silently stranding both the tracker entry and the broker copy.
  For a **changed** queue specifically, the new consumer under the same logical name
  starts polling immediately while the outgoing one finishes tearing down once
  nothing references it any more.

**A new pool just starts.** No special-casing required.

### 5.2 Consumer restarts, config reloads, and leadership loss never abort a delivery in flight [R-26]

A consumer restart (e.g. after a stall), a queue configuration change, and a
leadership loss **MUST NOT** abort an in-flight HTTP delivery. The in-hand delivery
**detaches**, runs to completion independently of whatever triggered the
reconfiguration, and resolves its own broker action (ack/nack/release) once it
finishes. Buffered (not-yet-started) siblings of an ordered group follow the normal
release rules of §3.3, unaffected by this.

**Design direction**: message-group processors are long-lived. A restart or
reconfigure event affects **polling**, not **processing** — group workers keep
running across consumer rebuilds and config reloads, and exit only at process
shutdown, or when their pool is removed and their buffer has finished draining
(§5.1). The one nuance: after a *leadership loss* specifically, a detached delivery
still completes, but no **new** delivery may start (a duplicate delivery from the
newly-elected leader picking the same message back up is an acceptable, expected
consequence of at-least-once delivery, not a defect to prevent here).

### 5.3 Shutdown [R-49]

On process shutdown, the group processor is **never cancelled**. The correct
sequence is: stop accepting new work, finish the message currently being mediated
(bounded by a drain budget), then **release the rest of the buffer back to the
broker** rather than attempting to drain the whole buffer. An implementation MUST
NOT try to fully drain a deep buffer against a slow target at shutdown — that could
take arbitrarily long, and the orchestrator's own kill window (SIGTERM → SIGKILL)
would sever any deliveries still in flight anyway. The broker holding the
undelivered remainder is the safe place for it to sit until the process (or its
replacement) picks it back up.

Within-process events — reconfigure, consumer rebuild, leadership loss — cancel
nothing at all (§5.2); this shutdown sequence is specifically the process-exit path.

### 5.4 Reconfiguration also governs the settled-siblings reporting hook [A-01]

Because §3.2's `BLOCK_ON_ERROR` ACK branch depends on a platform round-trip that must
never block a pool worker or hold a concurrency/semaphore slot, an implementation
that ships it **MUST** treat the platform report as fire-and-forget with its own
bounded timeout, independent of the delivery pipeline's own timeouts. A router that
dies between ACKing the siblings and successfully reporting them **MUST** be
recoverable without operator intervention: the platform side needs an independent
reaper sweep (on the order of minutes, not hours) that notices ACKed-but-unreported
rows and recovers them on its own schedule, so the fire-and-forget report is a
latency optimisation, never the only path to correctness.

### 5.5 Leadership gating [R-33, R-34]

- **Config reload MUST be leadership-gated.** A follower that reloads configuration
  and starts consumers would create two active pollers/deliverers for the same
  queues, breaking the per-group single-drainer invariant that ordering depends on. A
  follower's reload request **MUST** answer as a no-op (refused, or a 200 that
  explicitly says nothing happened) rather than silently starting consumers.
- **Every pool/consumer, including any default/bootstrap broker configuration, MUST
  start only under leadership**, and MUST be recreated on a loss→regain cycle — not
  merely started once at process boot independent of the election outcome. A
  configuration path that starts consumers before or without leadership is
  non-conformant even if it is the "simple, single-instance-only" default broker
  path; if standby/HA is enabled at all, every pool obeys the gate uniformly.
- Losing leadership **MUST** pause polling — it MUST NOT tear down in-flight work
  (§5.2) — and **MUST** stop accepting new deliveries until leadership is regained
  (or the process exits).

### 5.6 Last-known-good configuration per source [R-30]

When configuration is fetched from multiple sources and one source starts failing
(unreachable, malformed, etc.), an implementation **MUST** hold that source's
**last-known-good** configuration rather than tearing down its pools and queues. A
transient bad fetch from one source **MUST NOT** stop traffic that source's
configuration was driving. While a source is failing or stale, the implementation
**MUST** raise a `CONFIGURATION` warning naming which source, and clear it on
recovery.

---

## 6. Backpressure & capacity

Buffer capacity for a pool is **derived from its concurrency**, not configured
independently — a pool with higher concurrency needs a deeper buffer to keep workers
fed without polling excessively. The exact multiplier is an implementation constant,
not a ruled contract value; what is contractual is the *shape*: capacity scales with
concurrency, and there is a floor so a low-concurrency pool still has room to buffer
a reasonable burst.

**NACK-with-delay is the only backpressure signal the router sends the broker.**
There is no other mechanism (no "pause the queue," no protocol-level flow control)
for telling a broker to slow down; a pool at capacity NACKs the message with a short
fixed delay and lets the broker's own redelivery timing bring it back.

A consumer **SHOULD** pause polling its own queue when every pool that *its own last
batch fed* is at capacity — polling and immediately bouncing every fetched message
wastes broker round-trips and delivery-window budget for no gain; a consumer that
keeps a queue's messages flowing to at least one under-capacity pool should keep
polling.

A synthesised per-client fallback pool (§2.2) is subject to the same capacity rules
as any configured pool, plus its own idle-TTL eviction (§2.2, §9's config table).

---

## 7. Warnings, health & observability

### 7.1 Store-first warnings [X-04]

**Every** warning, of every category and severity — including categories that a
pre-ruling implementation might route only to a push notifier — **MUST** go through
the warning store first. The store is the single path; a webhook or other push
notifier is a **severity-filtered subscriber of the store**, not an independent
producer. This closes a real gap: categories like `STALL` and `QUEUE_HEALTH`
historically bypassed the store and reached only the notifier, meaning an operator
looking at `/warnings` or a health dashboard never saw them at all.

- The notifier's floor is configurable (env-tunable), defaulting to `WARNING` — INFO
  is recorded in the store but not pushed by default.
- `INFO`-severity entries **MUST** carry a materially shorter TTL than higher
  severities (on the order of an hour, versus the store's normal multi-hour
  retention) so a flood of informational entries cannot crowd out real warnings
  before an operator sees them.
- The warnings surface **MUST** support filtering by both `severity` and `category`.

### 7.2 Readiness [R-36]

**Consumer liveness feeds readiness; pool success rate does not.** A router that has
stopped polling any of its queues is not ready, and this **MUST** be reflected in the
readiness probe. A pool with a degraded success rate against a struggling target is
a **warning-and-metric** concern, not a readiness concern — a failing target is not
the same thing as a failing router, and flipping the whole process's readiness
because one downstream endpoint is unhealthy would take an otherwise-functional
router out of a load balancer's rotation for a problem it cannot fix by restarting.

### 7.3 Categories

Warning categories an implementation **MUST** support and actually emit (not merely
declare and leave dark): `CONFIGURATION`, `CONNECTION`, `RATE_LIMIT`,
`CIRCUIT_BREAKER`, `STALL`, `RESOURCE`, `ROUTING`, `POOL_CAPACITY`, `QUEUE_HEALTH`,
`CONSUMER_HEALTH`. A pre-ruling implementation that declares a category but never
constructs it (found to be true of `CONNECTION`, `RATE_LIMIT`, and
`CIRCUIT_BREAKER` in the reference implementation's earlier state) is non-conformant.

### 7.4 The operator surface

An implementation's monitoring/operator API **MUST** answer these questions, because
each corresponds to a real "what is the router doing right now" investigation:

| Surface | Answers |
|---|---|
| In-flight messages (list + detail + force-ack) | "What is this router currently holding, and can I forcibly release one stuck entry?" |
| Mediating (currently-delivering) list | "What is actively in an HTTP call right now?" |
| Blocked / held groups | "Which ordered groups are stalled, in which pool, how many messages deep, under what pool settings?" [R-04] — this view exists specifically because the platform-side dispatch-job hold-back (§3.4) and the router's own ordered-group blocking are otherwise invisible from outside the process. |
| Group-flush suppressions (list + operator clear) | "Why is this group quiet, and can I lift the suppression early?" [R-52, R-53] |
| Circuit breakers (list + reset) | "Which endpoints are currently open, and can I force one closed?" |
| Warnings (list, filter, acknowledge) | "What has gone wrong recently, and have I already seen it?" |

---

## 8. Broker backend contracts

### 8.1 Per-backend obligations, uniformly

Every broker backend an implementation supports **MUST** provide:

- A **stable `BrokerMessageID`** across redeliveries of the same logical message
  (§2.1) — this is the single most load-bearing per-backend contract, since the
  entire dedup layer 1 depends on it.
- **Nack-with-delay semantics**, or an honest declaration that the backend cannot
  honour a delay (in which case the router's backoff curve is advisory only for that
  backend, and the circuit breaker becomes the real protection for a struggling
  target — see §4.3's note that release cadence becomes the *broker's*, not the
  router's, once a message is released).
- **No assumption of a redrive policy** (max-receive-count → dead-letter-queue). The
  router's own stance is that broker retention alone is the terminal condition
  [A-02]; if a deployment's broker *does* have a redrive policy configured, the
  operator is responsible for sizing visibility × max-receive-count comfortably above
  the delivery timeout contract (§9's 15-minute figure) with margin, so a
  long-but-eventually-succeeding delivery is never swept into a dead-letter queue
  mid-flight. An implementation SHOULD add a startup configuration check that warns
  when it can detect a redrive policy without adequate margin, if the backend exposes
  that information — this is a **revisit-if-ever-relevant** item, not a current
  requirement, because production deployments as of this writing configure no
  redrive policy at all.

### 8.2 Undecodable payloads: quarantine, don't fail the batch [R-17, A-07]

A broker message whose body does not decode as a valid FlowCatalyst message **MUST
NOT** fail the entire poll batch, and **MUST NOT** be left claimed-but-unprocessable
so that it re-claims and re-fails forever (a "poison" row that permanently blocks
everything behind it on that queue). The required behaviour is: quarantine it (move
it out of the live/claimable set into a separate failed-messages store) and continue
processing the rest of the batch. Where a message is quarantined more than once (a
row that fails, is somehow re-queued, and fails again), the quarantine store
**MUST** keep the **latest** failure, not the first — the most recent decode error is
the one that reflects the row's current state.

### 8.3 SQS-shaped backends

- A `pendingDelete` (or equivalently-purposed) guard **MUST** prevent re-processing a
  message this process already deleted moments ago but that the broker redelivered
  before the delete settled — bounded by a TTL on the order of minutes, matched to
  the backend's own delete-visibility lag.
- Receipt handles **MUST** be treated as freshness-sensitive: a redelivery's receipt
  handle **MUST** replace the tracked owner's stale one (§2.1's dedup layer),
  because the final ack/nack has to use a handle the broker still considers valid.
- **MUST NOT set a deduplication id on publish.** Content-based dedup at the queue
  level (required for a FIFO-shaped queue anyway) is sufficient; the router's own
  in-flight dedup layer (§2.1) does not depend on it either way, and a message
  mediated twice by two different router instances (e.g. across a leadership
  handover) is an accepted consequence of at-least-once delivery [R-18], not a
  correctness bug to engineer around at the publish boundary.

### 8.4 NATS-shaped (or other consumer-sequence-bearing) backends

**The broker message id MUST be built from the stream sequence alone, never from a
consumer sequence.** A backend whose delivery identity folds in a per-delivery
consumer sequence produces a *different* id on every redelivery of the same logical
message — which, fed into dedup layer 1 (§2.1), classifies every redelivery as an
*external requeue* and causes the router to ACK (consume) the redelivered copy while
the original delivery attempt is still in flight. If the original later fails, the
message is now gone from the broker with nothing left to retry: at-least-once
silently becomes at-most-once [R-19]. This is a correctness requirement, not a
style preference — an implementation supporting a JetStream-shaped backend or
anything with a similar delivery-attempt counter **MUST** exclude that counter from
the broker id.

### 8.5 Postgres-shaped (row-based) backends

- The broker id **is** the application message id on a row-based backend (there is no
  separate broker-assigned identity), which means the "external requeue" dedup branch
  (§2.1) can never fire for this backend — every redelivery is classified as a
  redelivery, correctly.
- Claim/poll semantics **MUST** serialise at least within one poll batch: at most one
  message per group is claimable per poll, so two same-group messages cannot both be
  claimed in the same batch and processed out of order relative to each other within
  that batch. Cross-poll ordering (a claimed-but-not-yet-visible head not blocking
  its successors on the *next* poll) is a known, accepted non-guarantee — the
  per-group FIFO buffer inside the router process is what actually enforces ordering
  once messages are claimed; the broker's claim query is only responsible for not
  handing out two heads of the same group in the same batch.

---

## 9. Producer contract [R-16]

A producer publishing ordered work to the router **MUST** set `poolCode` and
`dispatchMode` explicitly on every message — the platform scheduler resolves these
from the owning subscription's configuration *at publish time*, so the router never
needs to know anything about clients or subscriptions to route correctly (§2.2's
`{clientIdentifier}-{poolCode}` / `{poolCode}` / `{clientIdentifier}-DEFAULT-POOL` /
`DEFAULT-POOL` resolution is entirely the producer's responsibility to have already
performed before publish). A producer that omits either field is, once the strict
gate of §0/§2.3 is enabled, publishing a malformed message that the router will drop
rather than deliver. Until that gate is enabled everywhere, an omitting producer's
messages silently lose ordering (§2.3) — this is exactly the risk the gate exists to
retire, and is not a safe steady state to leave in production.

**Per-job signing.** Each dispatch job the platform publishes carries its own
HMAC-signed authorization token (`Authorization: Bearer <token>`), verified
independently by the receiving processing endpoint exactly as the router's own
outbound signature is verified by a webhook subscriber (§4.1) — the platform never
has to trust the router with a separate, longer-lived credential; a token is scoped
to the one job it signs.

**The processing-endpoint seam.** When a message's mediation target is the
platform's own dispatch-processing endpoint (rather than a real subscriber URL
directly), that endpoint is itself a client of this specification's response
contract (§4): it answers the router with the outcomes of §4.2/§4.4 on the router's
behalf, and separately implements the `GroupHolding` hold-back of §3.4 before
recording any attempt. Two invariants this seam **MUST** hold:

- **A job MUST NOT be left in an in-progress state with no queue message behind
  it.** Every failure path on this endpoint NACKs (letting the queue redeliver)
  rather than ACKing on an error it cannot fully resolve — ACKing here would strand
  the job, recoverable only by an out-of-band sweep.
- **A hold-back MUST cost no retry budget.** Reverting a held job to pending must not
  count as an attempt, or a group blocked for a long time would exhaust its
  siblings' retry budget purely by waiting, which is exactly what makes "re-queue the
  group on review resolution" (§3.2) actually work: the siblings arrive with their
  budget intact.

A separate settled-report seam (`/api/dispatch/settled`-shaped) is the platform-facing
half of the `BLOCK_ON_ERROR` ACK branch — see §3.2 and §5.4 for its contract and
gating.

**The 15-minute delivery-attempt timeout is contractual, not incidental.** An
implementation's per-request HTTP timeout for a delivery attempt SHOULD be sized
generously (on the order of 15 minutes in production) because the platform's own
processing endpoint may itself be doing real work — loading a job, delivering to a
real subscriber, recording the outcome — before it answers. This interacts with the
retry policy (§4.3): a single `Mediate` call bounded by this timeout, retried per the
named policy, can hold a worker for a multiple of the timeout in the worst case. This
is accepted as the cost of the seam's timeout being generous rather than the seam
being asked to answer faster than the work it does allows. **Drain-rate framing**: an
implementation's throughput SLO for a healthy pool should be read against this
budget — a pool sized for N concurrent workers against a healthy target drains at a
rate bounded by the target's own real latency, not by this ceiling; the ceiling only
matters when a target is unhealthy, in which case the circuit breaker (§4.4) is what
keeps a struggling target from being hammered at this rate at all.

---

## 10. Configuration reference

The table below merges the reference implementation's environment-variable surface
with ledger-ruled naming and defaults. Column **Ruled** marks a value or a switch's
existence as fixed by the ledger rather than left to implementation taste; column
**Default** is the value used when unset.

| Concern | Reference name | Default | Ruled? | Notes |
|---|---|---|---|---|
| Enable the subsystem | `FC_ROUTER_ENABLED` | `false` | — | |
| Monitoring/API mount prefix | `FC_ROUTER_HTTP_PREFIX` | unset | — | unset means "root only" (an implementation MAY instead default to mounting exclusively under `/router`, as the Go reference does inside its unified `fc-server` binary); when set, an implementation MUST continue serving the full route tree — public and protected alike — at root AND additionally nest the same tree under the prefix, so a deployment that health-checks/operates against `<prefix>/...` and one that hits root paths directly both work unmodified. Nesting MUST NOT change which routes are public: the public/protected split is decided before nesting, never re-derived from the mount-relative path (contrast Go's `internal/router/api` `IsPublicPath`/`mountRelativePath`, which computes the split *at* the mount point and had to fix a real bug there — [R-43], see §11.3 — that a "split first, nest after" design is not exposed to). |
| Config source URL(s) | `FLOWCATALYST_CONFIG_URL` | unset | — | comma-separated; each fetched independently and merged first-source-wins by key |
| Default/bootstrap broker | `FC_DEFAULT_BROKER` | unset (no pools start) | — | e.g. `postgres` synthesises a single bootstrap pool/queue when no config URL is set |
| Config poll interval | (code constant) | 5 min | **[A-10]** | env-tunability itself is open [R-31, deferred] |
| Strict routing gate | `FC_ROUTER_STRICT_ROUTING`-shaped | **off** | **[R-13, R-16]** | see §0 and §2.3; MUST default off until every producer is confirmed compliant |
| Malformed-routing behaviour when off | — | pre-ruling fallback (§2.3) | **[R-16]** | |
| Synthesised per-client pool idle TTL | `FC_ROUTER_SYNTH_POOL_IDLE_SECS`-shaped | on the order of 1 hour | **[R-59]** | 0/unset means "use the implementation's own default," not "never evict" |
| Platform base URL for the settled-report hook | `FC_ROUTER_PLATFORM_URL`-shaped | unset (hook disabled) | **[A-01]** | unset is the correct, safe default for a standalone router with no platform behind it (§5.4) |
| Graceful drain budget | `FC_DRAIN_TIMEOUT_SECONDS`-shaped | 60s | **[R-49]** (semantics), value not separately ruled | see §5.3 |
| Notifier target | `FC_NOTIFY_WEBHOOK_URL`-shaped | unset (store-only) | — | |
| Notifier severity floor | `FC_NOTIFY_MIN_SEVERITY`-shaped | `WARNING` | **[X-04]** | env-tunable; below-floor warnings still land in the store, only the push is filtered |
| Leader election toggle | `FC_STANDBY_ENABLED`-shaped | `false` | — | |
| Leader election backend URL | `FC_STANDBY_REDIS_URL`-shaped | local default | — | |
| Election lock key | `FC_STANDBY_LOCK_KEY`-shaped | implementation default | — | subsystems sharing one election backend MUST suffix their own key so "router leader" and any other subsystem's leader can be different instances |
| Auth for the operator/monitoring surface | `FC_ROUTER_AUTH_USER` / `FC_ROUTER_AUTH_PASS`-shaped | unset (auth disabled) | — | health/liveness/readiness/metrics/openapi endpoints MUST bypass auth regardless of mount prefix [R-43] |
| Dev-mode mediator relaxation | `FLOWCATALYST_DEV_MODE`-shaped | `false` | — | shorter timeouts, relaxed TLS; never for production |

**Naming variance across implementations**: at least one implementation has been
observed using `NOTIFICATION_MIN_SEVERITY` where the reference above uses
`FC_NOTIFY_MIN_SEVERITY` for the same concern — this specification does not mandate
a single variable name, only the existence and default of the concern; an
implementation MUST document its own naming in its own configuration reference and
MAY differ from the reference implementation's spelling.

### 10.1 Go-dialect env-name aliases (ECS drop-in compat)

An implementation intended to be a drop-in replacement for an ECS task
definition currently running the Go reference implementation (`fc-server` with
`RouterEnabled=true`) MUST read the Go-dialect canonical name FIRST, falling
back to its own historical name(s) — same env vars, same health-check path,
same ports; only the image tag changes. The table below is the alias set the
Rust implementation resolves through a single N-way "first non-empty value
wins, priority order" helper (`fc_common::config::env_first` and its
`_bool`/`_parse`/`_opt` siblings — mirrors the Go reference's own
`envFirst`/`envBoolAlias`/`envIntAlias` pattern in
`internal/server/envcfg.go`), so the alias table lives in exactly one place
rather than being reimplemented at each call site.

| Concern | Canonical (Go-dialect) name | Fallback names, in priority order | Safety-critical? |
|---|---|---|---|
| Leader election toggle | `FC_STANDBY_ENABLED` | `FLOWCATALYST_STANDBY_ENABLED`, then the Go reference's own legacy `STANDBY_ENABLED` | **Yes** — resolving this to `false` when the deployer intended `true` means the instance runs with NO leader election, actively delivering the same messages another "leader" instance is also delivering |
| Leader election backend URL | `FC_STANDBY_REDIS_URL` | `FLOWCATALYST_STANDBY_REDIS_URL`, `FLOWCATALYST_REDIS_URL`, then legacy `REDIS_URL` | Yes, jointly with the toggle above — a wrong/unreachable URL with the toggle correctly resolved fails loudly (the processor can't acquire a lock); a wrong URL is a *quieter* failure mode than the toggle resolving to `false`, but still routes traffic without the coordination the deployer configured |
| Election lock key | `FC_STANDBY_LOCK_KEY` | `FLOWCATALYST_STANDBY_LOCK_KEY` | No — both names are already implementation-local; not a Go/Rust naming split |
| Auth for the operator/monitoring surface | `FC_ROUTER_AUTH_USER` / `FC_ROUTER_AUTH_PASS` | `AUTH_BASIC_USERNAME` / `AUTH_BASIC_PASSWORD` | No, but see the mode-inference note below — silently leaving auth *off* on a monitoring surface that MUST be reachable from inside the deployment's own network is a lower-severity miss than the standby case, not a zero-severity one |
| API listener port | `FC_API_PORT` | implementation's own historical `API_PORT` | No |
| Metrics port | `FC_METRICS_PORT` | — (no fallback; there is nothing to fall back to) | No — accepted and logged as a no-op where metrics are always served on the API listener at `/metrics`; MUST NOT fail startup just because the var is set |
| Notifier target | `FC_NOTIFY_WEBHOOK_URL` | implementation's own historical `NOTIFICATION_TEAMS_WEBHOOK_URL` | No |
| Graceful drain budget | `FC_DRAIN_TIMEOUT_SECONDS` | — (no fallback; was a hardcoded constant before this ruling) | No |

Two behaviours ride along with this table and are not simple name aliasing:

- **Auth mode inference.** The Go reference enables BasicAuth on the router
  surface whenever a username is configured, with no separate "mode" switch —
  only an explicit `AUTH_MODE=NONE` (case-insensitive) turns it off regardless
  of credentials. An implementation that instead requires an explicit
  `AUTH_MODE=BASIC` before honouring `FC_ROUTER_AUTH_USER`/`AUTH_BASIC_USERNAME`
  is not a drop-in: a Go-dialect task definition sets the credentials but never
  sets `AUTH_MODE=BASIC` (it doesn't need to), so auth would silently stay off.
  MUST: when `AUTH_MODE` is unset or unrecognized and a username is present,
  infer Basic auth; an explicit `AUTH_MODE=OIDC`/`OIDC_FLOW` still wins over
  the inference for an implementation that supports those modes.
- **Notify-enabled semantics.** The Go reference has no separate
  "notifications enabled" flag — a non-empty webhook URL alone means notify.
  An implementation with its own historical separate enabled flag MUST derive
  "enabled" as *at least* "the resolved webhook URL is non-empty" (the
  historical flag, if kept, may only ever widen this, never narrow it) so a
  Go-dialect task definition that sets only the URL still gets notified.

Every other var in the §10 table already shares the Go reference's exact
name (`FLOWCATALYST_CONFIG_URL`, `FC_ROUTER_STRICT_ROUTING`,
`FC_ROUTER_SYNTH_POOL_IDLE_SECS`, `FC_NOTIFY_MIN_SEVERITY`,
`FC_ROUTER_HTTP_PREFIX`) and needs no aliasing — only, for the mount prefix,
the default-value and dual-serving behaviour described in the §10 table row
above.

---

## 11. Conformance

### 11.1 What the corpus is, and how it binds this document

`flowcatalyst-javalin/conformance/mediation-outcomes.json` is the **executable**
annex to §4 of this document. It states each case as an HTTP response (or a named
precondition — an unreachable target, a malformed target URL, an unsupported
mediation type, a pre-opened breaker) and the expected `outcome`, `disposition`,
`breaker` effect, and `metric` effect. An implementation is expected to run every
case in the corpus against its own mediator and match on `disposition` (the field
that decides the message's actual fate — `DELIVERED` / `RETRY_IN_PLACE` /
`RETURN_TO_BROKER` / `REJECTED` / `UNDELIVERABLE`) without exception, and to match on
`breaker` and `metric` unless a documented, argued divergence exists (see
`conformance/README.md`'s divergence-block convention). Matching the outcome *name*
is not required where two implementations reasonably differ for a benign,
client-library-shaped reason (the corpus's own `unexpected-status-1xx` case is the
worked example: Go and a `java.net.http`-based client disagree on the outcome name
for a bare 1xx response because of how each HTTP client surfaces it, but both reach
`RETURN_TO_BROKER` with the same delay and the same breaker effect — that is a
`correct: both` row).

Sections of this document that carry **MUST** force for the outcome contract
specifically are §4.2 (the outcome set), §4.4 (the response table, including the two
counter-intuitive rows), and §4.5 (`flushGroup`'s safety rules and the extend-only/
ungrouped-no-op/`ack:false`-precedence sub-rules). Everything else in this document
(§§1, 2, 3, 5–10) is contract in the RFC-2119 sense of its own MUST/SHOULD language,
but is not currently mechanised by the corpus — an implementation conforms to those
sections by inspection and by the ledger's own test-pinning convention ("a ruling
becomes a spec line + conformance test"), not by a shared cross-implementation
executable suite.

### 11.2 A rule found by the corpus, not by either implementation

`config-error-501` — treating 501 as an ordinary 5xx and retrying it forever — was
found by writing the corpus, independently corroborated by the fact that the
reference implementation had already reached the same special-case rule by a
different route. Two implementations arriving separately at the same boundary is the
strongest evidence available that a rule is right rather than merely convenient; this
document's §4.4 501 row exists because of that corroboration, not because either
implementation happened to do it first.

The corpus also records a same-shape gap for **every** 4xx without a named branch:
an implementation that only warns on 400/401/403/404/501 and merely logs (without
warning) on any other 4xx (422, 418, …) has the same permanence (permanent ack-drop)
without the same trace. §4.4's "other 4xx" row therefore carries a MUST-warn note
that is stricter than a naive reading of "warn on the ones I've seen in practice"
would produce.

### 11.3 Known per-implementation divergence

| Divergence | Nature |
|---|---|
| Rust's absent in-pipeline retry | Where the reference (Go) implementation's retry policy (§4.3) executes as bounded in-process retries with an `Attempts` counter that climbs across a delivery's lifetime, an implementation that has not yet built the pool-side half of the retry-policy collapse (§4.3) will observe `attempts=0` semantics on outcomes that Go's implementation would report with a non-zero attempt count. This is not a contract violation on its own — the *disposition* the corpus checks does not depend on the attempt counter — but any operator tooling or metric that surfaces "how many times was this retried" will read differently until the retry-policy collapse lands. |
| BasicAuth / mount-prefix immunity | An implementation whose public (unauthenticated) monitoring routes are structurally a separate sub-router with no auth middleware attached at all is immune to the mount-prefix path-matching defect [R-43] by construction, rather than by a path-matching fix — both are conformant; the structural approach is simply not susceptible to the class of bug the path-matching fix addresses. |
| Per-client fallback-pool synthesis presence | An implementation that has not yet built the `{clientIdentifier}-DEFAULT-POOL` synthesis-on-demand mechanism at all (as opposed to having built it but not yet wired its idle-TTL eviction) is missing a §2.2/§9-required feature outright, not merely diverging on a timing constant — this is a gap to close, not a documented, accepted divergence. |

Beyond these, any implementation-specific gap against the rulings in
`owner-questions.md` that has not yet been closed is tracked in that implementation's
own status/gap-analysis document, not duplicated here — this specification describes
the target contract, not any one codebase's current distance from it.

---

## 12. Open items this specification does not pin

Everything below is either genuinely undecided in the ledger (`— deferred`) or ruled
only as "keep the reference behaviour," not as an exact contract value. This document
deliberately does not promote any of these to a MUST-exact-value by writing them down
more confidently than the ledger does (see Change control). Where this document gives
a number above, read it as descriptive of the current reference implementation, not
as a pinned requirement, unless the surrounding text cites a ledger id.

- **Almost every timing constant** — the exact shape of the backoff curves (§4.3),
  host-connection-pool watermarks, stall-detection thresholds, tracker reap age,
  breaker idle-eviction age, breaker trip thresholds (failure rate, minimum sample
  count, half-open behaviour — single-probe vs. every-concurrent-caller), rate-limiter
  burst sizing, notifier batching cadence, and most consumer poll-loop pacing values
  are `deferred` in the ledger (R-07 through R-11, R-14, R-15, R-20 through R-25,
  R-28, R-35, R-37 through R-48, R-50, R-55, R-58, R-60 through R-62). An
  implementation SHOULD track the reference implementation's current values for these
  until each is individually ruled, but this document does not make any of them a
  MUST.
- **Whether the config poll interval is env-tunable** [R-31] — the 5-minute interval
  itself is ruled [A-10]; whether it can be overridden by configuration is not.
- **Canonical header-name casing** [R-45] — receivers matching case-insensitively is
  the safe MUST this document states in §2.5; which spelling an implementation
  *emits* is not yet ruled to a single answer.
- **Signature verification clock-skew tolerance** — no ruling narrows this from the
  reference implementation's current window; §4.1 states only that it should be
  generous, not a specific number of seconds.
- **A committed golden signing vector** [R-44] — no vector is currently checked into
  either behavioural-spec source tree; the conformance corpus does not yet pin one
  for the signing formula the way it pins the mediation-outcome table.
- **Whether an internal rate-limiter stall and an HTTP 429 should share one metric
  series, or be split** [R-09] — currently merged in the reference implementation;
  not ruled either way.
- **`ErrorConnection` vs `ErrorProcess` metric asymmetry on retried attempts** [R-07]
  and **whether `CircuitOpen` should record any pool metric at all** [R-08] — both
  flagged, neither ruled; this document's §4.4 table reflects the reference
  implementation's current (unruled) behaviour rather than asserting either is
  correct.
- **`HighPriority`'s fate** [R-14] — carried on the wire, acted on nowhere; whether an
  implementation must preserve the field at all (versus dropping it from its own
  model) is not ruled. This document does not mention it as a MUST anywhere above; an
  implementation MAY treat it as dead.

None of these gaps block conformance as defined in §11 — the corpus does not exercise
them — but an implementation reporting itself "specification-complete" should not
read that as license to treat any of the above as settled.

---

## Change control

Rulings enter through `owner-questions.md`, the ledger. This document **follows**
the ledger; it never leads it. When a fact here and a ruling in the ledger disagree,
the ledger is right and this document is stale — file the fix as a direct edit
against the relevant section, citing the ledger id that forced it, rather than
treating the disagreement as an open question to re-litigate here. A ledger entry
that is still **deferred** (no ruling yet — "current [reference] behaviour stands per
the standing convention") is documented in this specification, where it is covered
at all, as *current behaviour, not yet a ruled contract* — distinguishable from a
ruled MUST by its citation reading "deferred" rather than naming a settled ruling.
This document is not itself an authority to promote a deferred item to a ruling by
writing it down more confidently than the ledger does.

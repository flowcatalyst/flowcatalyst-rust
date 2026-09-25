# Conformance corpus

What a router **should** do with a mediation response, in a form that fails.

Language-neutral, so both implementations can run it for as long as both
exist — but this is **not a Go-compatibility harness**, and the distinction is
the whole point. Where the two differ, the corpus asserts the behaviour that
is right on the merits and records the other as a defect. Three rows already
do; a fourth (`config-error-501`) was added when this corpus caught Java
retrying a "Not Implemented" target for ever.

Freezing Go's behaviour as the definition of correct would make the port
inherit every defect the port was a chance to fix, and would make the harness
actively harmful: it would fail Java for being right.

## Why this exists

Two test suites written separately against the same prose agree only by luck.
`docs/spec/router.md` §6 has a precise outcome table that was the source of
truth for a dozen decisions, and until now nothing checked that either
implementation still matched it — the table could drift from both.

This corpus is that table, made executable — and then argued with. Several
rows now carry the reasoning for *why* the behaviour is right, independent of
either implementation, because a rule extracted from a Go source file is
evidence of what Go does and not of what is correct.

It is also the only artefact here that bears on the claim "drop-in
replacement". A row that passes in Java and fails in Go is a behaviour change
that will reach production at cutover; today nothing else would find it. But
"differs from Go" is a question, not a verdict — the row says which answer is
right.

## Design constraints

Every case is stated as **an HTTP response or a named precondition** — never
as a call into either implementation. No Java types, no Go types, no
assumptions about internal structure. A runner needs only to be able to:

1. stand up a local HTTP server it controls,
2. point a mediation target at it,
3. call its own mediator,
4. read back the outcome, the disposition, and the breaker's counters.

## Case shape

```jsonc
{
  "id": "config-error-404",           // stable; referenced in commit messages
  "spec": "§6 outcome table row 8",   // optional: where the rule is written
  "why": "…",                         // optional: the reasoning, not the rule
  "given": { "kind": "response", "status": 404, "body": "", "headers": {} },
  "expect": {
    "outcome":      "ErrorConfig",    // the outcome kind's name
    "statusCode":   404,
    "delaySeconds": 30,               // optional; omit where it is not fixed
    "flushGroup":   true,             // optional
    "httpCallMade": false,            // optional; for cases that must NOT call
    "disposition":  "UNDELIVERABLE",  // what happens to the MESSAGE
    "breaker":      "success",        // success | failure | neither | none
    "metric":       "failure"         // success | failure | transient
                                      //   | rateLimited | none
  },
  "divergence": { … }                 // optional; see below
}
```

### `given.kind`

| kind | The runner must |
|---|---|
| `response` | answer the next request with this status, body and headers |
| `unreachableTarget` | point the target at a port nothing is listening on |
| `malformedTargetUrl` | use a target URL with no host |
| `unsupportedMediationType` | set the message's mediation type to something other than HTTP |
| `breakerOpen` | drive the breaker for this target to OPEN before delivering |

### `disposition` — the field that matters most

`outcome` is the implementation's own vocabulary; `disposition` is what
actually happens to the message, and it is the safety property:

| Value | Meaning |
|---|---|
| `DELIVERED` | acknowledged; gone from the broker |
| `RETRY_IN_PLACE` | kept, retried here, position preserved |
| `RETURN_TO_BROKER` | handed back; the broker owns the retry |
| `REJECTED` | given up on and acknowledged away |
| `UNDELIVERABLE` | can never succeed as addressed; acknowledged away |

Two implementations may reasonably disagree about the outcome *name* and must
never disagree about this. Where they do, a message is lost or duplicated.

## Divergences

A `divergence` block records a row where the implementations differ. It must
carry:

- `correct` — `java`, `go`, or `both`. Which side the row asserts.
- `basis` — **why that answer is right**, argued from the behaviour itself and
  not from either codebase. This is the field that keeps the corpus honest: a
  divergence whose only justification is "the other one does it differently"
  has not been thought about.
- what the other implementation does, and where its fix is tracked.

`correct: "both"` is for cases where the outcome *name* differs for a benign
reason but the disposition — the message's actual fate — is identical.

If you cannot write `basis`, the row is an open question. Say so and leave it
unasserted rather than picking a side by default; the default would be Go, and
that is exactly the bias this corpus exists to avoid.

Current entries:

| Case | Correct | Nature |
|---|---|---|
| `success-carries-real-2xx-status` | java | The status is the target's own answer; flattening 201/202 to 200 discards information irrecoverably. Go fix tracked in `docs/spec/router-fixes.md`. |
| `unfollowed-3xx-is-permanent` | java | Retrying reproduces the redirect for ever and following it drops the body. Go retries indefinitely. Owner ruling 2026-08-24. |
| `unexpected-status-1xx` | both | Client-library artefact: Go's client surfaces the 1xx as final, `java.net.http` refuses to and fails the exchange. Both reach `RETURN_TO_BROKER`, 30s, breaker failure. |
| `malformed-target-url` | java | Go ACK-drops silently. Java warns: it deletes every message routed through it and is a fixable configuration mistake. |
| `unsupported-mediation-type` | java | As above. |

Not a divergence, but found by this corpus: `config-error-501`. Java was
treating 501 as an ordinary 5xx and retrying for ever, because the `>= 500`
branch was tested before anything could special-case it.

The `warning` column encodes one rule: **a permanent ACK-drop must warn; a
retryable outcome must not.** A permanent drop deletes the message, so the
warning is its only trace; a retryable one keeps it, and warning per attempt
would flood the store during any ordinary outage.

## Running it

**Java** — part of the default build, no profile or tag:

```
mvn -pl server test -Dtest=MediationConformanceTest
```

**Go** — not yet written. `conformance/go-runner.md` is the handover spec:
the API it needs (all verified present), the outcome-name mapping, a
`given.kind` table, and the one Go change `disposition` requires. Phase 1
asserts six of the seven fields and needs no changes to Go at all.

It belongs in the Go repo and must read the corpus by path rather than copy
it — two copies drift and then prove nothing.

## Adding a case

Add it when a rule is *decided*, not when code is written — a case authored
from the implementation only restates it, and a case authored from the Go
source restates Go.

Give the row an `id` that says what it pins. Where the rule is surprising, put
the argument in `why`, not just the rule: `config-error-404` records a breaker
**success**, and every reader's first instinct is that it should be a failure.
The reason it is not — a breaker detects an unhealthy *target*, and a target
rejecting a bad request promptly is working perfectly — is the part worth
writing down, because it is what tells the next person whether a new case
belongs on that side of the line.

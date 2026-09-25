# Go runner — handover spec

Build the Go half of `conformance/mediation-outcomes.json`. The Java half is
`server/src/test/java/io/flowcatalyst/router/conformance/MediationConformanceTest.java`
and runs in the default build.

**Read `conformance/README.md` first.** In particular: this is *not* a
Go-compatibility harness. A row that fails is a question with three possible
answers — Go is wrong, Java is wrong, or the corpus is wrong — and the answer
is argued, not assumed. Do not "fix" Go to match Java without checking which
one is actually right, and do not assume Go is right because it shipped first.

## Where things live

The corpus is in the **Java** repo and must be **read, not copied** — two
copies drift and then prove nothing:

```go
const corpusPath = "../flowcatalyst-javalin/conformance/mediation-outcomes.json"
```

Make the path overridable by env var (`FC_CONFORMANCE_CORPUS`) so CI can point
at a checkout. If the file is missing, **skip with a clear message** rather
than fail — the Go repo must still build standalone.

## The API you need

All verified present as of `819b390`:

```go
breakers := router.NewBreakerRegistry(router.DefaultBreakerConfig())
mediator := router.NewHTTPMediator(router.DevMediatorConfig(), breakers)
outcome  := mediator.Mediate(ctx, &msg)   // common.MediationOutcome
stats    := breakers.Get(targetURL).Stats()  // BreakerStats{Successes, Failures uint64}
```

`common.MediationOutcome` carries `Result`, `StatusCode`, `DelaySeconds`,
`FlushGroup`, `ErrorMessage`. `BreakerStats` already exports cumulative
`Successes`/`Failures`, so the breaker column needs no new API.

### Outcome names

The corpus uses implementation-neutral names. Map them:

| corpus `outcome` | Go `MediationResult` |
|---|---|
| `Success` | `common.MediationSuccess` |
| `Deferred` | `common.MediationDeferred` |
| `ErrorConfig` | `common.MediationErrorConfig` |
| `ErrorProcess` | `common.MediationErrorProcess` |
| `ErrorConnection` | `common.MediationErrorConnection` |
| `RateLimited` | `common.MediationRateLimited` |
| `CircuitOpen` | `common.MediationCircuitOpen` |

### `given.kind`

| kind | Go |
|---|---|
| `response` | `httptest.NewServer` answering that status, body and headers |
| `unreachableTarget` | open a listener on `:0`, record the port, close it, point the target there |
| `malformedTargetUrl` | target `http:///no-host` |
| `unsupportedMediationType` | set `msg.MediationType` to something other than HTTP |
| `breakerOpen` | `for breakers.Get(url).State() != router.CircuitOpen { breakers.Get(url).RecordFailure() }` |

### The breaker column

Snapshot `Stats()` before and after each `Mediate`, and assert the **delta**:

| `expect.breaker` | delta |
|---|---|
| `success` | successes +1, failures +0 |
| `failure` | successes +0, failures +1 |
| `neither` / `none` | both +0 |

This column is the one most often got wrong, because the intuitive answer is
wrong: a 404 is a breaker **success**. The endpoint answered, so the target is
healthy — it is the request it cannot serve. Counting it a failure would trip
the circuit on a client bug and stop delivery to a working endpoint.

## Phase 1 — what you can assert today

`outcome`, `statusCode`, `delaySeconds`, `flushGroup`, `breaker`, and
`httpCallMade`. That is the whole classification layer, and it is where every
defect this corpus has found so far lived (four, listed below).

Ignore `disposition` for now; see Phase 2.

## Phase 2 — `disposition`, and the Go change it needs

`disposition` is the field that matters most: it is what actually happens to
the *message*, and two implementations may reasonably disagree about an
outcome's name but must never disagree about this. The values are
`DELIVERED`, `RETRY_IN_PLACE`, `RETURN_TO_BROKER`, `REJECTED`,
`UNDELIVERABLE` (`conformance/README.md` defines them).

**Go cannot currently assert it.** The decision lives in an inline `switch
outcome.Result` inside the pool's delivery loop (`internal/router/pool.go`
around line 901), entangled with metrics, group flushing and backoff. There is
nothing a runner can call.

The change is to extract that switch into a **pure function** of the outcome —
`func dispositionOf(out common.MediationOutcome) Disposition` — and have the
pool loop call it. No behaviour change; the switch arms move, the side effects
stay put.

Worth doing on its own merits, and there is direct evidence: Java's
disposition used to be a *defaulted* boolean on the outcome type, overridden
by only two of seven cases. The other five silently inherited "the target is
fine, so the message is at fault", and an ordered group whose head met an open
circuit was ACK-deleted, siblings and all. Making it an explicit member every
case must answer is what surfaced that. Go's version is not defaulted, but it
is unreachable and untested, which fails differently and just as quietly.

Recommend doing Phase 1 first and landing it — it is useful immediately and
needs no Go changes.

## Current divergences

Verified against `819b390` while writing this:

| Case | Correct | Status |
|---|---|---|
| `success-carries-real-2xx-status` | **java** | Open. `common.Success()` still hard-codes `StatusCode: 200`, so 201/202/204 are all reported as 200. Fix is in `docs/spec/router-fixes.md` Fix 1. |
| `unfollowed-3xx-is-permanent` | **java** | Open. A 3xx falls through to `default:` → `ErrorProcess(30)` with `StatusCode` unset, so it is retried for ever and reports status 0. Owner ruling 2026-08-24: permanent, ACK-drop. |
| `unexpected-status-1xx` | **both** | Benign. Same `default:` arm gives `ErrorProcess(0, 30)`; `java.net.http` refuses to treat 1xx as final and fails the exchange, giving `ErrorConnection(0, 30)`. Both reach `RETURN_TO_BROKER`, 30s, breaker failure — identical fate. Expect this row to differ on `outcome` and agree on everything that matters. |
| `config-error-501` | **both** | Agreed. Go fixed this in `4f2d52c`; Java was still letting 501 fall into the generic `>= 500` branch and retrying a "Not Implemented" target for ever, and this corpus caught it. Independent agreement on the same answer is the best evidence either fix was right. |

## Warnings

**Now asserted.** Every case carries `expect.warning`: `ERROR`, `CRITICAL`, or
`none`. Java raises through a `Warnings` collaborator on the mediator; Go
through `m.warnConfig(...)`. Category is `CONFIGURATION` on both sides.

Go was ahead here and its behaviour was copied: ERROR for 400/401/403/404 and
other 4xx, CRITICAL for 501. Java had these as `TODO(warnings)` and dropped
them on the floor.

The rule the column encodes: **a permanent ACK-drop must warn; a retryable
outcome must not.** A permanent drop deletes the message and the warning is
the only trace it leaves. A retryable one keeps the message, and warning per
attempt would flood the store during any ordinary outage — a 500, a 429 and an
open circuit all raise nothing.

Two rows where Java now goes further, both `correct: java`:

| Case | Go |
|---|---|
| `malformed-target-url` | returns `ErrorConfig` **silently** |
| `unsupported-mediation-type` | returns `ErrorConfig` **silently** |

Both ACK-drop every message routed through them, permanently, and both are
configuration mistakes an operator can fix. Go warns on 404 for exactly that
reason and then does not warn here, which reads as an omission rather than a
decision — the pre-flight rejections are *more* clearly configuration errors
than a 404 is, not less.

## Skeleton

```go
func TestMediationConformance(t *testing.T) {
    corpus := loadCorpus(t)           // skip if absent
    for _, c := range corpus.Cases {
        t.Run(c.ID, func(t *testing.T) {
            breakers := router.NewBreakerRegistry(router.DefaultBreakerConfig())
            mediator := router.NewHTTPMediator(router.DevMediatorConfig(), breakers)
            target, cleanup := setUp(t, c.Given, breakers)   // per given.kind
            defer cleanup()

            before := breakers.Get(target).Stats()
            out := mediator.Mediate(context.Background(), messageFor(target, c.Given))
            after := breakers.Get(target).Stats()

            assertOutcome(t, c, out)
            assertBreaker(t, c.Expect.Breaker, before, after)
        })
    }
}
```

Keep it one file. It is a fixture runner, not a framework.

## When a row fails

Write down which of the three it is before changing any code:

1. **Go is wrong** → fix Go, note the case id in the commit message.
2. **Java is wrong** → say so; it gets fixed on the Java side and the row stays.
3. **The corpus is wrong** → fix the row *and its `basis`*. If you cannot
   write a `basis` for the corrected expectation, the rule is not decided yet:
   mark the row as an open question rather than picking a side. The silent
   default is whatever Go already does, which is the bias this corpus exists
   to avoid.

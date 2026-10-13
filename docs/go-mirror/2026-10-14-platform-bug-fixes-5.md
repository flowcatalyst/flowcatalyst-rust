# Go platform bug fixes, fifth batch, mirrored (2026-10-14)

Go's F13 ingest unwrap and F14 ingest client and source rules
(`flowcatalyst-go/docs/platform-bug-fixes.md` F13, F14 and decisions 20, 21;
`flowcatalyst-go/docs/wire-contract.md` "Encoded `data` and `operationData` at batch ingest" and
"SDK ingest: client and source"; owner rulings 2026-10-12), checked against Java
(`flowcatalyst-javalin/docs/go-mirror/2026-10-13-platform-bug-fixes-4.md` B and `-5.md`,
`docs/spec/sdk-ingest.md` §3.3 to §3.5 and §4). Every commit is on `idiomatic-rust`, not pushed.

The earlier batches are `2026-10-10-platform-bug-fixes.md`, `2026-10-11-platform-bug-fixes-2.md`,
`2026-10-11-event-catalogue.md`, `2026-10-12-platform-bug-fixes-3.md` and
`2026-10-13-platform-bug-fixes-4.md`.

Tests named `*_test::` are in `crates/fc-platform/tests/it/` (`FC_TEST_PG_BIN=<postgres bin> cargo
test -p fc-platform --test it -- --include-ignored <module>::`).

## Summary

| Item | Go | Java | Rust commit |
|---|---|---|---|
| A. Batch ingest unwraps an encoded event `data` / audit `operationData` | `8bf67d1` | `14ea9b5a` | `deebef82` |
| B. Dispatch-job `clientCode` | `fe25501` | `c1572b44` | `842c21d2` |
| D. Events: a `clientCode` refusal names the code | `abd2ae9` | `2928d4a0` | `c5a3bd5a` |
| C. A missing event `source` is defaulted | `4c3654a` | `5fc01261` | `43258d94` |
| Ledger | `4bc3cc6` | | (this file) |

(The letters are the brief's rules; D went in before C because it touches the same lookup B
added.)

## A. Encoded `data` and `operationData` (`deebef82`)

**Rule** (`fc_platform_core::shared::encoded_json::unwrap_encoded_json`; Go
`sdk.UnwrapEncodedJSON`, Java `EncodedJson.unwrap`):

- A member that is not a JSON string is returned as sent: an object, array, number, boolean,
  `null`, or an absent member.
- A string whose text, less JSON whitespace only (space, tab, LF, CR), starts with `{` or `[` and
  is exactly one valid JSON value is replaced by that value.
- Every other string is kept: `"hello"`, `""`, `"42"`, `"true"`, `"null"` (Java's null map), a value
  encoded twice, `"{not json"`, `"[1,"`, `"{} trailing"`, `"{}{}"`.
- At most one level is removed; the result of an unwrap is an object or array, so a second pass
  changes nothing.

**Where**: `POST /api/events/batch` (`shared/batch_api.rs`, `data`) and `POST /api/audit-logs/batch`
(`shared/sdk_audit_batch_api.rs`, `operationData`, before `audit_redaction::redact_document`).

**Unchanged**: the singular `POST /api/events`, dispatch-job `payload` (a string on the wire),
function events, the outbox processor, and `/bff/events/batch` (see Divergences).

**Accepted cost** (Go's and Java's): a writer that means a string whose text is a JSON object or
array gets the object.

**Tests**:

- Lib `shared::encoded_json::tests` (fc-platform-core):
  - `gos_cases`: Go's 23 table cases that apply to a parsed body (its two raw-input cases, `` and
    `"abc`, cannot reach the handler once the body has been parsed), each also unchanged by a
    second pass.
  - `only_json_whitespace_is_trimmed`: `"\u000b{}"`, `"{}\f"`, `" {}"` and `" []"` stay
    strings.
  - `non_json_text_is_kept`: single quotes, unquoted keys, trailing commas, `NaN`, leading zeros,
    a comment, a raw control character.
  - `the_text_is_parsed_as_a_body_is` and `nesting_beyond_the_parser_limit_is_kept`: see
    Divergences.
- Lib `shared::sdk_audit_batch_api::tests::encoded_operation_data_is_unwrapped_then_redacted`
  (fc-platform).
- `encoded_ingest_test`, through HTTP, with the Rust SDK's golden fixtures copied verbatim from
  `flowcatalyst-go/clients/rust-sdk/tests/fixtures/wire` (Go `544158f`) into
  `crates/fc-platform/tests/data/rust-sdk-wire/` (`event-full.json`, `event-minimal.json`,
  `audit-log-full.json`; byte-identical, checked with `cmp`):
  - `event-full.json` is stored as the object, under the client its `clientCode` `acme` names.
  - `event-minimal.json` was refused in its slot ("source is required") at this commit, and is
    `SUCCESS` with source `orders:sales` since C. Stored as `{}`.
  - Go's table: the value itself, an encoded array, `"[]"`, `"hello"`, `"null"`, `"42"`,
    `"{not json"`, a value encoded twice, a number, each with its `jsonb_typeof`.
  - The singular create keeps a string `data`.
  - Audit: `audit-log-full.json` whole (the test creates the `orders` application and the `acme`
    client) is stored as `{"apiKey":"***","total":1500}`; an encoded `{"password":"hunter2",...}`
    is stored as the redacted object; an object, `"plain text"` and `"null"` are kept.
- `audit_redaction_test::ingested_audit_logs_are_redacted_before_they_are_stored` pinned the old
  behaviour (an encoded document stored as a JSON string); it now expects the redacted object.
- **Failing first**: with the unwrap removed from both handlers, both `encoded_ingest_test` tests
  failed at their first assertion (`("string", "{\"orderId\":...}")` and `("string",
  "{\"apiKey\":...}")`).
- **Mutant, equivalent here**: redaction moved before the unwrap still stores the redacted object
  (`redact_document` already parses an encoded string and re-encodes it redacted; the unwrap then
  parses that). The order matters in Go and Java, not here; it is kept as theirs.

**Stored rows**: not migrated, as in Go and Java. To size them:

```sql
SELECT count(*) FROM msg_events WHERE jsonb_typeof(data) = 'string'
  AND left(ltrim(data #>> '{}'), 1) IN ('{', '[');
```

and the same on `aud_logs.operation_json`. The owner decides, as for Go.

## B. Dispatch-job `clientCode` (`842c21d2`)

`CreateDispatchJobRequest` gained `clientCode` (alias `client_code`; never stored). It is the item
of `POST /api/dispatch-jobs/batch` (`shared/sdk_dispatch_jobs_api.rs`), the body of `POST
/api/dispatch-jobs` and `/bff/dispatch-jobs`, and the item of `POST /bff/dispatch-jobs/batch`
(`dispatch_job/api.rs`); all four apply the rule. `caller_reach::resolve_job_client`, per job,
before anything is written:

1. a non-blank `clientId` wins (the code is ignored, even one naming another client) and goes
   through `require_writable_client`: 403 `FORBIDDEN` "No access to client: `<clientId>`";
2. else a code naming a client the caller reaches resolves to it;
3. a code naming another client, or none, from a non-anchor: 403 `FORBIDDEN` "No access to client:
   `<clientCode>`";
4. a code naming no client from an anchor: 400 `CLIENT_NOT_FOUND` "Client not found:
   `<clientCode>`";
5. neither: `require_writable_client(None)`, unchanged (anchor: platform-scoped; a caller with
   exactly one client: that client; anyone else 403 "clientId is required: …"). Rust keeps its
   refusal; see Divergences.

A blank (`""`, whitespace) `clientId` or `clientCode` is absent. Any refusal rejects the whole batch
(no `items[i]` prefix). A request's distinct codes on jobs without a `clientId` are looked up in one
query (`caller_reach::codes_to_resolve`, `api::resolve_job_client_codes`), none when there are
none. `SdkDispatchJobsState` gained `client_repo`.

**Tests**:

- Lib `shared::caller_reach::tests` (fc-platform-core): `only_codes_of_items_without_a_client_id_are_looked_up`,
  `a_job_code_resolves_to_a_client_the_caller_reaches`,
  `a_job_code_the_caller_cannot_use_is_refused_naming_the_code` (status, code and message).
- `ingest_client_code_test::a_dispatch_job_names_its_client_by_code` (three clients whose
  identifiers differ from their ids; an anchor, a caller confined to `acme`, a partner of `acme`
  and `coda`):
  - anchor: two items sharing a code resolve; an explicit id wins over a code; a blank code is no
    code (`null` stored); a blank `clientId` beside a code resolves the code;
  - anchor, unknown code: 400 `CLIENT_NOT_FOUND` "Client not found: nosuch-client", the valid item
    before it not written;
  - client-scoped: own code resolves; own id wins over another client's code; another client's
    code and an unknown code are the same 403 naming the code, the body never names the other
    client's id, nothing of the batch written;
  - partner: names one of its clients by code (before: 403 "clientId is required");
  - singular: the anchor's known and unknown code, the client's two 403s, own id over another
    code, the partner's code;
  - `/bff/dispatch-jobs/batch`: the partner's code resolves, another client's code is 403;
  - the table holds exactly the 12 accepted jobs at the end.
- **Failing first** (the code ignored, which is the code before the fix): the test failed at the
  anchor's first job (`client_id` `None` for `acme`).
- **Mutants** (lib tests, each killed): the anchor's unknown code answering 403 instead of 400; a
  resolved id not reach-checked; the code winning over `clientId`; a blank code not ignored.

## D. Events: a `clientCode` refusal names the code (`c5a3bd5a`)

`POST /api/events/batch` resolved a `clientCode` and passed the client id to
`require_writable_client`, so a code naming a client the caller cannot reach answered "No access
to client: `<that client's id>`": it leaked the other tenant's id and told an existing code apart
from an unknown one ("No access to client: `<code>`"). `caller_reach::resolve_event_client` now
answers 403 `FORBIDDEN` "No access to client: `<clientCode>`" for both.

Already as Go and Java here: an explicit `clientId` wins and goes through
`require_writable_client`; an anchor's unknown code leaves the event platform-scoped; blank
`clientId` and `clientCode` are absent (Rust's `non_blank`, before this batch); the single-client
default; the partner refusal; the whole-batch refusal. The singular `POST /api/events` has no
`clientCode` (as Go and Java).

**Tests**: lib `caller_reach::tests::an_event_code_refusal_names_the_code`;
`ingest_client_code_test::an_event_client_code_refusal_names_the_code` (another client's code and an
unknown code, each with and without a blank `clientId`: the same 403 naming the code, the body never
names the other id, the valid item before it not written; own code, own id over another code, a
blank id beside the own code and alone; the partner's code and its refusal without one; the
anchor's unknown code and blank id platform-scoped, a blank id beside a known code resolving it;
the singular route's blank `clientId`). **Failing first**: with the old arm restored (the resolved
id through `require_writable_client`) the test failed with "No access to client:
clt_6GK4EQ3FY7PHF" where "No access to client: beta" was expected.

## C. A missing event `source` is defaulted (`43258d94`)

`event::api::event_source(source, type)` (Go `eventSource`, Java `EventIngestMapper.eventSource`):
a non-blank source is returned as sent (not trimmed); else the trimmed type's first `:` segment,
plus `:` and the second when that is non-blank (`orders:sales:order:created` → `orders:sales`,
`orders` → `orders`, `orders::x` and `orders: :x` → `orders`); `None` when the first segment is
blank (`:sales:order:created`, a blank type).

- `POST /api/events/batch`: `batch_item_error` stores the default on the item; only a blank first
  segment is refused in its slot, `BAD_REQUEST` "source is required".
- `POST /api/events` (and `/bff/events`): `CreateEventRequest.source` is optional; a blank first
  segment is 400 `VALIDATION` "source is required", nothing written. Before, an absent source
  failed the body's deserialization.
- `POST /bff/events/batch` (Rust's own handler, items of the singular shape): defaulted the same
  way; a blank first segment refuses the whole batch 400 `VALIDATION` (that route has no per-item
  slots).

**OpenAPI**: `CreateEventRequest.source` is `Option<String>`, so the served document no longer
lists it in `required`; it and `BatchEventItem.source` carry Go's descriptions. The vendored Go
lock `frontend/openapi/openapi.json` takes Go `4c3654a`'s lock hunk (both schemas now equal Go's
`api/openapi.lock.json`, compared as parsed JSON) and `frontend/src/api/generated/types.gen.ts`
takes Go's hunks byte for byte. The route snapshot takes the new document hashes.

**Tests**:

- Lib `event::api::tests::a_missing_source_is_the_types_application_and_subdomain`
  (fc-platform-messaging): Go's and Java's table, 14 rows, and the `VALIDATION` refusal.
- `event_source_default_test`: Go's batch table (absent, `null`, blank, snake_case `event_type`,
  given, type with spaces, two segments, one segment); a blank first segment refused in its slot
  beside an item that lands; the singular route (absent, blank, given; a blank first segment 400
  `VALIDATION`, nothing written); `/bff/events/batch` (defaulted; refused).
- `encoded_ingest_test`: `event-minimal.json` now `SUCCESS`, source `orders:sales`.
- **Failing first**: before the fix the batch answered the first table item `BAD_REQUEST`
  "source is required".
- **Mutants** (lib test, each killed): the first segment only; the type not trimmed; a blank given
  source kept; the second segment checked for emptiness without trimming.

## Divergences from Go and Java

- **Unwrap parses; Go keeps bytes** (A). The text is parsed by serde_json, as the request body is,
  so the stored value is what the same value sent as itself becomes (Java's choice too; Go stores
  the inner bytes and JSONB normalises them). Pinned in lib tests:
  - numbers: the workspace's serde_json has `preserve_order`, not `arbitrary_precision`, so an
    integer beyond `u64` and a long fraction become `f64`
    (`123456789012345678901234567890` → `1.2345678901234568e29`); JSONB would keep Go's digits;
  - a number beyond `f64` (`[1e400]`) is refused by serde_json, so the text stays a string (Go
    unwraps it; sent as itself, the body is refused here);
  - a lone UTF-16 surrogate escape is refused by serde_json, so the text stays a string (Go's
    decoder makes it U+FFFD);
  - nesting: serde_json's recursion limit, 127 levels parse and 128 do not (the same for the body
    itself); Java's is 500, Go's 10,000;
  - a duplicate key keeps the last value, as JSONB does.
- **Whitespace** (A, B, C, D): the unwrap trims JSON whitespace only, as Go and Java. "Blank" and
  "trimmed" elsewhere use `str::trim`, Unicode `White_Space`, the same set as Go's
  `unicode.IsSpace` (Java's `isBlank`/`strip` differ on U+00A0, U+0085, U+2007, U+202F and
  U+001C–U+001F).
- **`/bff/events/batch` is not unwrapped** (A). In Java it is the ingest handler and unwraps; Go
  has no such route. Here it is its own handler taking the singular create's shape, which the
  ruling leaves alone; the SPA sends objects.
- **Dispatch jobs with neither `clientId` nor `clientCode`** (B): `require_writable_client`, as
  Java (owner: left as is); Go writes a non-anchor's job platform-scoped (Go's open finding).
- **`/bff/dispatch-jobs/batch` takes `clientCode`** (B): its item is `CreateDispatchJobRequest`, so
  the field is in its schema and is honoured rather than silently dropped. Go has no such route.
- **Order** (B): in the SDK batch the client is resolved first per item, then the kind, target and
  confinement (Java's order); Go maps first. The singular checks code, target, confinement and
  service account before the client. Only the first refusal of a request with two faults differs.
- **Singular event order** (C): Rust checks the client guard and signing before `data` and
  `source` (as Java); Go checks `data` and `source` first. A refused request with no source gets
  403 here where Go answers 400.
- **`/bff/events/batch` refuses a blank-application item for the whole batch** (C): that route has
  no per-item results.

## Wire impact

- `POST /api/events/batch`, `POST /api/audit-logs/batch`: an encoded object or array is stored,
  fanned out and delivered as the object.
- Dispatch jobs: `clientCode` accepted on the four create routes; a code naming no client is now
  refused (400 for an anchor, 403 otherwise) where it was ignored; a partner can name its client by
  code.
- Events batch: a refused `clientCode` names the code, not the other client's id.
- Events: `source` optional on `CreateEventRequest`; a missing source succeeds where it was
  refused (`BAD_REQUEST` in the batch, a body error on the singular and BFF routes).

## Verified

All database tests ran on the `FC_TEST_PG_BIN` harness (embedded PostgreSQL 18.3 from the Go
repo's archive, a private cluster on a free loopback port); no dev database was touched.

- **Before each fix** (failing first, recorded per item above): A both `encoded_ingest_test` tests
  with the unwrap removed; B `ingest_client_code_test::a_dispatch_job_names_its_client_by_code`
  with the code ignored; D the events test with the resolved-id arm restored; C
  `event_source_default_test` on the sources before the fix (only the test module added).
- **Per commit**: the touched `it` modules (`encoded_ingest_test`, `audit_ingest_test`,
  `audit_redaction_test`, `app_compat_test`, `ingest_app_scope_test`, `ingest_security_test`,
  `ingest_client_code_test`, `dispatch_descriptor_test`, `dispatch_ingest_pending_test`,
  `api_integration_tests`, `delivery_policy_test`, `event_source_default_test`, `go_field_gaps_test`,
  `openapi_go_contract_test`, `route_auth_convention_test`, `route_wiring_convention_test`) and the
  lib tests above, all passed; the route snapshot regenerated with `UPDATE_ROUTE_SNAPSHOT=1` at B and
  C (document hashes only).
- **Final commit** (`43258d94`):
  - `cargo fmt --all -- --check`: clean.
  - `cargo clippy --workspace --all-targets -- -D warnings`: clean.
  - Lib tests: fc-platform-core 134, fc-platform-messaging 271, fc-platform 77, all passed.
  - The whole `it` suite with `--include-ignored`: **478 passed, 20 failed**. The 20 are the
    modules that start their own Docker container whatever the harness says
    (`login_backoff_integration_tests` 8, `webauthn_integration_tests` 6,
    `function_table_prefix_test` 4, `id_roundtrip_test` 1, and
    `dispatch_scheduler_test::the_scheduler_publishes_to_its_tenants_sqs_fifo_queue`, LocalStack):
    each failed with `CreateContainer(RequestTimeoutError)` (no Docker here), as in the earlier
    batches.
  - `FC_TEST_PG_BIN=… scripts/sqlx-prepare.sh --check`: passes (no checked query or migration
    added).

## Not verified

- Embedded databases only; no run through a running outbox processor or a deployed platform, and
  no browser.
- Already-stored encoded rows were not counted on any database.
- Builds stalled for long stretches with rustc idle in a proc-macro `dlopen` (macOS `syspolicyd`;
  `target/debug/deps` holds about a million files); runs were restarted until they completed, with
  `CARGO_INCREMENTAL=0`, outside the command sandbox.

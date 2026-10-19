# Go to Rust migration mapping

Owner decision D5 (2026-10-18): Go's goose files (`../flowcatalyst-go/internal/migrate/sql/`) are canonical for the
shared schema. Rust mirrors each Go migration it lacks, one Rust file per Go file, with Go's SQL verbatim (goose
directives and the Down section removed, comments ASCII-folded). Every mirrored migration is idempotent, so a database
Go migrated and a database Rust created end in the same schema. The runner is `crates/fc-migrations`; the tracker is
`_schema_migrations`, keyed `NNN_name`. A new Go migration needs a row here and a row in `PENDING.md` within the week.

State: Go at 086 (`bbbb4bb3`; Go 083/084, the function service account, are not mirrored). Rust at 096. Rechecked 2026-10-19 after the identity and F28-F34 batch (Go `e64ac802`): Go added no migration and this batch added none here; its schema change is none (the new `.sqlx` entry is a query, not a table).

## Mapping

| Go | Rust | What | Probe true when |
|---|---|---|---|
| 001-030 | 001-030 | Identical once comments are stripped | assumed applied |
| 031 | 041 | mfa tables | table, junction |
| 032 | 044 | reset approval | table |
| 033 | 042 (by name, not verified) | password reset factor attempts | purpose CHECK, redirect_uri |
| 034 | 031 | principal all_applications | column |
| 035 | 071 | persist-boundary columns (only `oauth_idp_role_mappings.idp_type` was missing) | column |
| 036 | 072 | read-projection indexes | indexes |
| 037 | 073 | login throttle index, drops 5 redundant indexes | index present, `idx_msg_events_read_time` gone |
| 038 | 035 | scheduled jobs application_id | column |
| 039 | 052 | developer API credentials | column |
| 040 | 055 | IdP role sync | column, junction |
| 041 | 046 | portal identities | tables, column |
| 042 | 043 | oauth client api_access | column |
| 043 | (none, not verified) | drops the IdP portal binding; no Rust migration creates it | n/a |
| 044 | 051 | app docs | table |
| 045 | 063 (by name, not verified) | dispatch job dirty index (Rust's 063 is the wider index rebuild) | index |
| 046, 047 | 033 | oauth client secret grace | column |
| 048 | 074 | dispatch mode default | default is NEXT_ON_ERROR |
| 049 | 075 | partition `iam_login_attempts` (self-guarded rebuild and swap) | table partitioned |
| 051 | 076 | X-06 enum CHECK constraints | three of the constraints |
| 052 | 077 | X-06 dispatch-job CHECK constraints | two of the constraints |
| 053 | 047 | portal apps | tables, columns |
| 054 | 039 | dispatch job queue column, attempt request | columns |
| 055 | 078 | data: seeded `v1` event schema versions become `1.0` | no `v1` row left |
| 056 | 050 | connection application scope | column, index |
| 057 | 057 | dispatch job descriptor and read metadata | columns |
| 058 | 038 | aud_logs entity_id width | width |
| 059 | 079 | `fng_*` registry, `function_id` on subscriptions and scheduled jobs, FUNCTION source | tables, columns |
| 060 | 080 | `msg_scheduled_job_instances.not_before` | column |
| 061 | 081 | `fng_versions.runtime` | column |
| 062 | 082 | `fng_domains`, `fng_routes` | tables |
| 063 | 083 | data: scrub SECRET/DB values from `aud_logs` | no unscrubbed row |
| 064 | 084 | `fng_routes.alias_prefixes` | column |
| 065 | 063 | dispatch job scheduler indexes | index |
| 066 | 064 | dispatch queue (retired in practice) | see probe in lib.rs |
| 067 | 065 | dispatch queue claim indexes | index |
| 068 | 066 | drop dispatch queue | index present, table absent |
| 069 | 067 | application-service batch permissions | role absent, or the permission present |
| 070 | 068 | client event type codes | no `platform:iam:client:*` type |
| 071 | 069 | prune unemitted event types | archive table exists, none of the 15 left |
| 072 | 070 | platform event schema fixes | no seeded `dispatchPoolId` form left |
| 073 | 085 | data: `function:deleted` event-type rename | old row absent |
| 074 | 086 | data: other eight function event-type renames | old rows absent |
| 075 | 087 | F17 data: connection `service_account_id` prn_ to sac_ | no connection names a principal id |
| 076 | 088 | F18 data: normalise role assignment names | no whitespace name |
| 077 | 089 | F18 data: backfill `iam_roles.application_id` | nothing to backfill |
| 078 | 090 | F19: `msg_dispatch_pools.application_id` | column |
| 079 | 091 | F19 grant: `dispatch-pool:sync` to application-service | role absent, or granted |
| 080 | 092 | F20 grants: six admin-page permissions | role absent, or granted |
| 081 | 093 | F22: `fng_versions.requeued_at_revision` | column |
| 082 | 094 | F24: `msg_dispatch_jobs.connection_id` | column |
| 083, 084 | (none, pending) | function version runtime tag and function service account (`fng_functions.service_account_id`); function model P4, out of scope of the dispatch-job batch | n/a |
| 085 | 095 | data: grant requeue, cancel, complete, edit-payload to every role holding `dispatch-job:view` except `platform:viewer` | no such role lacks requeue |
| 086 | 096 | `msg_dispatch_jobs_read.connection_id`; index swap (code, subscription_id to `(col, created_at DESC)`; partial connection index) | partial connection index present |

Rows for Go 031-054 are mapped from file names and the Rust probe comments (`crates/fc-migrations/src/lib.rs`). The
fingerprint equality proves the end state matches, not that each row pairs the right two files; rows marked "not
verified" are the ones where only the name was matched. Rust 071-078 mirror the Go migrations (035-052, 055) whose
effects a fresh Rust database still lacked.

## Rust-only objects (additive, ignored by Go)

`app_client_configs.{base_url_override,config_json}` (058), `iam_principal_roles.assigned_by` (059),
`iam_service_accounts.wh_*` (060), `msg_dispatch_jobs_read.queue` (061), and every `fnr_*` table (062). The schema
fingerprint test allows exactly these (`crates/fc-migrations/tests/fixtures/rust-only-objects.txt`) and fails on any other
difference in either direction.

## Function tables (decision D1)

`fng_*` is the function schema of record (079, 081, 082, 084, 093). The `fnr_*` tables of Rust 062 are **kept**: the
Rust function code (`fc-platform-functions`, the component host) still reads and writes them, and no behaviour code
changes in P1. Dropping them is a P4 migration, written when that code moves to `fng_*`; until then both exist and
`rust-only-objects.txt` allows `fnr_*`. Nothing was dropped and no data path was disturbed.

## Hazards

- **B (Rust on a Go database older than Go 072), fixed.** Rust 067-070 had no probe, so the pre-tracker backfill
  recorded them as applied on a database that never ran them. They now have probes; verified on a Go-068 scratch
  database holding unmigrated rows (all four ran and made the rows Go's).
- **A (Go on a Rust-first database).** Go's `bootstrap()` seeds goose from `_schema_migrations` by number. That is a
  Go-repo fix (F27), not mirrored here. Until it lands, do not start Go against a database Rust migrated first.
- **C (Rust 070 against Go 072).** Rust 070 corrects four schemas Go's 072 does not (`oauth-client:secret-rotated`,
  `admin:roles:synced`, `admin:subscription:synced`, `iam:user:logged-in`). The probe above treats a database whose
  dispatch-pool schema is already corrected (Go 072 ran) as done, so on a Go database at 072 or later those four stay
  as Go has them. An owner decision (reconcile the payloads) is still open.
- **049 on a Rust database.** Rust 075 converts `iam_login_attempts` to a quarterly partitioned table, the way Go's 049
  does (copy then swap, once). On a large production table that is a long transaction; run it in a quiet window. Rust's
  repository already copes with both shapes.
- **076/077 CHECKs.** A stored value outside the allowed set makes the migration fail loudly. Prescan a database Rust
  has been writing to before the upgrade.
- **Role catalogue.** 091 and 092 grant the new application-service permissions only to a role row that exists. Rust's
  catalogue (`permissions.rs`) does not carry them, so `POST /bff/roles/sync-platform` would strip them again. That is
  code, not schema: batch P2 (F19/F20).

## Checking it

- Fresh database, Rust migrations: `cargo test -p fc-platform --test it -- rust_schema_matches_go_fingerprint --ignored`
  (Docker or `FC_TEST_PG_BIN`). `comparison_catches_each_kind_of_drift` runs without a database.
- Go-082 database, then Rust: `scripts/schema-fingerprint.sh takeover` (its own container).
- Regenerate the Go side after a Go migration: `scripts/schema-fingerprint.sh regen-go-fixture`.

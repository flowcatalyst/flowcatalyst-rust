-- Rust 077 mirrors Go 052_x06_dispatch_job_enum_check_constraints.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- X-06 CHECK constraints on msg_dispatch_jobs, msg_dispatch_jobs_read and msg_dispatch_job_attempts. Guarded like 076; fails loudly on a stored value outside the allowed set.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- X-06 final tranche: CHECK constraints backing the strict (T, bool) enum
-- parsers for common.ParseDispatchStatus and dispatchjob's ParseKind /
-- ParseRetryStrategy / ParseErrorType - the four parsers migration 051's
-- commit message named as still lenient ("Still to convert:
-- common.ParseDispatchStatus and dispatchjob's
-- ParseKind/ParseRetryStrategy/ParseErrorType"). common.ParseDispatchMode
-- (the `mode` column) is NOT covered here and gets no CHECK constraint -
-- it is the one deliberate X-06 exemption, ruled X-01 (unknown/absent mode
-- defaults to NEXT_ON_ERROR rather than rejecting).
--
-- Each constraint matches its Go const block (+ accepted legacy aliases)
-- exactly - see internal/common/dispatch_status.go and
-- internal/platform/dispatchjob/entity.go for the source of truth:
--
--   - status:         PENDING, QUEUED, PROCESSING, COMPLETED, FAILED,
--                      CANCELLED, EXPIRED - PLUS the legacy aliases
--                      IN_PROGRESS (-> PROCESSING) and ERROR (-> FAILED).
--                      ERROR predates the current status set; it MUST stay
--                      admitted because dispatchjob.GroupHoldingStatusSQL
--                      still matches `status IN ('FAILED', 'ERROR')` by
--                      design ("so old rows keep blocking as they always
--                      did") and legacy rows using it exist - this is the
--                      exact case 051's own commit message flagged ahead of
--                      time. Widening a constraint to admit a legacy value
--                      that a real code path still depends on is legitimate
--                      (contrast: do not widen merely to make a scan pass).
--   - kind:            EVENT, TASK.
--   - retry_strategy:  immediate, fixed, exponential - PLUS the legacy
--                      aliases IMMEDIATE (-> immediate) and FIXED_DELAY
--                      (-> fixed); nullable (defaults to exponential when
--                      unset).
--   - error_type:      CONNECTION, TIMEOUT, HTTP_ERROR, VALIDATION,
--                      UNKNOWN; nullable (attempts without a recorded
--                      error type, i.e. successes, leave it NULL).
--
-- Pre-scan: every column below was queried against a freshly-migrated test
-- database (schema + all migration-seeded bootstrap rows) for values
-- outside its allowed set. Zero violations found - dispatch jobs are
-- infrastructure-processing writes (see dispatchjob/entity.go's package
-- doc), never migration-seeded bootstrap data, so there is no analogue of
-- 051's 'PLATFORM' fixture surprise here. This does not scan a live
-- production database - see scripts/ops/x06-052-prescan.sql, which MUST be
-- run against production before this migration deploys there; any stored
-- value outside its Go const block (beyond the legacy aliases already
-- admitted above) will make this migration FAIL TO APPLY.
--
-- msg_dispatch_jobs, msg_dispatch_jobs_read, and msg_dispatch_job_attempts
-- are all RANGE-partitioned parent tables (migration 019). Exactly as in
-- 051: ALTER TABLE ... ADD CONSTRAINT on a partitioned parent validates and
-- applies the constraint across every existing partition (including the
-- DEFAULT partition, if any) and is inherited by every future partition
-- the housekeeping loop creates - no per-partition migration needed.
--
-- Each ADD CONSTRAINT is guarded by a pg_constraint existence check
-- (PostgreSQL has no `ADD CONSTRAINT IF NOT EXISTS`), matching 051's
-- idempotency discipline.

-- common.DispatchStatus - internal/common/dispatch_status.go (partitioned parent; propagates to all partitions)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_dispatch_jobs_status'
    ) THEN
        ALTER TABLE msg_dispatch_jobs
            ADD CONSTRAINT chk_msg_dispatch_jobs_status
            CHECK (status IN ('PENDING', 'QUEUED', 'PROCESSING', 'IN_PROGRESS', 'COMPLETED', 'FAILED', 'ERROR', 'CANCELLED', 'EXPIRED'));
    END IF;
END $$;

-- dispatchjob.Kind - internal/platform/dispatchjob/entity.go (partitioned parent; propagates to all partitions)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_dispatch_jobs_kind'
    ) THEN
        ALTER TABLE msg_dispatch_jobs
            ADD CONSTRAINT chk_msg_dispatch_jobs_kind
            CHECK (kind IN ('EVENT', 'TASK'));
    END IF;
END $$;

-- dispatchjob.RetryStrategy - internal/platform/dispatchjob/entity.go (partitioned parent; propagates to all partitions)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_dispatch_jobs_retry_strategy'
    ) THEN
        ALTER TABLE msg_dispatch_jobs
            ADD CONSTRAINT chk_msg_dispatch_jobs_retry_strategy
            CHECK (retry_strategy IS NULL OR retry_strategy IN ('immediate', 'IMMEDIATE', 'fixed', 'FIXED_DELAY', 'exponential'));
    END IF;
END $$;

-- common.DispatchStatus, mirrored on the read projection - internal/common/dispatch_status.go (partitioned parent; propagates to all partitions)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_dispatch_jobs_read_status'
    ) THEN
        ALTER TABLE msg_dispatch_jobs_read
            ADD CONSTRAINT chk_msg_dispatch_jobs_read_status
            CHECK (status IN ('PENDING', 'QUEUED', 'PROCESSING', 'IN_PROGRESS', 'COMPLETED', 'FAILED', 'ERROR', 'CANCELLED', 'EXPIRED'));
    END IF;
END $$;

-- dispatchjob.Kind, mirrored on the read projection - internal/platform/dispatchjob/entity.go (partitioned parent; propagates to all partitions)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_dispatch_jobs_read_kind'
    ) THEN
        ALTER TABLE msg_dispatch_jobs_read
            ADD CONSTRAINT chk_msg_dispatch_jobs_read_kind
            CHECK (kind IN ('EVENT', 'TASK'));
    END IF;
END $$;

-- dispatchjob.RetryStrategy, mirrored on the read projection - internal/platform/dispatchjob/entity.go (partitioned parent; propagates to all partitions)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_dispatch_jobs_read_retry_strategy'
    ) THEN
        ALTER TABLE msg_dispatch_jobs_read
            ADD CONSTRAINT chk_msg_dispatch_jobs_read_retry_strategy
            CHECK (retry_strategy IS NULL OR retry_strategy IN ('immediate', 'IMMEDIATE', 'fixed', 'FIXED_DELAY', 'exponential'));
    END IF;
END $$;

-- dispatchjob.ErrorType - internal/platform/dispatchjob/entity.go (partitioned parent; propagates to all partitions)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_dispatch_job_attempts_error_type'
    ) THEN
        ALTER TABLE msg_dispatch_job_attempts
            ADD CONSTRAINT chk_msg_dispatch_job_attempts_error_type
            CHECK (error_type IS NULL OR error_type IN ('CONNECTION', 'TIMEOUT', 'HTTP_ERROR', 'VALIDATION', 'UNKNOWN'));
    END IF;
END $$;

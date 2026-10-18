-- Rust 075 mirrors Go 049_partition_login_attempts.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Rebuild-and-swap of iam_login_attempts into a quarterly range-partitioned table. Self-guarded: a no-op once the table is partitioned. fc-platform-iam login_attempt/repository.rs already copes with both shapes.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Range-partition iam_login_attempts by quarter on attempted_at (owner
-- ruling X-03). It is a genuinely populated table in production (unlike
-- migrations 018/019/022, which assumed empty messaging tables and just
-- dropped/recreated) - this is a rebuild-and-swap: build the partitioned
-- replacement, copy every existing row into it, then swap names in.
--
-- PK becomes (id, attempted_at) - a partition key must be part of every
-- unique constraint. Local indexes match the two hot login-backoff reads
-- (internal/platform/loginattempt/loginattempt.go): the per-identifier
-- global ceiling (identifier, attempted_at) and the per-(identifier, IP)
-- backoff (identifier, ip_address, attempted_at); both queries already
-- carry an attempted_at lower bound, which is what makes partition pruning
-- effective for them. The previously existing single-column and throttle
-- indexes are preserved too, so the admin list/filter API and the
-- CountRecentFailures / FailureCountByIdentifierSince paths keep the plans
-- they had before.
--
-- Partitions cover every existing row (from its earliest quarter) through
-- the current quarter and the next, plus a DEFAULT partition to catch
-- anything outside that computed range (retention/forward maintenance from
-- here on is the housekeeping loop - see StartPurger in
-- internal/server/subsystems.go - which ensures next quarter's partition
-- ahead of time and drops partitions entirely older than the 3-year
-- retention; it never issues row DELETEs).
--
-- Idempotent: guarded by an explicit check that iam_login_attempts is
-- already partitioned, matching the guard style of 018/019/022.

DO $migration049$
DECLARE
    already_partitioned boolean;
    min_ts       TIMESTAMPTZ;
    upper_bound  TIMESTAMPTZ;
    q_start      TIMESTAMPTZ;
    partition_name TEXT;
BEGIN
    SELECT EXISTS (
        SELECT 1
        FROM pg_partitioned_table pt
        JOIN pg_class c ON c.oid = pt.partrelid
        WHERE c.relname = 'iam_login_attempts'
    ) INTO already_partitioned;

    IF already_partitioned THEN
        RAISE NOTICE 'Migration 049: iam_login_attempts is already partitioned; skipping.';
        RETURN;
    END IF;

    -- --- Build the partitioned replacement ---------------------------------
    CREATE TABLE iam_login_attempts_new (
        id             VARCHAR(17)  NOT NULL,
        attempt_type   VARCHAR(30)  NOT NULL,
        outcome        VARCHAR(20)  NOT NULL,
        failure_reason VARCHAR(100),
        identifier     VARCHAR(255),
        principal_id   VARCHAR(17),
        ip_address     VARCHAR(45),
        user_agent     TEXT,
        attempted_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        PRIMARY KEY (id, attempted_at)
    ) PARTITION BY RANGE (attempted_at);

    -- Working names to avoid colliding with the old table's indexes of the
    -- same name; renamed to the canonical names below once the old table
    -- (and its indexes) are gone. Index creation on a partitioned parent
    -- auto-propagates to every partition - existing and future.
    CREATE INDEX idx_iam_login_attempts_np_identifier_at
        ON iam_login_attempts_new (identifier, attempted_at);
    CREATE INDEX idx_iam_login_attempts_np_identifier_ip_at
        ON iam_login_attempts_new (identifier, ip_address, attempted_at);
    CREATE INDEX idx_iam_login_attempts_np_type
        ON iam_login_attempts_new (attempt_type);
    CREATE INDEX idx_iam_login_attempts_np_outcome
        ON iam_login_attempts_new (outcome);
    CREATE INDEX idx_iam_login_attempts_np_principal
        ON iam_login_attempts_new (principal_id);
    CREATE INDEX idx_iam_login_attempts_np_failure_throttle
        ON iam_login_attempts_new (identifier, attempted_at) WHERE outcome = 'FAILURE';
    -- attempted_at alone, for the admin list's unfiltered ORDER BY
    -- attempted_at DESC / DateFrom-DateTo range scan (loginattempt.FindPage).
    -- The bare identifier-only index from migration 008 is dropped as
    -- redundant - it's now a strict prefix of the composite above, same
    -- reasoning migration 037 used for the messaging-table singles.
    CREATE INDEX idx_iam_login_attempts_np_at
        ON iam_login_attempts_new (attempted_at);

    -- --- Quarterly partitions: earliest existing row through the current
    --     quarter + the next one -----------------------------------------
    SELECT date_trunc('quarter', COALESCE(MIN(attempted_at), NOW()))
      INTO min_ts
      FROM iam_login_attempts;

    upper_bound := date_trunc('quarter', NOW()) + INTERVAL '6 months'; -- exclusive end of "next quarter"
    q_start := min_ts;
    WHILE q_start < upper_bound LOOP
        partition_name := 'iam_login_attempts_' || to_char(q_start, 'YYYY') || '_q' || to_char(q_start, 'Q');
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF iam_login_attempts_new FOR VALUES FROM (%L) TO (%L)',
            partition_name, q_start, q_start + INTERVAL '3 months'
        );
        q_start := q_start + INTERVAL '3 months';
    END LOOP;

    CREATE TABLE IF NOT EXISTS iam_login_attempts_default
        PARTITION OF iam_login_attempts_new DEFAULT;

    -- --- Copy every existing row, then swap names in -----------------------
    INSERT INTO iam_login_attempts_new
        (id, attempt_type, outcome, failure_reason, identifier, principal_id,
         ip_address, user_agent, attempted_at)
    SELECT id, attempt_type, outcome, failure_reason, identifier, principal_id,
           ip_address, user_agent, attempted_at
      FROM iam_login_attempts;

    ALTER TABLE iam_login_attempts RENAME TO iam_login_attempts_old;
    ALTER TABLE iam_login_attempts_new RENAME TO iam_login_attempts;

    -- Renaming a table does NOT rename its indexes - iam_login_attempts_old
    -- still holds idx_iam_login_attempts_type/outcome/principal/at/
    -- failure_throttle at this point, AND the schema-global index name
    -- iam_login_attempts_pkey backing its primary key. So the old table must
    -- be gone before the working-named indexes above (and the new PK
    -- constraint, whose rename renames its backing index) can claim the
    -- canonical names.
    DROP TABLE iam_login_attempts_old;

    ALTER TABLE iam_login_attempts RENAME CONSTRAINT iam_login_attempts_new_pkey TO iam_login_attempts_pkey;

    ALTER INDEX idx_iam_login_attempts_np_identifier_at RENAME TO idx_iam_login_attempts_identifier_at;
    ALTER INDEX idx_iam_login_attempts_np_identifier_ip_at RENAME TO idx_iam_login_attempts_identifier_ip_at;
    ALTER INDEX idx_iam_login_attempts_np_type RENAME TO idx_iam_login_attempts_type;
    ALTER INDEX idx_iam_login_attempts_np_outcome RENAME TO idx_iam_login_attempts_outcome;
    ALTER INDEX idx_iam_login_attempts_np_principal RENAME TO idx_iam_login_attempts_principal;
    ALTER INDEX idx_iam_login_attempts_np_failure_throttle RENAME TO idx_iam_login_attempts_failure_throttle;
    ALTER INDEX idx_iam_login_attempts_np_at RENAME TO idx_iam_login_attempts_at;
END
$migration049$;

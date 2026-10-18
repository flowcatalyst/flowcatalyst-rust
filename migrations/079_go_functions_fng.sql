-- Rust 079 mirrors Go 059_functions.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- The function registry as Go models it: fng_functions, fng_versions, fng_aliases, fng_settings, fng_runners, fng_pool_revisions, plus msg_subscriptions.function_id, msg_scheduled_jobs.function_id and the FUNCTION subscription source. fng_* is the schema of record (parity plan D1); the fnr_* tables of 062 are left in place and unused by this migration.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Function runner platform schema - docs/function-runner-plan.md s.8.1
-- (WP3 of the phase-1 work packages, s.12.1). Six new tables plus two
-- widenings of existing tables so subscriptions and scheduled jobs can be
-- owned by a function.
--
-- The fng_ prefix is deliberate (owner decision, 2026-09-28): the Java,
-- Rust and Go platforms share databases and each has its own, incompatible
-- function-runner schema - Java keeps fn_, Rust uses fnr_, Go uses fng_ -
-- until one implementation is chosen and renamed back to fn_. Every table
-- here CREATEs IF NOT EXISTS, so a shared prefix would silently reuse
-- another implementation's table. The same goes for the fng_desired NOTIFY
-- channel.
--
-- fng_functions   - the aggregate root. Identity is the address
--                   `{applicationCode}.{name}` (two DNS labels, immutable,
--                   unique). application_id is a bare column, no FK - same
--                   convention as msg_subscriptions.dispatch_pool_id and
--                   msg_connections.application_code: every cross-aggregate
--                   reference in this schema is a plain column, not a FK,
--                   so aggregates can be migrated/reordered independently.
-- fng_versions    - immutable artifacts (sha256 digest) plus their describe
--                   document. Owned by fng_functions: FK ON DELETE CASCADE,
--                   the same pattern migrations 020/031/032/053 use for a
--                   true parent/child relationship (not a loose reference).
-- fng_aliases     - named pointers to a version (`live`, `canary`, ...). Owned
--                   by fng_functions: FK ON DELETE CASCADE.
-- fng_settings    - platform-held config/secret/DB values, keyed by
--                   (function_id, kind, key). Owned by fng_functions: FK ON
--                   DELETE CASCADE. SECRET and DB values are stored through
--                   the same encryption.EncryptSecretRef convention as
--                   iam_service_accounts.wh_*_ref (encrypted:<blob>, or an
--                   external secret-manager reference stored verbatim) -
--                   the `value` column holds ciphertext or a reference, never
--                   plaintext, for those two kinds.
-- fng_runners     - one row per live runner process, heartbeat + budget
--                   report. Not owned by any function.
-- fng_pool_revisions - one row per pool, bumped whenever that pool's desired
--                   state changes (promote, alias, settings). No FK: "pool"
--                   is a bare string key shared with msg_dispatch_pools.code,
--                   not every pool in this table need have a dispatch-pool
--                   row (a function's implied pool can outlive/precede one).
--
-- Status/kind values are TEXT with a CHECK constraint inline in the CREATE
-- TABLE (the tables are new in this migration, so there's no need for the
-- guarded ALTER ... ADD CONSTRAINT dance migration 051 uses for pre-existing
-- tables) - same X-06 discipline: an unrecognised value is a write-boundary
-- rejection, never silently coerced.

CREATE TABLE IF NOT EXISTS fng_functions (
    id             VARCHAR(17) PRIMARY KEY,
    application_id VARCHAR(17) NOT NULL,
    client_id      VARCHAR(17),
    name           VARCHAR(63) NOT NULL,
    address        VARCHAR(127) NOT NULL,
    description    TEXT,
    pool           VARCHAR(100),
    warm           BOOLEAN NOT NULL DEFAULT FALSE,
    limits         JSONB NOT NULL,
    created_by     VARCHAR(17),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_fng_functions_address ON fng_functions (address);
CREATE INDEX IF NOT EXISTS idx_fng_functions_application_id ON fng_functions (application_id);
CREATE INDEX IF NOT EXISTS idx_fng_functions_client_id ON fng_functions (client_id);

CREATE TABLE IF NOT EXISTS fng_versions (
    id           VARCHAR(17) PRIMARY KEY,
    function_id  VARCHAR(17) NOT NULL REFERENCES fng_functions(id) ON DELETE CASCADE,
    number       INTEGER NOT NULL,
    digest       VARCHAR(64) NOT NULL,
    size_bytes   BIGINT NOT NULL,
    abi          INTEGER NOT NULL,
    describe     JSONB NOT NULL,
    status       VARCHAR(20) NOT NULL DEFAULT 'PUBLISHED'
        CHECK (status IN ('PUBLISHED', 'READY', 'FAILED', 'RETIRED')),
    failure      JSONB,
    ready_at     TIMESTAMPTZ,
    published_by VARCHAR(17),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_fng_versions_function_number ON fng_versions (function_id, number);
CREATE UNIQUE INDEX IF NOT EXISTS uq_fng_versions_function_digest ON fng_versions (function_id, digest);
CREATE INDEX IF NOT EXISTS idx_fng_versions_function_id ON fng_versions (function_id);
CREATE INDEX IF NOT EXISTS idx_fng_versions_status ON fng_versions (status);

CREATE TABLE IF NOT EXISTS fng_aliases (
    function_id VARCHAR(17) NOT NULL REFERENCES fng_functions(id) ON DELETE CASCADE,
    name        VARCHAR(63) NOT NULL,
    version_id  VARCHAR(17) NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by  VARCHAR(17),
    PRIMARY KEY (function_id, name)
);

CREATE INDEX IF NOT EXISTS idx_fng_aliases_version_id ON fng_aliases (version_id);

CREATE TABLE IF NOT EXISTS fng_settings (
    function_id VARCHAR(17) NOT NULL REFERENCES fng_functions(id) ON DELETE CASCADE,
    kind        VARCHAR(20) NOT NULL CHECK (kind IN ('CONFIG', 'SECRET', 'DB')),
    key         VARCHAR(100) NOT NULL,
    value       TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (function_id, kind, key)
);

CREATE INDEX IF NOT EXISTS idx_fng_settings_function_id ON fng_settings (function_id);

CREATE TABLE IF NOT EXISTS fng_runners (
    id           VARCHAR(17) PRIMARY KEY,
    pool         VARCHAR(100) NOT NULL,
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    report       JSONB
);

CREATE INDEX IF NOT EXISTS idx_fng_runners_pool ON fng_runners (pool);

CREATE TABLE IF NOT EXISTS fng_pool_revisions (
    pool     VARCHAR(100) PRIMARY KEY,
    revision BIGINT NOT NULL DEFAULT 0
);

-- msg_subscriptions: a subscription may be owned by a function (source
-- FUNCTION), same shape as the existing CODE/API/UI authorship values
-- (migration 051's chk_msg_subscriptions_source). function_id is a bare
-- column, no FK - same convention as dispatch_pool_id on this table.
ALTER TABLE msg_subscriptions ADD COLUMN IF NOT EXISTS function_id VARCHAR(17);
CREATE INDEX IF NOT EXISTS idx_msg_subscriptions_function_id ON msg_subscriptions (function_id);

ALTER TABLE msg_subscriptions DROP CONSTRAINT IF EXISTS chk_msg_subscriptions_source;
ALTER TABLE msg_subscriptions
    ADD CONSTRAINT chk_msg_subscriptions_source
    CHECK (source IN ('CODE', 'API', 'UI', 'FUNCTION'));

-- msg_scheduled_jobs (the scheduled-job DEFINITION table - see migration
-- 021's "Definitions" section; msg_scheduled_job_instances is the per-firing
-- history table and is untouched here) gains the same optional function
-- ownership link.
ALTER TABLE msg_scheduled_jobs ADD COLUMN IF NOT EXISTS function_id VARCHAR(17);
CREATE INDEX IF NOT EXISTS idx_msg_scheduled_jobs_function_id ON msg_scheduled_jobs (function_id);

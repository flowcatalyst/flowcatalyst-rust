-- Function registry: Java's V13__functions.sql + V15__fn_routes_alias_prefixes.sql
-- − V16__fn_domains_no_verification.sql (../flowcatalyst-javalin at 0118cdca,
-- server/src/main/resources/db/migration/). The management interface for
-- functions mirrors Java exactly, so every table, column, CHECK, unique, index
-- and FK below carries Java's name and definition.
--
-- The tables are created in their net shape: fn_routes already has V15's
-- alias_prefixes, and fn_domains never had V16's dropped verification
-- columns. V15's ADD COLUMN and V16's DROP COLUMN are repeated at the end,
-- both idempotent, so a database Java migrated only to V13 or V15 still ends
-- up with the same shape.
--
-- Go has no function service, so none of this has a goose counterpart. The one
-- change to a Go-shared table is msg_subscriptions.source gaining FUNCTION.

-- fn_functions: one row per app.service.name address. application_code is a
-- copy of the owning application's immutable code, so a lookup by address can
-- check all three segments without a join; no FK leaves the fn_ family.
-- client_id NULL means a platform-owned function.
CREATE TABLE IF NOT EXISTS fn_functions (
    id VARCHAR(17) NOT NULL,
    application_id VARCHAR(17) NOT NULL,
    application_code VARCHAR(63) NOT NULL,
    service_name VARCHAR(63) NOT NULL,
    name VARCHAR(63) NOT NULL,
    client_id VARCHAR(17),
    runtime VARCHAR(10) NOT NULL,
    description VARCHAR(1000),
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_functions_pkey PRIMARY KEY (id),
    CONSTRAINT fn_functions_application_code_check CHECK (application_code ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fn_functions_service_name_check CHECK (service_name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fn_functions_name_check CHECK (name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fn_functions_runtime_check CHECK (runtime IN ('JVM', 'WASM')),
    CONSTRAINT fn_functions_status_check CHECK (status IN ('ACTIVE', 'DISABLED')),
    CONSTRAINT fn_functions_application_id_service_name_name_key UNIQUE (application_id, service_name, name),
    CONSTRAINT fn_functions_application_code_service_name_name_key UNIQUE (application_code, service_name, name)
);

-- fn_versions: an immutable published artifact of a function. The signer and
-- bundle columns are nullable because dev runs with signatures off. Deleting a
-- function deletes its versions.
CREATE TABLE IF NOT EXISTS fn_versions (
    id VARCHAR(17) NOT NULL,
    function_id VARCHAR(17) NOT NULL,
    version INT NOT NULL,
    artifact_ref VARCHAR(1000) NOT NULL,
    digest VARCHAR(71) NOT NULL,
    signature_bundle TEXT,
    signature_bundle_ref VARCHAR(1000),
    signer_issuer VARCHAR(500),
    signer_subject VARCHAR(1000),
    manifest JSONB NOT NULL,
    state VARCHAR(20) NOT NULL,
    published_by VARCHAR(17) NOT NULL,
    published_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ready_at TIMESTAMPTZ,
    retired_at TIMESTAMPTZ,
    CONSTRAINT fn_versions_pkey PRIMARY KEY (id),
    CONSTRAINT fn_versions_function_id_fkey FOREIGN KEY (function_id) REFERENCES fn_functions (id) ON DELETE CASCADE,
    CONSTRAINT fn_versions_version_check CHECK (version > 0),
    CONSTRAINT fn_versions_digest_check CHECK (digest ~ '^sha256:[0-9a-f]{64}$'),
    CONSTRAINT fn_versions_state_check CHECK (state IN ('PUBLISHED', 'READY', 'RETIRED')),
    CONSTRAINT fn_versions_ready_at_check CHECK (state <> 'READY' OR ready_at IS NOT NULL),
    CONSTRAINT fn_versions_retired_at_check CHECK (state <> 'RETIRED' OR retired_at IS NOT NULL),
    CONSTRAINT fn_versions_function_id_version_key UNIQUE (function_id, version),
    CONSTRAINT fn_versions_function_id_digest_key UNIQUE (function_id, digest)
);

CREATE INDEX IF NOT EXISTS idx_fn_versions_function_id_state ON fn_versions (function_id, state);

-- fn_aliases: a mutable named pointer (e.g. `live`) from a function to one of
-- its versions. Natural key, no TSID. Both FKs cascade.
CREATE TABLE IF NOT EXISTS fn_aliases (
    function_id VARCHAR(17) NOT NULL,
    alias VARCHAR(63) NOT NULL,
    version_id VARCHAR(17) NOT NULL,
    updated_by VARCHAR(17) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_aliases_pkey PRIMARY KEY (function_id, alias),
    CONSTRAINT fn_aliases_function_id_fkey FOREIGN KEY (function_id) REFERENCES fn_functions (id) ON DELETE CASCADE,
    CONSTRAINT fn_aliases_alias_check CHECK (alias ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fn_aliases_version_id_fkey FOREIGN KEY (version_id) REFERENCES fn_versions (id) ON DELETE CASCADE
);

-- fn_hosts: a running function host registers itself under its own id.
-- Natural key, no TSID.
CREATE TABLE IF NOT EXISTS fn_hosts (
    id VARCHAR(100) NOT NULL,
    pool VARCHAR(63) NOT NULL,
    state VARCHAR(20) NOT NULL,
    loaded JSONB NOT NULL DEFAULT '[]',
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_hosts_pkey PRIMARY KEY (id),
    CONSTRAINT fn_hosts_pool_check CHECK (pool ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fn_hosts_state_check CHECK (state IN ('ACTIVE', 'DRAINING'))
);

CREATE INDEX IF NOT EXISTS idx_fn_hosts_pool_last_heartbeat ON fn_hosts (pool, last_heartbeat);

-- fn_client_policies: one row per client, holding allowed signers and
-- per-client ceilings. client_id also accepts the reserved value 'PLATFORM'
-- for the platform-owned functions' policy (a primary key cannot be NULL, and
-- no TSID is ever 'PLATFORM').
CREATE TABLE IF NOT EXISTS fn_client_policies (
    client_id VARCHAR(17) NOT NULL,
    signers JSONB NOT NULL DEFAULT '[]',
    max_duration_ms INT,
    max_concurrency INT,
    max_wasm_memory_mb INT,
    max_db_pool_size INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_client_policies_pkey PRIMARY KEY (client_id),
    CONSTRAINT fn_client_policies_max_duration_ms_check CHECK (max_duration_ms IS NULL OR max_duration_ms > 0),
    CONSTRAINT fn_client_policies_max_concurrency_check CHECK (max_concurrency IS NULL OR max_concurrency > 0),
    CONSTRAINT fn_client_policies_max_wasm_memory_mb_check CHECK (max_wasm_memory_mb IS NULL OR max_wasm_memory_mb > 0),
    CONSTRAINT fn_client_policies_max_db_pool_size_check CHECK (max_db_pool_size IS NULL OR max_db_pool_size > 0)
);

-- fn_domains: a claimed hostname that may carry public fn_routes. client_id
-- NULL means the platform's domain. A claim is verified by being made (Java
-- V16), so there are no verification columns.
CREATE TABLE IF NOT EXISTS fn_domains (
    id VARCHAR(17) NOT NULL,
    client_id VARCHAR(17),
    hostname VARCHAR(253) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_domains_pkey PRIMARY KEY (id),
    CONSTRAINT fn_domains_hostname_check CHECK (hostname = lower(hostname)),
    CONSTRAINT fn_domains_hostname_key UNIQUE (hostname)
);

CREATE INDEX IF NOT EXISTS idx_fn_domains_client_id ON fn_domains (client_id);

-- fn_routes: public (hostname, path_prefix) pairs and the function they
-- resolve to. A private call needs no route row, so hostname is NOT NULL.
-- Unique across every function: two functions cannot claim one public
-- prefix. alias_prefixes (Java V15) are the opt-in DNS-label prefixes copied
-- from the published manifest's public[].aliasPrefixes; empty means an exact
-- hostname match only.
CREATE TABLE IF NOT EXISTS fn_routes (
    id VARCHAR(17) NOT NULL,
    function_id VARCHAR(17) NOT NULL,
    hostname VARCHAR(253) NOT NULL,
    path_prefix VARCHAR(1024) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    alias_prefixes TEXT[] NOT NULL DEFAULT '{}',
    CONSTRAINT fn_routes_pkey PRIMARY KEY (id),
    CONSTRAINT fn_routes_function_id_fkey FOREIGN KEY (function_id) REFERENCES fn_functions (id) ON DELETE CASCADE,
    CONSTRAINT fn_routes_hostname_path_prefix_key UNIQUE (hostname, path_prefix)
);

CREATE INDEX IF NOT EXISTS idx_fn_routes_function_id ON fn_routes (function_id);

-- fn_trigger_objects: the platform-managed objects (dispatch pool,
-- subscriptions, scheduled jobs) a function's live manifest created at
-- promote, so a later promote can reconcile them and the SDK syncs can tell a
-- function-owned object from an application-owned one. kind is POOL,
-- SUBSCRIPTION or SCHEDULED_JOB; object_id is unique per kind.
CREATE TABLE IF NOT EXISTS fn_trigger_objects (
    function_id VARCHAR(17) NOT NULL,
    kind VARCHAR(20) NOT NULL,
    object_id VARCHAR(17) NOT NULL,
    trigger_key VARCHAR(200) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_trigger_objects_pkey PRIMARY KEY (function_id, kind, trigger_key),
    CONSTRAINT fn_trigger_objects_function_id_fkey FOREIGN KEY (function_id) REFERENCES fn_functions (id) ON DELETE CASCADE,
    CONSTRAINT fn_trigger_objects_kind_check CHECK (kind IN ('POOL', 'SUBSCRIPTION', 'SCHEDULED_JOB')),
    CONSTRAINT fn_trigger_objects_kind_object_id_key UNIQUE (kind, object_id)
);

CREATE INDEX IF NOT EXISTS idx_fn_trigger_objects_function_id ON fn_trigger_objects (function_id);

-- fn_config: per-function, per-key config values. A value outlives a deploy;
-- a version only lists the keys it needs. key follows the SettingKey rule.
CREATE TABLE IF NOT EXISTS fn_config (
    function_id VARCHAR(17) NOT NULL,
    key VARCHAR(100) NOT NULL,
    value TEXT NOT NULL,
    updated_by VARCHAR(17) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_config_pkey PRIMARY KEY (function_id, key),
    CONSTRAINT fn_config_function_id_fkey FOREIGN KEY (function_id) REFERENCES fn_functions (id) ON DELETE CASCADE,
    CONSTRAINT fn_config_key_check CHECK (key ~ '^[A-Za-z][A-Za-z0-9_./-]{0,99}$')
);

-- fn_secrets: per-function, per-key secret values. value_ref is the
-- `encrypted:` form, never plaintext at rest.
CREATE TABLE IF NOT EXISTS fn_secrets (
    function_id VARCHAR(17) NOT NULL,
    key VARCHAR(100) NOT NULL,
    value_ref TEXT NOT NULL,
    updated_by VARCHAR(17) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fn_secrets_pkey PRIMARY KEY (function_id, key),
    CONSTRAINT fn_secrets_function_id_fkey FOREIGN KEY (function_id) REFERENCES fn_functions (id) ON DELETE CASCADE,
    CONSTRAINT fn_secrets_key_check CHECK (key ~ '^[A-Za-z][A-Za-z0-9_./-]{0,99}$')
);

-- msg_subscriptions.source widened to admit FUNCTION: a function's
-- subscription is never touched by an application SDK's removeUnlisted sync.
-- Java's statement verbatim: drop-then-add, so a database that has Java's (or
-- Go's) narrower chk_msg_subscriptions_source ends up widened, and a Rust
-- database, which had no CHECK on this column, gains Java's.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_subscriptions_source'
    ) THEN
        ALTER TABLE msg_subscriptions DROP CONSTRAINT chk_msg_subscriptions_source;
    END IF;
    ALTER TABLE msg_subscriptions
        ADD CONSTRAINT chk_msg_subscriptions_source
        CHECK (source IN ('CODE', 'API', 'UI', 'FUNCTION'));
END $$;

-- Java V15, for a database Java migrated to V13 only.
ALTER TABLE fn_routes
    ADD COLUMN IF NOT EXISTS alias_prefixes TEXT[] NOT NULL DEFAULT '{}';

-- Java V16, for a database Java migrated to V13 or V15 only.
ALTER TABLE fn_domains
    DROP COLUMN IF EXISTS verification_token,
    DROP COLUMN IF EXISTS verified_at;

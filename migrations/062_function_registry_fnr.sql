-- Owner decision #48 (2026-09-28): the Rust platform's function registry
-- moves from `fn_*` to `fnr_*`, as a temporary measure until the owner picks
-- one implementation.
--
-- Go (its migration 059_functions) now has a function runner of its own,
-- with `fn_functions`, `fn_versions`, `fn_aliases`, `fn_settings`,
-- `fn_runners` and `fn_pool_revisions`, whose columns are incompatible with
-- Java's and Rust's. Every platform creates its tables with CREATE TABLE IF
-- NOT EXISTS, so on a shared database the first platform to migrate wins
-- and the other fails later ("column state does not exist"). Each
-- implementation now has its own prefix: Java keeps `fn_`, Rust uses
-- `fnr_`, Go uses `fng_`.
--
-- This creates the end state of Rust's former 034_functions (Java's V13 +
-- V15 - V16), 037_function_component_runtime and 056_function_js_runtime,
-- with every table, constraint and index renamed `fn_` -> `fnr_`. The runner
-- retires those three migrations (`RETIRED_MIGRATIONS` in
-- `crates/fc-platform/src/shared/database.rs`): they never run again, so
-- Rust no longer creates or alters any `fn_*` table, which on a given
-- database may be Java's or Go's. Rust leaves every `fn_*` table alone,
-- including ones its own 034 created on a dev database; no row is copied
-- from them, so functions registered on such a database are published
-- again.
--
-- The tables are created in their net shape: fnr_routes has Java V15's
-- alias_prefixes and fnr_domains never had V16's verification columns, the
-- runtime CHECK already admits COMPONENT (037) and JS (056), and fnr_hosts
-- already has 037's runtimes column. No FK leaves the fnr_ family.

-- fnr_functions: one row per app.service.name address. application_code is a
-- copy of the owning application's immutable code, so a lookup by address can
-- check all three segments without a join. client_id NULL means a
-- platform-owned function.
CREATE TABLE IF NOT EXISTS fnr_functions (
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
    CONSTRAINT fnr_functions_pkey PRIMARY KEY (id),
    CONSTRAINT fnr_functions_application_code_check CHECK (application_code ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fnr_functions_service_name_check CHECK (service_name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fnr_functions_name_check CHECK (name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fnr_functions_runtime_check CHECK (runtime IN ('JVM', 'WASM', 'COMPONENT', 'JS')),
    CONSTRAINT fnr_functions_status_check CHECK (status IN ('ACTIVE', 'DISABLED')),
    CONSTRAINT fnr_functions_application_id_service_name_name_key UNIQUE (application_id, service_name, name),
    CONSTRAINT fnr_functions_application_code_service_name_name_key UNIQUE (application_code, service_name, name)
);

-- fnr_versions: an immutable published artifact of a function. The signer
-- and bundle columns are nullable because dev runs with signatures off.
-- Deleting a function deletes its versions.
CREATE TABLE IF NOT EXISTS fnr_versions (
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
    CONSTRAINT fnr_versions_pkey PRIMARY KEY (id),
    CONSTRAINT fnr_versions_function_id_fkey FOREIGN KEY (function_id) REFERENCES fnr_functions (id) ON DELETE CASCADE,
    CONSTRAINT fnr_versions_version_check CHECK (version > 0),
    CONSTRAINT fnr_versions_digest_check CHECK (digest ~ '^sha256:[0-9a-f]{64}$'),
    CONSTRAINT fnr_versions_state_check CHECK (state IN ('PUBLISHED', 'READY', 'RETIRED')),
    CONSTRAINT fnr_versions_ready_at_check CHECK (state <> 'READY' OR ready_at IS NOT NULL),
    CONSTRAINT fnr_versions_retired_at_check CHECK (state <> 'RETIRED' OR retired_at IS NOT NULL),
    CONSTRAINT fnr_versions_function_id_version_key UNIQUE (function_id, version),
    CONSTRAINT fnr_versions_function_id_digest_key UNIQUE (function_id, digest)
);

CREATE INDEX IF NOT EXISTS idx_fnr_versions_function_id_state ON fnr_versions (function_id, state);

-- fnr_aliases: a mutable named pointer (e.g. `live`) from a function to one
-- of its versions. Natural key, no TSID. Both FKs cascade.
CREATE TABLE IF NOT EXISTS fnr_aliases (
    function_id VARCHAR(17) NOT NULL,
    alias VARCHAR(63) NOT NULL,
    version_id VARCHAR(17) NOT NULL,
    updated_by VARCHAR(17) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fnr_aliases_pkey PRIMARY KEY (function_id, alias),
    CONSTRAINT fnr_aliases_function_id_fkey FOREIGN KEY (function_id) REFERENCES fnr_functions (id) ON DELETE CASCADE,
    CONSTRAINT fnr_aliases_alias_check CHECK (alias ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fnr_aliases_version_id_fkey FOREIGN KEY (version_id) REFERENCES fnr_versions (id) ON DELETE CASCADE
);

-- fnr_hosts: a running function host registers itself under its own id.
-- Natural key, no TSID. runtimes: the runtimes the host says it can load,
-- from its heartbeat (lower-case manifest spellings, e.g.
-- ["component","js","wasm"]); NULL when the host did not say (Java's hosts),
-- and publish then assumes nothing about the pool.
CREATE TABLE IF NOT EXISTS fnr_hosts (
    id VARCHAR(100) NOT NULL,
    pool VARCHAR(63) NOT NULL,
    state VARCHAR(20) NOT NULL,
    loaded JSONB NOT NULL DEFAULT '[]',
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    runtimes JSONB,
    CONSTRAINT fnr_hosts_pkey PRIMARY KEY (id),
    CONSTRAINT fnr_hosts_pool_check CHECK (pool ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    CONSTRAINT fnr_hosts_state_check CHECK (state IN ('ACTIVE', 'DRAINING'))
);

CREATE INDEX IF NOT EXISTS idx_fnr_hosts_pool_last_heartbeat ON fnr_hosts (pool, last_heartbeat);

-- fnr_client_policies: one row per client, holding allowed signers and
-- per-client ceilings. client_id also accepts the reserved value 'PLATFORM'
-- for the platform-owned functions' policy (a primary key cannot be NULL,
-- and no TSID is ever 'PLATFORM').
CREATE TABLE IF NOT EXISTS fnr_client_policies (
    client_id VARCHAR(17) NOT NULL,
    signers JSONB NOT NULL DEFAULT '[]',
    max_duration_ms INT,
    max_concurrency INT,
    max_wasm_memory_mb INT,
    max_db_pool_size INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fnr_client_policies_pkey PRIMARY KEY (client_id),
    CONSTRAINT fnr_client_policies_max_duration_ms_check CHECK (max_duration_ms IS NULL OR max_duration_ms > 0),
    CONSTRAINT fnr_client_policies_max_concurrency_check CHECK (max_concurrency IS NULL OR max_concurrency > 0),
    CONSTRAINT fnr_client_policies_max_wasm_memory_mb_check CHECK (max_wasm_memory_mb IS NULL OR max_wasm_memory_mb > 0),
    CONSTRAINT fnr_client_policies_max_db_pool_size_check CHECK (max_db_pool_size IS NULL OR max_db_pool_size > 0)
);

-- fnr_domains: a claimed hostname that may carry public fnr_routes.
-- client_id NULL means the platform's domain. A claim is verified by being
-- made (Java V16), so there are no verification columns.
CREATE TABLE IF NOT EXISTS fnr_domains (
    id VARCHAR(17) NOT NULL,
    client_id VARCHAR(17),
    hostname VARCHAR(253) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fnr_domains_pkey PRIMARY KEY (id),
    CONSTRAINT fnr_domains_hostname_check CHECK (hostname = lower(hostname)),
    CONSTRAINT fnr_domains_hostname_key UNIQUE (hostname)
);

CREATE INDEX IF NOT EXISTS idx_fnr_domains_client_id ON fnr_domains (client_id);

-- fnr_routes: public (hostname, path_prefix) pairs and the function they
-- resolve to. A private call needs no route row, so hostname is NOT NULL.
-- Unique across every function: two functions cannot claim one public
-- prefix. alias_prefixes (Java V15) are the opt-in DNS-label prefixes copied
-- from the published manifest's public[].aliasPrefixes; empty means an exact
-- hostname match only.
CREATE TABLE IF NOT EXISTS fnr_routes (
    id VARCHAR(17) NOT NULL,
    function_id VARCHAR(17) NOT NULL,
    hostname VARCHAR(253) NOT NULL,
    path_prefix VARCHAR(1024) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    alias_prefixes TEXT[] NOT NULL DEFAULT '{}',
    CONSTRAINT fnr_routes_pkey PRIMARY KEY (id),
    CONSTRAINT fnr_routes_function_id_fkey FOREIGN KEY (function_id) REFERENCES fnr_functions (id) ON DELETE CASCADE,
    CONSTRAINT fnr_routes_hostname_path_prefix_key UNIQUE (hostname, path_prefix)
);

CREATE INDEX IF NOT EXISTS idx_fnr_routes_function_id ON fnr_routes (function_id);

-- fnr_trigger_objects: the platform-managed objects (dispatch pool,
-- subscriptions, scheduled jobs) a function's live manifest created at
-- promote, so a later promote can reconcile them and the SDK syncs can tell
-- a function-owned object from an application-owned one. kind is POOL,
-- SUBSCRIPTION or SCHEDULED_JOB; object_id is unique per kind.
CREATE TABLE IF NOT EXISTS fnr_trigger_objects (
    function_id VARCHAR(17) NOT NULL,
    kind VARCHAR(20) NOT NULL,
    object_id VARCHAR(17) NOT NULL,
    trigger_key VARCHAR(200) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fnr_trigger_objects_pkey PRIMARY KEY (function_id, kind, trigger_key),
    CONSTRAINT fnr_trigger_objects_function_id_fkey FOREIGN KEY (function_id) REFERENCES fnr_functions (id) ON DELETE CASCADE,
    CONSTRAINT fnr_trigger_objects_kind_check CHECK (kind IN ('POOL', 'SUBSCRIPTION', 'SCHEDULED_JOB')),
    CONSTRAINT fnr_trigger_objects_kind_object_id_key UNIQUE (kind, object_id)
);

CREATE INDEX IF NOT EXISTS idx_fnr_trigger_objects_function_id ON fnr_trigger_objects (function_id);

-- fnr_config: per-function, per-key config values. A value outlives a
-- deploy; a version only lists the keys it needs. key follows the SettingKey
-- rule.
CREATE TABLE IF NOT EXISTS fnr_config (
    function_id VARCHAR(17) NOT NULL,
    key VARCHAR(100) NOT NULL,
    value TEXT NOT NULL,
    updated_by VARCHAR(17) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fnr_config_pkey PRIMARY KEY (function_id, key),
    CONSTRAINT fnr_config_function_id_fkey FOREIGN KEY (function_id) REFERENCES fnr_functions (id) ON DELETE CASCADE,
    CONSTRAINT fnr_config_key_check CHECK (key ~ '^[A-Za-z][A-Za-z0-9_./-]{0,99}$')
);

-- fnr_secrets: per-function, per-key secret values. value_ref is the
-- `encrypted:` form, never plaintext at rest.
CREATE TABLE IF NOT EXISTS fnr_secrets (
    function_id VARCHAR(17) NOT NULL,
    key VARCHAR(100) NOT NULL,
    value_ref TEXT NOT NULL,
    updated_by VARCHAR(17) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fnr_secrets_pkey PRIMARY KEY (function_id, key),
    CONSTRAINT fnr_secrets_function_id_fkey FOREIGN KEY (function_id) REFERENCES fnr_functions (id) ON DELETE CASCADE,
    CONSTRAINT fnr_secrets_key_check CHECK (key ~ '^[A-Za-z][A-Za-z0-9_./-]{0,99}$')
);

-- msg_subscriptions.source admits FUNCTION: a function's subscription is
-- never touched by an application SDK's removeUnlisted sync. The retired 034
-- did this; a database that ran it, or Go's 059, or Java's V13, already
-- admits FUNCTION and is left as it is (so a CHECK another platform widened
-- further is never narrowed). Otherwise the CHECK is (re)created: a Go
-- database before its 059 has CODE/API/UI only, a fresh Rust database has
-- none.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_msg_subscriptions_source'
          AND pg_get_constraintdef(oid) LIKE '%''FUNCTION''%'
    ) THEN
        ALTER TABLE msg_subscriptions DROP CONSTRAINT IF EXISTS chk_msg_subscriptions_source;
        ALTER TABLE msg_subscriptions
            ADD CONSTRAINT chk_msg_subscriptions_source
            CHECK (source IN ('CODE', 'API', 'UI', 'FUNCTION'));
    END IF;
END $$;

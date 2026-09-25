-- Owner decision 5 (2026-09-25): the function contract gains an explicit
-- `runtime: component` (a WASI 0.2 component exporting
-- wasi:http/incoming-handler), alongside Java's `jvm` and `wasm`.
--
-- 036 is taken by the scheduler's cron-dialect rewrite
-- (036_scheduled_job_cron_dialect, a Rust migration), so this is 037.

ALTER TABLE fn_functions DROP CONSTRAINT IF EXISTS fn_functions_runtime_check;
ALTER TABLE fn_functions ADD CONSTRAINT fn_functions_runtime_check
    CHECK (runtime IN ('JVM', 'WASM', 'COMPONENT'));

-- The runtimes a host says it can load, from its heartbeat (lower-case
-- manifest spellings, e.g. ["component","wasm"]). NULL: the host did not
-- say (Java's hosts, and Rust hosts before this change); publish then
-- assumes nothing about the pool.
ALTER TABLE fn_hosts ADD COLUMN IF NOT EXISTS runtimes JSONB;

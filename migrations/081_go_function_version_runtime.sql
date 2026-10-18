-- Rust 081 mirrors Go 061_function_version_runtime.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- fng_versions.runtime (wasm | js) with its CHECK.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Function versions declare which runtime they run on
-- (docs/function-runner-plan.md s.9, WP4 of the phase-1 work packages,
-- s.12.1): "wasm" (an ABI v1 module, compiled as is) or "js" (a bundled
-- script run on the shared JS engine, internal/functions/runtimes). Every
-- version published before this migration is a wasm module, hence the
-- DEFAULT - no backfill needed.
ALTER TABLE fng_versions ADD COLUMN IF NOT EXISTS runtime TEXT NOT NULL DEFAULT 'wasm';
ALTER TABLE fng_versions DROP CONSTRAINT IF EXISTS chk_fng_versions_runtime;
ALTER TABLE fng_versions
    ADD CONSTRAINT chk_fng_versions_runtime
    CHECK (runtime IN ('wasm', 'js'));

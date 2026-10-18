-- Rust 082 mirrors Go 062_function_routes.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- fng_domains and fng_routes.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Function runner public routes - docs/function-runner-plan.md s.8 (public
-- routes: domain claims and route materialisation, phase-1 follow-on to
-- migration 059). Two new tables, both fng_ (owner decision, migration
-- 059's header: the Go platform's function-runner schema stays fng_ until
-- one implementation is chosen and renamed back to fn_).
--
-- fng_domains - a claimed hostname ZONE. A claim is verified by being
--               made (no DNS step, plan owner ruling); it covers the zone
--               itself and every hostname under it. client_id NULL means a
--               platform/anchor-owned zone. A zone may not be claimed if an
--               existing claim by a DIFFERENT owner covers it, or is
--               covered by it (checked in the operations layer, not by a
--               constraint - "covers" is a suffix relationship SQL can't
--               express as a simple CHECK/unique index).
-- fng_routes  - one public route: (hostname, path_prefix) -> a function's
--               live version, or a named alias. Owned by fng_functions: FK
--               ON DELETE CASCADE, same convention as fng_versions/
--               fng_aliases/fng_settings (migration 059). UNIQUE
--               (hostname, path_prefix) across ALL functions - a route
--               claims one URL space regardless of which function owns it.

CREATE TABLE IF NOT EXISTS fng_domains (
    id         VARCHAR(17) PRIMARY KEY,
    zone       VARCHAR(255) NOT NULL,
    client_id  VARCHAR(17),
    created_by VARCHAR(17),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_fng_domains_zone ON fng_domains (zone);
CREATE INDEX IF NOT EXISTS idx_fng_domains_client_id ON fng_domains (client_id);

CREATE TABLE IF NOT EXISTS fng_routes (
    id          VARCHAR(17) PRIMARY KEY,
    function_id VARCHAR(17) NOT NULL REFERENCES fng_functions(id) ON DELETE CASCADE,
    hostname    VARCHAR(255) NOT NULL,
    path_prefix VARCHAR(255) NOT NULL,
    alias       VARCHAR(63),
    created_by  VARCHAR(17),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_fng_routes_host_prefix ON fng_routes (hostname, path_prefix);
CREATE INDEX IF NOT EXISTS idx_fng_routes_function_id ON fng_routes (function_id);

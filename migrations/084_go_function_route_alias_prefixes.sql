-- Rust 084 mirrors Go 064_function_route_alias_prefixes.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- fng_routes.alias_prefixes.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Alias prefixes on a public route: a route may opt into serving named
-- aliases from prefixed hostnames, so `qa-myapp.acme.com` serves the `qa`
-- alias of the function that owns the route on `myapp.acme.com`. Empty (the
-- default) means exact-host matching only - behaviour before this column.
-- Derived hostnames are not stored; the runner derives them at match time.
ALTER TABLE fng_routes ADD COLUMN IF NOT EXISTS alias_prefixes TEXT[] NOT NULL DEFAULT '{}';

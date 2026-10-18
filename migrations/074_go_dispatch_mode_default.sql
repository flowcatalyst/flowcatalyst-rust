-- Rust 074 mirrors Go 048_dispatch_mode_default.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Column defaults only: an unspecified dispatch mode means NEXT_ON_ERROR. Rows keep the mode they stored.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Unspecified dispatch mode now means NEXT_ON_ERROR, not IMMEDIATE.
--
-- IMMEDIATE is the only mode with no ordering at all, so an omitted mode
-- silently discarded sequencing - and only visibly under load, where concurrent
-- dispatch actually interleaves. Ordering is cheap to opt out of and expensive
-- to discover you never had, so the default now keeps a message group in
-- sequence and moves on past a failure.
--
-- Column defaults only. Rows that already say IMMEDIATE keep saying it: this
-- changes what "unspecified" means from here on, not anyone's stored choice.
-- (Ungrouped traffic is unaffected in practice - the router dispatches a
-- message with no message group concurrently whatever its mode, because
-- ordering is only defined relative to a group.)

-- Only the two tables where "unspecified" is a real input path.
-- msg_dispatch_jobs_read is a projection whose writer always supplies the mode,
-- so it is left without one rather than given a default it can never use.
ALTER TABLE msg_subscriptions ALTER COLUMN mode SET DEFAULT 'NEXT_ON_ERROR';
ALTER TABLE msg_dispatch_jobs ALTER COLUMN mode SET DEFAULT 'NEXT_ON_ERROR';

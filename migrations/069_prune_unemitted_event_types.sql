-- Go's migration 071 (flowcatalyst-go 45537af), its Up section, with this
-- platform's set of codes: the runner here applies migrations forward only.
-- It is idempotent, so a database Go already migrated past 071 runs it as a
-- no-op (Go's set is a superset of this one).
--
-- Older seeds catalogued 15 platform event types that no use case emits
-- since cb1b3e4f (2026-09-25), when the events took Go's codes: the emitted
-- equivalents are platform:admin:role:*, platform:admin:roles:synced,
-- platform:admin:anchor-domain:*, platform:admin:auth-config:*,
-- platform:admin:identity-provider:* and platform:admin:email-domain-mapping:*.
-- The seed no longer lists them; this removes the rows an older seed wrote,
-- with their schema versions. (Go also prunes
-- platform:iam:user:password-reset-requested, which this platform emits.)
--
-- A subscription binding is a code pattern matched against each event's type.
-- A binding naming one of these codes exactly, or pointing at one of these
-- rows, matches nothing any more. Such bindings are moved to
-- msg_subscription_event_types_archive (so what the subscriber picked is kept)
-- rather than left naming a type that no longer exists, and are not
-- re-pointed at the emitted equivalent: that would start deliveries, with a
-- different payload, nobody asked for. A wildcard binding
-- (platform:iam:role:*) names no row and is left alone. A subscription left
-- with no binding matches nothing, as before. Stored events are not touched.
-- The archive table and its reason are Go's, so either platform's archive
-- reads the same. Idempotent: once removed, nothing matches.
CREATE TABLE IF NOT EXISTS msg_subscription_event_types_archive (
    id INTEGER NOT NULL,
    subscription_id VARCHAR(17) NOT NULL,
    event_type_id VARCHAR(17),
    event_type_code VARCHAR(255) NOT NULL,
    spec_version VARCHAR(50),
    archived_reason VARCHAR(100) NOT NULL,
    archived_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_msg_sub_event_types_archive_subscription
    ON msg_subscription_event_types_archive (subscription_id);

-- One statement (data-modifying CTEs, one snapshot), as Go's.
WITH unemitted(code) AS (VALUES
        ('platform:admin:edm:created'),
        ('platform:admin:edm:deleted'),
        ('platform:admin:edm:updated'),
        ('platform:admin:idp:created'),
        ('platform:admin:idp:deleted'),
        ('platform:admin:idp:updated'),
        ('platform:iam:anchor-domain:created'),
        ('platform:iam:anchor-domain:deleted'),
        ('platform:iam:auth-config:created'),
        ('platform:iam:auth-config:deleted'),
        ('platform:iam:auth-config:updated'),
        ('platform:iam:role:created'),
        ('platform:iam:role:deleted'),
        ('platform:iam:role:updated'),
        ('platform:iam:roles:synced')),
     unemitted_types AS (
        SELECT et.id FROM msg_event_types et WHERE et.code IN (SELECT code FROM unemitted)),
     moved AS (
        DELETE FROM msg_subscription_event_types b
         WHERE b.event_type_code IN (SELECT code FROM unemitted)
            OR b.event_type_id IN (SELECT id FROM unemitted_types)
        RETURNING b.id, b.subscription_id, b.event_type_id, b.event_type_code, b.spec_version),
     archived AS (
        INSERT INTO msg_subscription_event_types_archive
            (id, subscription_id, event_type_id, event_type_code, spec_version, archived_reason)
        SELECT id, subscription_id, event_type_id, event_type_code, spec_version, '071_event_type_not_emitted'
          FROM moved
        RETURNING id),
     schemas AS (
        DELETE FROM msg_event_type_spec_versions
         WHERE event_type_id IN (SELECT id FROM unemitted_types)
        RETURNING id)
DELETE FROM msg_event_types WHERE id IN (SELECT id FROM unemitted_types);

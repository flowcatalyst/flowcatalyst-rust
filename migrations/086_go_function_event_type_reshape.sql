-- Rust 086 mirrors Go 074_function_event_type_reshape.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Data: renames the eight other function event-type rows an older Go seed wrote under platform:admin:function*, and splits the setting-event bindings. A no-op where those rows do not exist.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- The function events Go shares with Java and Rust now carry their
-- platform:function:* codes and data (owner ruling 2026-10-13: reshape Go's
-- events to the Java/Rust design where a mapping exists;
-- docs/platform-bug-fixes.md F11). 073 renamed function:deleted; this renames
-- the other eight a seed wrote under platform:admin:function*:
--
--   function:created           -> platform:function:function:created
--   function:updated           -> platform:function:function:updated
--   function-version:published -> platform:function:version:published
--   function-version:retired   -> platform:function:version:retired
--   function-alias:set         -> platform:function:alias:changed
--   function-alias:deleted     -> platform:function:alias:removed
--   function-domain:claimed    -> platform:function:domain:claimed
--   function-domain:released   -> platform:function:domain:released
--
-- Each is the same event under its new code with its data reshaped, so, as in
-- 073, the catalogue row keeps its id and status, the subscription bindings
-- naming it (by row or by code) follow it, and when the new code is already a
-- row (this build's seeder or a Java/Rust seeder ran first) the bindings move
-- to that row, a binding the move would duplicate is dropped, and the old row
-- is deleted with its schema versions. A renamed row's 1.0 schema is replaced
-- with the new data's schema while it is still exactly what a seed wrote (a
-- schema someone changed is theirs); a row that already had the new code keeps
-- its own.
--
-- The setting events split by kind: a SECRET setting now emits
-- platform:function:secret:set / :deleted, a CONFIG setting
-- platform:function:config:updated, and platform:admin:function-setting:set /
-- :deleted remain, for DB settings only. Their rows and bindings stay; so that
-- a subscriber keeps receiving every setting change it picked, each
-- subscription bound to function-setting:set also gets a binding to
-- secret:set and config:updated, and one bound to function-setting:deleted to
-- secret:deleted and config:updated, unless it has one already.
-- (config:updated covers both writes and removals: a subscriber to only
-- function-setting:set also receives CONFIG removals.)
--
-- Wildcard bindings (platform:admin:function:*) are patterns, not names of
-- these types, and are left alone. Stored events and dispatch jobs keep the
-- code they were written with. Idempotent. Plain statements with inline
-- VALUES, no temp table (sqlc reads migrations and would model one).
DELETE FROM msg_subscription_event_types b
 USING (VALUES ('platform:admin:function:created', 'platform:function:function:created'),
               ('platform:admin:function:updated', 'platform:function:function:updated'),
               ('platform:admin:function-version:published', 'platform:function:version:published'),
               ('platform:admin:function-version:retired', 'platform:function:version:retired'),
               ('platform:admin:function-alias:set', 'platform:function:alias:changed'),
               ('platform:admin:function-alias:deleted', 'platform:function:alias:removed'),
               ('platform:admin:function-domain:claimed', 'platform:function:domain:claimed'),
               ('platform:admin:function-domain:released', 'platform:function:domain:released')) AS r(old_code, new_code)
 WHERE (b.event_type_code = r.old_code
        OR b.event_type_id IN (SELECT o.id FROM msg_event_types o WHERE o.code = r.old_code))
   AND EXISTS (SELECT 1 FROM msg_subscription_event_types k
                WHERE k.subscription_id = b.subscription_id AND k.event_type_code = r.new_code);

UPDATE msg_subscription_event_types b
   SET event_type_code = r.new_code,
       event_type_id = COALESCE((SELECT n.id FROM msg_event_types n WHERE n.code = r.new_code), b.event_type_id)
  FROM (VALUES ('platform:admin:function:created', 'platform:function:function:created'),
               ('platform:admin:function:updated', 'platform:function:function:updated'),
               ('platform:admin:function-version:published', 'platform:function:version:published'),
               ('platform:admin:function-version:retired', 'platform:function:version:retired'),
               ('platform:admin:function-alias:set', 'platform:function:alias:changed'),
               ('platform:admin:function-alias:deleted', 'platform:function:alias:removed'),
               ('platform:admin:function-domain:claimed', 'platform:function:domain:claimed'),
               ('platform:admin:function-domain:released', 'platform:function:domain:released')) AS r(old_code, new_code)
 WHERE b.event_type_code = r.old_code
    OR b.event_type_id IN (SELECT o.id FROM msg_event_types o WHERE o.code = r.old_code);

DELETE FROM msg_event_type_spec_versions sv
 USING msg_event_types o,
       (VALUES ('platform:admin:function:created', 'platform:function:function:created'),
               ('platform:admin:function:updated', 'platform:function:function:updated'),
               ('platform:admin:function-version:published', 'platform:function:version:published'),
               ('platform:admin:function-version:retired', 'platform:function:version:retired'),
               ('platform:admin:function-alias:set', 'platform:function:alias:changed'),
               ('platform:admin:function-alias:deleted', 'platform:function:alias:removed'),
               ('platform:admin:function-domain:claimed', 'platform:function:domain:claimed'),
               ('platform:admin:function-domain:released', 'platform:function:domain:released')) AS r(old_code, new_code)
 WHERE sv.event_type_id = o.id
   AND o.code = r.old_code
   AND EXISTS (SELECT 1 FROM msg_event_types n WHERE n.code = r.new_code);

DELETE FROM msg_event_types o
 USING (VALUES ('platform:admin:function:created', 'platform:function:function:created'),
               ('platform:admin:function:updated', 'platform:function:function:updated'),
               ('platform:admin:function-version:published', 'platform:function:version:published'),
               ('platform:admin:function-version:retired', 'platform:function:version:retired'),
               ('platform:admin:function-alias:set', 'platform:function:alias:changed'),
               ('platform:admin:function-alias:deleted', 'platform:function:alias:removed'),
               ('platform:admin:function-domain:claimed', 'platform:function:domain:claimed'),
               ('platform:admin:function-domain:released', 'platform:function:domain:released')) AS r(old_code, new_code)
 WHERE o.code = r.old_code
   AND EXISTS (SELECT 1 FROM msg_event_types n WHERE n.code = r.new_code);

-- Renamed rows: the seeded 1.0 schema becomes the new data's. Runs before the
-- rename, on the old code, so a row that already had the new code is never
-- touched.
UPDATE msg_event_type_spec_versions sv
   SET schema_content = s.new_schema::jsonb, updated_at = NOW()
  FROM msg_event_types et,
       (VALUES
        ('platform:admin:function:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"functionId":{"type":"string"},"name":{"type":"string"}},"required":["functionId","address","name"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"applicationId":{"type":"string"},"clientId":{"type":["string","null"]},"functionId":{"type":"string"}},"required":["functionId","address","applicationId"],"type":"object"}'),
        ('platform:admin:function:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"functionId":{"type":"string"}},"required":["functionId","address"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"description":{"type":["string","null"]},"functionId":{"type":"string"},"status":{"type":"string"}},"required":["functionId","address","status"],"type":"object"}'),
        ('platform:admin:function-version:published',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"digest":{"type":"string"},"functionId":{"type":"string"},"number":{"minimum":0,"type":"integer"},"runtime":{"type":"string"}},"required":["functionId","address","number","digest","runtime"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"digest":{"type":"string"},"functionId":{"type":"string"},"pool":{"type":"string"},"signerIssuer":{"type":["string","null"]},"signerSubject":{"type":["string","null"]},"version":{"minimum":0,"type":"integer"},"versionId":{"type":"string"}},"required":["functionId","address","versionId","version","digest","pool"],"type":"object"}'),
        ('platform:admin:function-version:retired',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"functionId":{"type":"string"},"number":{"minimum":0,"type":"integer"}},"required":["functionId","address","number"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"functionId":{"type":"string"},"version":{"minimum":0,"type":"integer"},"versionId":{"type":"string"}},"required":["functionId","address","versionId","version"],"type":"object"}'),
        ('platform:admin:function-alias:set',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"alias":{"type":"string"},"functionId":{"type":"string"},"number":{"minimum":0,"type":"integer"}},"required":["functionId","address","alias","number"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"alias":{"type":"string"},"functionId":{"type":"string"},"previousVersionId":{"type":["string","null"]},"version":{"minimum":0,"type":"integer"},"versionId":{"type":"string"}},"required":["functionId","address","alias","versionId","version"],"type":"object"}'),
        ('platform:admin:function-alias:deleted',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"alias":{"type":"string"},"functionId":{"type":"string"}},"required":["functionId","address","alias"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"address":{"type":"string"},"alias":{"type":"string"},"functionId":{"type":"string"},"version":{"minimum":0,"type":"integer"},"versionId":{"type":"string"}},"required":["functionId","address","alias","versionId","version"],"type":"object"}'),
        ('platform:admin:function-domain:claimed',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":["string","null"]},"domainId":{"type":"string"},"zone":{"type":"string"}},"required":["domainId","zone"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"domainId":{"type":"string"},"hostname":{"type":"string"},"owner":{"type":"string"}},"required":["domainId","hostname","owner"],"type":"object"}'),
        ('platform:admin:function-domain:released',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"domainId":{"type":"string"},"zone":{"type":"string"}},"required":["domainId","zone"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"domainId":{"type":"string"},"hostname":{"type":"string"},"owner":{"type":"string"}},"required":["domainId","hostname","owner"],"type":"object"}')
       ) AS s(old_code, old_schema, new_schema)
 WHERE et.id = sv.event_type_id
   AND et.code = s.old_code
   AND sv.version = '1.0'
   AND sv.schema_content = s.old_schema::jsonb;

UPDATE msg_event_types et
   SET code = r.new_code, subdomain = r.subdomain, aggregate = r.aggregate, updated_at = NOW()
  FROM (VALUES
        ('platform:admin:function:created', 'platform:function:function:created', 'function', 'function'),
        ('platform:admin:function:updated', 'platform:function:function:updated', 'function', 'function'),
        ('platform:admin:function-version:published', 'platform:function:version:published', 'function', 'version'),
        ('platform:admin:function-version:retired', 'platform:function:version:retired', 'function', 'version'),
        ('platform:admin:function-alias:set', 'platform:function:alias:changed', 'function', 'alias'),
        ('platform:admin:function-alias:deleted', 'platform:function:alias:removed', 'function', 'alias'),
        ('platform:admin:function-domain:claimed', 'platform:function:domain:claimed', 'function', 'domain'),
        ('platform:admin:function-domain:released', 'platform:function:domain:released', 'function', 'domain'))
       AS r(old_code, new_code, subdomain, aggregate)
 WHERE et.code = r.old_code
   AND NOT EXISTS (SELECT 1 FROM msg_event_types n WHERE n.code = r.new_code);

INSERT INTO msg_subscription_event_types (subscription_id, event_type_id, event_type_code)
SELECT DISTINCT b.subscription_id,
       (SELECT n.id FROM msg_event_types n WHERE n.code = r.new_code),
       r.new_code
  FROM msg_subscription_event_types b,
       (VALUES ('platform:admin:function-setting:set', 'platform:function:secret:set'),
               ('platform:admin:function-setting:set', 'platform:function:config:updated'),
               ('platform:admin:function-setting:deleted', 'platform:function:secret:deleted'),
               ('platform:admin:function-setting:deleted', 'platform:function:config:updated'))
       AS r(old_code, new_code)
 WHERE (b.event_type_code = r.old_code
        OR b.event_type_id IN (SELECT o.id FROM msg_event_types o WHERE o.code = r.old_code))
   AND NOT EXISTS (SELECT 1 FROM msg_subscription_event_types k
                    WHERE k.subscription_id = b.subscription_id AND k.event_type_code = r.new_code);

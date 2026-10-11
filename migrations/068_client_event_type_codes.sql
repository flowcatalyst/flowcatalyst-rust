-- Go's migration 070 (flowcatalyst-go 25c3ed6), its Up section verbatim: the
-- runner here applies migrations forward only. It is idempotent, so a
-- database Go already migrated past 070 runs it as a no-op.
--
-- The seeded event-type catalogue named the client lifecycle events
-- platform:iam:client:*, while the client use cases emit (and always have
-- emitted) platform:admin:client:*. A subscription bound to a catalogued
-- client event never received one. The seed now catalogues the emitted codes
-- (and their data, below); this renames the rows an older seed wrote, so the
-- type keeps its id, status and schema versions.
--
-- Only the six seeded codes, and only where the emitted code is not already
-- an event type (then both rows are left as they are). Subscription bindings
-- to a renamed type follow it, so a subscription made to "Client Created"
-- from the catalogue now receives client-created events. Idempotent: once
-- renamed, no old row matches.
CREATE TEMP TABLE tmp_070_client_renames (old_code VARCHAR(255), new_code VARCHAR(255)) ON COMMIT DROP;
INSERT INTO tmp_070_client_renames VALUES
    ('platform:iam:client:created',    'platform:admin:client:created'),
    ('platform:iam:client:updated',    'platform:admin:client:updated'),
    ('platform:iam:client:activated',  'platform:admin:client:activated'),
    ('platform:iam:client:suspended',  'platform:admin:client:suspended'),
    ('platform:iam:client:deleted',    'platform:admin:client:deleted'),
    ('platform:iam:client:note-added', 'platform:admin:client:note-added');

UPDATE msg_subscription_event_types b
   SET event_type_code = r.new_code
  FROM tmp_070_client_renames r, msg_event_types et
 WHERE et.code = r.old_code
   AND (b.event_type_id = et.id OR (b.event_type_id IS NULL AND b.event_type_code = r.old_code))
   AND NOT EXISTS (SELECT 1 FROM msg_event_types n WHERE n.code = r.new_code);

UPDATE msg_event_types et
   SET code = r.new_code, subdomain = 'admin', updated_at = NOW()
  FROM tmp_070_client_renames r
 WHERE et.code = r.old_code
   AND NOT EXISTS (SELECT 1 FROM msg_event_types n WHERE n.code = r.new_code);

-- The seeded 1.0 schemas did not describe the emitted data (activated
-- required a previousStatus, deleted a name, note-added an author, none of
-- which is sent). Replace a renamed type's schema only while it is still
-- exactly what the seed wrote; a schema someone changed is theirs.
UPDATE msg_event_type_spec_versions sv
   SET schema_content = s.new_schema::jsonb, updated_at = NOW()
  FROM msg_event_types et,
       (VALUES
        ('platform:admin:client:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"description":{"type":["string","null"]},"identifier":{"type":"string"},"name":{"type":"string"}},"required":["clientId","name","identifier"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"identifier":{"type":"string"},"name":{"type":"string"}},"required":["clientId","name","identifier"],"type":"object"}'),
        ('platform:admin:client:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"description":{"type":["string","null"]},"name":{"type":["string","null"]}},"required":["clientId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"name":{"type":"string"}},"required":["clientId","name"],"type":"object"}'),
        ('platform:admin:client:activated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"previousStatus":{"type":"string"}},"required":["clientId","previousStatus"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"}},"required":["clientId"],"type":"object"}'),
        ('platform:admin:client:deleted',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"identifier":{"type":"string"},"name":{"type":"string"}},"required":["clientId","name","identifier"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"identifier":{"type":"string"}},"required":["clientId","identifier"],"type":"object"}'),
        ('platform:admin:client:note-added',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"author":{"type":"string"},"category":{"type":"string"},"clientId":{"type":"string"},"text":{"type":"string"}},"required":["clientId","category","text","author"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"category":{"type":"string"},"clientId":{"type":"string"},"text":{"type":"string"}},"required":["clientId","category","text"],"type":"object"}')
       ) AS s(code, old_schema, new_schema)
 WHERE et.id = sv.event_type_id
   AND et.code = s.code
   AND sv.schema_content = s.old_schema::jsonb;


-- Go's migration 072 (flowcatalyst-go 4c23adf), its Up section, extended to
-- every seeded form this platform's seed ever wrote: the runner here applies
-- migrations forward only. Idempotent, and a database Go already migrated
-- past 072 runs it as a no-op for Go's 28 codes (their corrected forms are
-- Go's, byte for byte as JSON).
--
-- Older seeded 1.0 schemas did not describe the data the use cases emit:
-- the dispatch-pool events send poolId, not dispatchPoolId; the event-type
-- schema events send specVersion, not version; the user, application,
-- service-account, connection and subscription events send fewer fields
-- than the schemas required; the first seed's subscription:synced lacked
-- clientId and its user:logged-in described another payload; an older Go
-- seed's client:applications-updated did not admit null for an empty list;
-- an older seed here let roles:synced syncedCodes and oauth-client
-- secret-rotated previousSecretExpiresAt be null (both are left out when
-- unset, never null; the timestamp is RFC 3339, as Go's schema says). The
-- seed now holds the corrected schemas, but the seeder never replaces an
-- attached 1.0 (or legacy v1), so this does it for existing databases: only
-- while the schema is still exactly what a seed wrote (each row below is one
-- seeded form, from this platform's seed history or Go's). A schema someone
-- changed is theirs and is left alone.
UPDATE msg_event_type_spec_versions sv
   SET schema_content = s.new_schema::jsonb, updated_at = NOW()
  FROM msg_event_types et,
       (VALUES
        ('platform:admin:connection:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":["string","null"]},"code":{"type":"string"},"connectionId":{"type":"string"},"endpoint":{"type":"string"},"name":{"type":"string"},"serviceAccountId":{"type":"string"}},"required":["connectionId","code","name","endpoint","serviceAccountId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"connectionId":{"type":"string"},"name":{"type":"string"}},"required":["connectionId","code","name"],"type":"object"}'),
        ('platform:admin:connection:deleted',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":["string","null"]},"code":{"type":"string"},"connectionId":{"type":"string"}},"required":["connectionId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"connectionId":{"type":"string"}},"required":["connectionId","code"],"type":"object"}'),
        ('platform:admin:connection:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"connectionId":{"type":"string"},"endpoint":{"type":["string","null"]},"name":{"type":["string","null"]},"status":{"type":["string","null"]}},"required":["connectionId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"connectionId":{"type":"string"},"name":{"type":"string"}},"required":["connectionId","name"],"type":"object"}'),
        ('platform:admin:dispatch-pool:archived',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"dispatchPoolId":{"type":"string"}},"required":["dispatchPoolId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"poolId":{"type":"string"}},"required":["poolId","code"],"type":"object"}'),
        ('platform:admin:dispatch-pool:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":["string","null"]},"code":{"type":"string"},"dispatchPoolId":{"type":"string"},"name":{"type":"string"}},"required":["dispatchPoolId","code","name"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"name":{"type":"string"},"poolId":{"type":"string"}},"required":["poolId","code","name"],"type":"object"}'),
        ('platform:admin:dispatch-pool:deleted',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"dispatchPoolId":{"type":"string"}},"required":["dispatchPoolId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"poolId":{"type":"string"}},"required":["poolId","code"],"type":"object"}'),
        ('platform:admin:dispatch-pool:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"concurrency":{"minimum":0,"type":["integer","null"]},"dispatchPoolId":{"type":"string"},"name":{"type":["string","null"]},"rateLimit":{"minimum":0,"type":["integer","null"]}},"required":["dispatchPoolId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"name":{"type":"string"},"poolId":{"type":"string"}},"required":["poolId","name"],"type":"object"}'),
        ('platform:admin:eventtype:schema-added',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"eventTypeId":{"type":"string"},"mimeType":{"type":"string"},"schemaType":{"type":"string"},"version":{"type":"string"}},"required":["eventTypeId","version","mimeType","schemaType"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"eventTypeId":{"type":"string"},"specVersion":{"type":"string"}},"required":["eventTypeId","specVersion"],"type":"object"}'),
        ('platform:admin:eventtype:schema-deprecated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"eventTypeId":{"type":"string"},"version":{"type":"string"}},"required":["eventTypeId","version"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"eventTypeId":{"type":"string"},"specVersion":{"type":"string"}},"required":["eventTypeId","specVersion"],"type":"object"}'),
        ('platform:admin:eventtype:schema-finalised',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"deprecatedVersion":{"type":["string","null"]},"eventTypeId":{"type":"string"},"version":{"type":"string"}},"required":["eventTypeId","version"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"deprecatedVersion":{"type":["string","null"]},"eventTypeId":{"type":"string"},"specVersion":{"type":"string"}},"required":["eventTypeId","specVersion"],"type":"object"}'),
        ('platform:admin:eventtype:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"description":{"type":["string","null"]},"eventTypeId":{"type":"string"},"name":{"type":["string","null"]}},"required":["eventTypeId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"description":{"type":["string","null"]},"eventTypeId":{"type":"string"},"name":{"type":"string"}},"required":["eventTypeId","name"],"type":"object"}'),
        ('platform:admin:oauth-client:secret-rotated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"oauthClientId":{"type":"string"},"previousSecretExpiresAt":{"type":["string","null"]}},"required":["oauthClientId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"oauthClientId":{"type":"string"},"previousSecretExpiresAt":{"format":"date-time","type":"string"}},"required":["oauthClientId"],"type":"object"}'),
        ('platform:admin:roles:synced',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationCode":{"type":["string","null"]},"created":{"minimum":0,"type":"integer"},"removed":{"minimum":0,"type":"integer"},"syncedCodes":{"items":{"type":"string"},"type":["array","null"]},"total":{"minimum":0,"type":"integer"},"updated":{"minimum":0,"type":"integer"}},"required":["created","updated","removed","total"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationCode":{"type":["string","null"]},"created":{"minimum":0,"type":"integer"},"removed":{"minimum":0,"type":"integer"},"syncedCodes":{"items":{"type":"string"},"type":"array"},"total":{"minimum":0,"type":"integer"},"updated":{"minimum":0,"type":"integer"}},"required":["created","updated","removed","total"],"type":"object"}'),
        ('platform:admin:subscription:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":["string","null"]},"code":{"type":"string"},"connectionId":{"type":"string"},"eventTypes":{"items":{"type":"string"},"type":"array"},"name":{"type":"string"},"subscriptionId":{"type":"string"}},"required":["subscriptionId","code","name","connectionId","eventTypes"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"name":{"type":"string"},"subscriptionId":{"type":"string"}},"required":["subscriptionId","code","name"],"type":"object"}'),
        ('platform:admin:subscription:paused',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"subscriptionId":{"type":"string"}},"required":["subscriptionId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"subscriptionId":{"type":"string"}},"required":["subscriptionId"],"type":"object"}'),
        ('platform:admin:subscription:resumed',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"subscriptionId":{"type":"string"}},"required":["subscriptionId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"subscriptionId":{"type":"string"}},"required":["subscriptionId"],"type":"object"}'),
        ('platform:admin:subscription:synced',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationCode":{"type":"string"},"created":{"minimum":0,"type":"integer"},"deleted":{"minimum":0,"type":"integer"},"syncedCodes":{"items":{"type":"string"},"type":"array"},"updated":{"minimum":0,"type":"integer"}},"required":["applicationCode","created","updated","deleted","syncedCodes"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationCode":{"type":"string"},"clientId":{"type":["string","null"]},"created":{"minimum":0,"type":"integer"},"deleted":{"minimum":0,"type":"integer"},"syncedCodes":{"items":{"type":"string"},"type":"array"},"updated":{"minimum":0,"type":"integer"}},"required":["applicationCode","created","updated","deleted","syncedCodes"],"type":"object"}'),
        ('platform:admin:subscription:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"eventTypesAdded":{"items":{"type":"string"},"type":"array"},"eventTypesRemoved":{"items":{"type":"string"},"type":"array"},"name":{"type":["string","null"]},"subscriptionId":{"type":"string"}},"required":["subscriptionId","eventTypesAdded","eventTypesRemoved"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"name":{"type":"string"},"subscriptionId":{"type":"string"}},"required":["subscriptionId","name"],"type":"object"}'),
        ('platform:iam:application:activated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"code":{"type":"string"}},"required":["applicationId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"}},"required":["applicationId"],"type":"object"}'),
        ('platform:iam:application:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"applicationType":{"type":"string"},"code":{"type":"string"},"name":{"type":"string"}},"required":["applicationId","code","name","applicationType"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"code":{"type":"string"},"name":{"type":"string"}},"required":["applicationId","code","name"],"type":"object"}'),
        ('platform:iam:application:deactivated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"code":{"type":"string"}},"required":["applicationId","code"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"}},"required":["applicationId"],"type":"object"}'),
        ('platform:iam:application:deleted',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"code":{"type":"string"},"name":{"type":"string"}},"required":["applicationId","code","name"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"code":{"type":"string"}},"required":["applicationId","code"],"type":"object"}'),
        ('platform:iam:application:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"description":{"type":["string","null"]},"name":{"type":["string","null"]}},"required":["applicationId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":"string"},"name":{"type":"string"}},"required":["applicationId","name"],"type":"object"}'),
        ('platform:iam:client:applications-updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"disabledRemoved":{"items":{"type":"string"},"type":"array"},"enabledAdded":{"items":{"type":"string"},"type":"array"},"enabledApplicationIds":{"items":{"type":"string"},"type":"array"}},"required":["clientId","enabledApplicationIds","enabledAdded","disabledRemoved"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":"string"},"disabledRemoved":{"items":{"type":"string"},"type":["array","null"]},"enabledAdded":{"items":{"type":"string"},"type":["array","null"]},"enabledApplicationIds":{"items":{"type":"string"},"type":["array","null"]}},"required":["clientId","enabledApplicationIds","enabledAdded","disabledRemoved"],"type":"object"}'),
        ('platform:iam:serviceaccount:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"applicationId":{"type":["string","null"]},"clientIds":{"items":{"type":"string"},"type":"array"},"code":{"type":"string"},"name":{"type":"string"},"serviceAccountId":{"type":"string"}},"required":["serviceAccountId","code","name","clientIds"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"code":{"type":"string"},"name":{"type":"string"},"serviceAccountId":{"type":"string"}},"required":["serviceAccountId","code","name"],"type":"object"}'),
        ('platform:iam:serviceaccount:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientIdsAdded":{"items":{"type":"string"},"type":"array"},"clientIdsRemoved":{"items":{"type":"string"},"type":"array"},"description":{"type":["string","null"]},"name":{"type":["string","null"]},"serviceAccountId":{"type":"string"}},"required":["serviceAccountId","clientIdsAdded","clientIdsRemoved"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"name":{"type":"string"},"serviceAccountId":{"type":"string"}},"required":["serviceAccountId","name"],"type":"object"}'),
        ('platform:iam:user:created',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":["string","null"]},"email":{"type":"string"},"emailDomain":{"type":"string"},"isAnchorUser":{"type":"boolean"},"name":{"type":"string"},"principalId":{"type":"string"},"scope":{"type":"string"}},"required":["principalId","email","emailDomain","name","scope","isAnchorUser"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"email":{"type":"string"},"principalId":{"type":"string"}},"required":["principalId","email"],"type":"object"}'),
        ('platform:iam:user:deactivated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"principalId":{"type":"string"},"reason":{"type":["string","null"]}},"required":["principalId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"principalId":{"type":"string"}},"required":["principalId"],"type":"object"}'),
        ('platform:iam:user:deleted',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"principalId":{"type":"string"}},"required":["principalId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"email":{"type":"string"},"principalId":{"type":"string"}},"required":["principalId","email"],"type":"object"}'),
        ('platform:iam:user:logged-in',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"clientId":{"type":["string","null"]},"email":{"type":"string"},"emailDomain":{"type":"string"},"identityProviderId":{"type":"string"},"loginMethod":{"type":"string"},"principalId":{"type":"string"},"scope":{"type":"string"}},"required":["principalId","email","emailDomain","scope","identityProviderId","loginMethod"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":true,"properties":{"email":{"type":"string"},"federatedClaims":{"oneOf":[{"properties":{"accessToken":{"type":"object"},"idToken":{"type":"object"}},"required":["accessToken","idToken"],"type":"object"},{"type":"null"}]},"flowcatalystClaims":{"properties":{"applications":{"items":{"type":"string"},"type":"array"},"clients":{"items":{"type":"string"},"type":"array"},"email":{"type":"string"},"roles":{"items":{"type":"string"},"type":"array"},"type":{"type":"string"}},"required":["email","type","roles","clients","applications"],"type":"object"},"identityProviderCode":{"type":["string","null"]},"loginMethod":{"enum":["INTERNAL","OIDC"],"type":"string"},"userId":{"type":"string"}},"required":["userId","email","loginMethod","flowcatalystClaims"],"type":"object"}'),
        ('platform:iam:user:password-reset-completed',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"email":{"type":"string"},"principalId":{"type":"string"}},"required":["principalId","email"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"principalId":{"type":"string"}},"required":["principalId"],"type":"object"}'),
        ('platform:iam:user:updated',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"email":{"type":["string","null"]},"name":{"type":["string","null"]},"principalId":{"type":"string"}},"required":["principalId"],"type":"object"}',
         '{"$schema":"http://json-schema.org/draft-07/schema#","additionalProperties":false,"properties":{"name":{"type":"string"},"principalId":{"type":"string"}},"required":["principalId","name"],"type":"object"}')
       ) AS s(code, old_schema, new_schema)
 WHERE et.id = sv.event_type_id
   AND et.code = s.code
   AND sv.version IN ('1.0', 'v1')
   AND sv.schema_content = s.old_schema::jsonb;

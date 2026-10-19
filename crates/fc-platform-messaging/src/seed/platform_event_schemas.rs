//! Platform Event Schemas
//!
//! JSON Schemas for all platform domain event payloads. These describe the
//! non-envelope fields of each event (i.e., everything except EventMetadata).
//!
//! Schema keys match the EVENT_TYPE constants on the domain event structs
//! exactly. No webhook delivery events are registered here — emitting
//! events about webhook delivery would create circular dispatch loops.
//!
//! Schemas follow JSON Schema draft-07.

use serde_json::{json, Value};
use std::collections::HashMap;

/// Returns a map of event type code → JSON Schema for the event payload.
pub fn schemas() -> HashMap<&'static str, Value> {
    let mut m = HashMap::new();

    // ─── platform:iam:user ─────────────────────────────────────────────
    m.insert(
        "platform:iam:user:created",
        obj(&[req_str("principalId"), req_str("email")]),
    );
    m.insert(
        "platform:iam:user:updated",
        obj(&[req_str("principalId"), req_str("name")]),
    );
    m.insert(
        "platform:iam:user:activated",
        obj(&[req_str("principalId")]),
    );
    m.insert(
        "platform:iam:user:deactivated",
        obj(&[req_str("principalId")]),
    );
    m.insert(
        "platform:iam:user:deleted",
        obj(&[req_str("principalId"), req_str("email")]),
    );
    m.insert(
        "platform:iam:user:roles-assigned",
        obj(&[
            req_str("principalId"),
            req_str_array("roles"),
            req_str_array("added"),
            req_str_array("removed"),
        ]),
    );
    m.insert(
        "platform:iam:user:application-access-assigned",
        obj(&[
            req_str("userId"),
            req_str_array("applicationIds"),
            req_str_array("added"),
            req_str_array("removed"),
        ]),
    );
    m.insert(
        "platform:iam:user:client-access-granted",
        obj(&[req_str("principalId"), req_str("clientId")]),
    );
    m.insert(
        "platform:iam:user:client-access-revoked",
        obj(&[req_str("principalId"), req_str("clientId")]),
    );
    m.insert(
        "platform:iam:user:logged-in",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "userId": {"type": "string"},
                "email": {"type": "string"},
                "loginMethod": {"type": "string", "enum": ["INTERNAL", "OIDC"]},
                "identityProviderCode": {"type": ["string", "null"]},
                "flowcatalystClaims": {
                    "type": "object",
                    "properties": {
                        "email": {"type": "string"},
                        "type": {"type": "string"},
                        "roles": {"type": "array", "items": {"type": "string"}},
                        "clients": {"type": "array", "items": {"type": "string"}},
                        "applications": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["email", "type", "roles", "clients", "applications"]
                },
                "federatedClaims": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "accessToken": {"type": "object"},
                                "idToken": {"type": "object"}
                            },
                            "required": ["accessToken", "idToken"]
                        },
                        {"type": "null"}
                    ]
                }
            },
            "required": ["userId", "email", "loginMethod", "flowcatalystClaims"],
            "additionalProperties": true
        }),
    );
    m.insert(
        "platform:iam:user:password-reset-requested",
        obj(&[req_str("principalId"), req_str("email")]),
    );
    m.insert(
        "platform:iam:user:password-reset-completed",
        obj(&[req_str("principalId")]),
    );

    // ─── platform:iam:principals (sync — plural aggregate) ─────────────
    m.insert(
        "platform:iam:principals:synced",
        obj(&[
            req_str("applicationCode"),
            req_u32("created"),
            req_u32("updated"),
            req_u32("deactivated"),
            req_str_array("syncedEmails"),
        ]),
    );

    // ─── platform:iam:serviceaccount (no hyphen) ───────────────────────
    m.insert(
        "platform:iam:serviceaccount:created",
        obj(&[
            req_str("serviceAccountId"),
            req_str("code"),
            req_str("name"),
        ]),
    );
    m.insert(
        "platform:iam:serviceaccount:updated",
        obj(&[req_str("serviceAccountId"), req_str("name")]),
    );
    m.insert(
        "platform:iam:serviceaccount:deleted",
        obj(&[req_str("serviceAccountId"), req_str("code")]),
    );
    m.insert(
        "platform:iam:serviceaccount:roles-assigned",
        obj(&[
            req_str("serviceAccountId"),
            req_str_array("rolesAdded"),
            req_str_array("rolesRemoved"),
        ]),
    );
    m.insert(
        "platform:iam:serviceaccount:token-regenerated",
        obj(&[req_str("serviceAccountId"), req_str("code")]),
    );
    m.insert(
        "platform:iam:serviceaccount:secret-regenerated",
        obj(&[req_str("serviceAccountId"), req_str("code")]),
    );

    // ─── platform:iam:application ──────────────────────────────────────
    m.insert(
        "platform:iam:application:created",
        obj(&[req_str("applicationId"), req_str("code"), req_str("name")]),
    );
    m.insert(
        "platform:iam:application:updated",
        obj(&[req_str("applicationId"), req_str("name")]),
    );
    m.insert(
        "platform:iam:application:activated",
        obj(&[req_str("applicationId")]),
    );
    m.insert(
        "platform:iam:application:deactivated",
        obj(&[req_str("applicationId")]),
    );
    m.insert(
        "platform:iam:application:deleted",
        obj(&[req_str("applicationId"), req_str("code")]),
    );
    m.insert(
        "platform:iam:application:service-account-provisioned",
        obj(&[
            req_str("applicationId"),
            req_str("applicationCode"),
            req_str("serviceAccountId"),
            req_str("serviceAccountCode"),
        ]),
    );
    m.insert(
        "platform:iam:application:enabled-for-client",
        obj(&[
            req_str("applicationId"),
            req_str("clientId"),
            req_str("configId"),
        ]),
    );
    m.insert(
        "platform:iam:application:disabled-for-client",
        obj(&[
            req_str("applicationId"),
            req_str("clientId"),
            req_str("configId"),
        ]),
    );

    // ─── platform:admin:cors ───────────────────────────────────────────
    m.insert(
        "platform:admin:cors:origin-added",
        obj(&[req_str("originId"), req_str("origin")]),
    );
    m.insert(
        "platform:admin:cors:origin-deleted",
        obj(&[req_str("originId"), req_str("origin")]),
    );

    // ─── platform:admin:eventtype (no hyphen) ───────────────────────────
    m.insert(
        "platform:admin:eventtype:created",
        obj(&[
            req_str("eventTypeId"),
            req_str("code"),
            req_str("name"),
            opt_str("description"),
            req_str("application"),
            req_str("subdomain"),
            req_str("aggregate"),
            req_str("eventName"),
            opt_str("clientId"),
        ]),
    );
    m.insert(
        "platform:admin:eventtype:updated",
        obj(&[
            req_str("eventTypeId"),
            req_str("name"),
            opt_str("description"),
        ]),
    );
    m.insert(
        "platform:admin:eventtype:archived",
        obj(&[req_str("eventTypeId"), req_str("code")]),
    );
    m.insert(
        "platform:admin:eventtype:deleted",
        obj(&[req_str("eventTypeId"), req_str("code")]),
    );
    m.insert(
        "platform:admin:eventtype:schema-added",
        obj(&[req_str("eventTypeId"), req_str("specVersion")]),
    );
    m.insert(
        "platform:admin:eventtype:schema-finalised",
        obj(&[
            req_str("eventTypeId"),
            req_str("specVersion"),
            opt_str("deprecatedVersion"),
        ]),
    );
    m.insert(
        "platform:admin:eventtype:schema-deprecated",
        obj(&[req_str("eventTypeId"), req_str("specVersion")]),
    );
    m.insert(
        "platform:admin:eventtypes:synced",
        obj(&[
            req_str("applicationCode"),
            req_u32("created"),
            req_u32("updated"),
            req_u32("deleted"),
            req_str_array("syncedCodes"),
        ]),
    );

    // ─── platform:admin:connection ──────────────────────────────────────
    m.insert(
        "platform:admin:connection:created",
        obj(&[req_str("connectionId"), req_str("code"), req_str("name")]),
    );
    m.insert(
        "platform:admin:connection:updated",
        obj(&[req_str("connectionId"), req_str("name")]),
    );
    m.insert(
        "platform:admin:connection:deleted",
        obj(&[req_str("connectionId"), req_str("code")]),
    );

    m.insert(
        "platform:admin:connection:synced",
        obj(&[
            req_str("applicationCode"),
            opt_str("clientId"),
            req_u32("created"),
            req_u32("updated"),
            req_u32("deleted"),
            req_str_array("syncedCodes"),
        ]),
    );

    // ─── platform:admin:dispatch-pool ───────────────────────────────────
    m.insert(
        "platform:admin:dispatch-pool:created",
        obj(&[req_str("poolId"), req_str("code"), req_str("name")]),
    );
    m.insert(
        "platform:admin:dispatch-pool:updated",
        obj(&[req_str("poolId"), req_str("name")]),
    );
    m.insert(
        "platform:admin:dispatch-pool:archived",
        obj(&[req_str("poolId"), req_str("code")]),
    );
    m.insert(
        "platform:admin:dispatch-pool:deleted",
        obj(&[req_str("poolId"), req_str("code")]),
    );
    m.insert(
        "platform:admin:dispatch-pools:synced",
        obj(&[
            req_str("applicationCode"),
            req_u32("created"),
            req_u32("updated"),
            req_u32("deleted"),
            req_str_array("syncedCodes"),
        ]),
    );

    // ─── platform:admin:subscription ────────────────────────────────────
    m.insert(
        "platform:admin:subscription:created",
        obj(&[req_str("subscriptionId"), req_str("code"), req_str("name")]),
    );
    m.insert(
        "platform:admin:subscription:updated",
        obj(&[req_str("subscriptionId"), req_str("name")]),
    );
    m.insert(
        "platform:admin:subscription:paused",
        obj(&[req_str("subscriptionId")]),
    );
    m.insert(
        "platform:admin:subscription:resumed",
        obj(&[req_str("subscriptionId")]),
    );
    m.insert(
        "platform:admin:subscription:deleted",
        obj(&[req_str("subscriptionId"), req_str("code")]),
    );
    m.insert(
        "platform:admin:subscription:synced",
        obj(&[
            req_str("applicationCode"),
            opt_str("clientId"),
            req_u32("created"),
            req_u32("updated"),
            req_u32("deleted"),
            req_str_array("syncedCodes"),
        ]),
    );

    emitted_beyond_go_catalogue(&mut m);

    m
}

/// Schemas for the event types the platform emits that Go's seeded
/// catalogue lacked. The entries above are Go's older catalogue (codes, names
/// and schemas, `internal/platform/seed/event_schemas.go`), pruned of the
/// codes nothing emits and with the schemas corrected to the emitted data
/// (Go's F9 follow-up, `45537af` and `4c23adf`; migrations 069 and 070 here).
/// These describe each emitted event's `data` exactly (Go's `ToDataJSON`),
/// so a subscription can reference every type the platform actually emits.
fn emitted_beyond_go_catalogue(m: &mut HashMap<&'static str, Value>) {
    let id_and = |id: &'static str, more: &[Prop]| {
        let mut props = vec![req_str(id)];
        props.extend_from_slice(more);
        obj(&props)
    };

    // Client (emitted as `platform:admin:client:*`).
    m.insert(
        "platform:admin:client:created",
        id_and("clientId", &[req_str("name"), req_str("identifier")]),
    );
    m.insert(
        "platform:admin:client:updated",
        id_and("clientId", &[req_str("name")]),
    );
    m.insert("platform:admin:client:activated", id_and("clientId", &[]));
    m.insert(
        "platform:admin:client:suspended",
        id_and("clientId", &[req_str("reason")]),
    );
    m.insert(
        "platform:admin:client:deactivated",
        id_and("clientId", &[opt_str("reason")]),
    );
    m.insert(
        "platform:admin:client:deleted",
        id_and("clientId", &[req_str("identifier")]),
    );
    m.insert(
        "platform:admin:client:note-added",
        id_and("clientId", &[req_str("category"), req_str("text")]),
    );
    m.insert(
        "platform:iam:client:applications-updated",
        id_and(
            "clientId",
            &[
                nullable_str_array("enabledApplicationIds"),
                nullable_str_array("enabledAdded"),
                nullable_str_array("disabledRemoved"),
            ],
        ),
    );
    m.insert(
        "platform:iam:application:client-config-updated",
        obj(&[
            req_str("applicationId"),
            req_str("clientId"),
            req_str("configId"),
            opt_bool("enabled"),
            opt_str("baseUrlOverride"),
            req_bool("configChanged"),
        ]),
    );

    // Service account deactivation.
    m.insert(
        "platform:iam:serviceaccount:deactivated",
        id_and("serviceAccountId", &[]),
    );

    // Role (Go emits `platform:admin:role:*`).
    for code in [
        "platform:admin:role:created",
        "platform:admin:role:updated",
        "platform:admin:role:deleted",
    ] {
        m.insert(code, id_and("roleId", &[req_str("name")]));
    }
    for code in [
        "platform:admin:role:permission-granted",
        "platform:admin:role:permission-revoked",
    ] {
        m.insert(
            code,
            id_and("roleId", &[req_str("roleName"), req_str("permission")]),
        );
    }
    for code in [
        "platform:admin:dispatch-pool:suspended",
        "platform:admin:dispatch-pool:activated",
    ] {
        m.insert(code, id_and("poolId", &[req_str("code")]));
    }
    m.insert(
        "platform:admin:roles:synced",
        obj(&[
            req_u32("created"),
            req_u32("updated"),
            req_u32("removed"),
            req_u32("total"),
            opt_str("applicationCode"),
            opt_str_array("syncedCodes"),
        ]),
    );

    // Anchor domain and auth config (Go emits them under platform:admin).
    for code in [
        "platform:admin:anchor-domain:created",
        "platform:admin:anchor-domain:updated",
        "platform:admin:anchor-domain:deleted",
    ] {
        m.insert(code, id_and("anchorDomainId", &[req_str("domain")]));
    }
    for code in [
        "platform:admin:auth-config:created",
        "platform:admin:auth-config:updated",
        "platform:admin:auth-config:deleted",
    ] {
        m.insert(code, id_and("authConfigId", &[req_str("emailDomain")]));
    }

    // IdP role mapping.
    m.insert(
        "platform:admin:idp-role-mapping:created",
        id_and(
            "mappingId",
            &[
                req_str("idpType"),
                req_str("idpRoleName"),
                req_str("platformRoleName"),
            ],
        ),
    );
    m.insert(
        "platform:admin:idp-role-mapping:deleted",
        id_and("mappingId", &[req_str("idpRoleName")]),
    );

    // OAuth client.
    m.insert(
        "platform:admin:oauth-client:created",
        id_and(
            "oauthClientId",
            &[req_str("clientId"), req_str("clientName")],
        ),
    );
    m.insert(
        "platform:admin:oauth-client:updated",
        id_and("oauthClientId", &[req_str("clientName")]),
    );
    m.insert(
        "platform:admin:oauth-client:deleted",
        id_and("oauthClientId", &[req_str("clientId")]),
    );
    for code in [
        "platform:admin:oauth-client:activated",
        "platform:admin:oauth-client:deactivated",
        "platform:admin:oauth-client:previous-secret-revoked",
    ] {
        m.insert(code, id_and("oauthClientId", &[]));
    }
    m.insert(
        "platform:admin:oauth-client:secret-rotated",
        id_and("oauthClientId", &[opt_date_time("previousSecretExpiresAt")]),
    );

    // Identity provider and email domain mapping (Go's emitted names).
    for code in [
        "platform:admin:identity-provider:created",
        "platform:admin:identity-provider:updated",
        "platform:admin:identity-provider:deleted",
    ] {
        m.insert(code, id_and("identityProviderId", &[req_str("code")]));
    }
    for code in [
        "platform:admin:email-domain-mapping:created",
        "platform:admin:email-domain-mapping:updated",
        "platform:admin:email-domain-mapping:deleted",
    ] {
        m.insert(code, id_and("mappingId", &[req_str("emailDomain")]));
    }

    // Platform config.
    m.insert(
        "platform:admin:platform-config:property-set",
        id_and(
            "configId",
            &[
                req_str("applicationCode"),
                req_str("section"),
                req_str("property"),
            ],
        ),
    );
    m.insert(
        "platform:admin:platform-config:access-granted",
        id_and(
            "accessId",
            &[
                req_str("applicationCode"),
                req_str("roleCode"),
                req_bool("canWrite"),
            ],
        ),
    );
    m.insert(
        "platform:admin:platform-config:access-revoked",
        id_and(
            "accessId",
            &[req_str("applicationCode"), req_str("roleCode")],
        ),
    );

    // Process.
    m.insert(
        "platform:admin:process:created",
        id_and("processId", &[req_str("code"), req_str("name")]),
    );
    m.insert(
        "platform:admin:process:updated",
        id_and("processId", &[req_str("name")]),
    );
    for code in [
        "platform:admin:process:archived",
        "platform:admin:process:deleted",
    ] {
        m.insert(code, id_and("processId", &[req_str("code")]));
    }
    m.insert(
        "platform:admin:processes:synced",
        obj(&[
            req_str("applicationCode"),
            req_u32("created"),
            req_u32("updated"),
            req_u32("deleted"),
            req_str_array("syncedCodes"),
        ]),
    );

    // Scheduled job.
    for code in [
        "platform:admin:scheduled-job:created",
        "platform:admin:scheduled-job:updated",
        "platform:admin:scheduled-job:paused",
        "platform:admin:scheduled-job:resumed",
        "platform:admin:scheduled-job:archived",
        "platform:admin:scheduled-job:deleted",
    ] {
        m.insert(code, id_and("scheduledJobId", &[req_str("code")]));
    }
    m.insert(
        "platform:admin:scheduled-job:fired-manually",
        id_and("scheduledJobId", &[req_str("code"), req_str("instanceId")]),
    );
    m.insert(
        "platform:admin:scheduledjobs:synced",
        obj(&[
            req_str("applicationCode"),
            nullable_str_array("created"),
            nullable_str_array("updated"),
            nullable_str_array("archived"),
        ]),
    );

    // Passkey.
    m.insert(
        "platform:admin:passkey:registered",
        id_and("credentialId", &[req_str("userId"), opt_str("name")]),
    );
    for code in [
        "platform:admin:passkey:authenticated",
        "platform:admin:passkey:revoked",
    ] {
        m.insert(code, id_and("credentialId", &[req_str("userId")]));
    }

    // Application OpenAPI spec sync.
    m.insert(
        "platform:developer:application-openapi:synced",
        obj(&[
            req_str("applicationId"),
            req_str("applicationCode"),
            req_str("specId"),
            req_str("version"),
            req_str("specHash"),
            opt_str("archivedPriorVersion"),
            req_bool("hasBreaking"),
            req_bool("unchanged"),
        ]),
    );

    // Go's F9 (`8ddaed4`), data as Go's `ToDataJSON` and Rust's events.
    for code in [
        "platform:iam:user:developer-credential-set",
        "platform:iam:user:developer-credential-revoked",
    ] {
        m.insert(code, id_and("userId", &[]));
    }
    m.insert(
        "platform:admin:email-domain-mapping:provider-changed",
        id_and(
            "mappingId",
            &[
                req_str("emailDomain"),
                req_str("fromIdentityProviderId"),
                req_str("toIdentityProviderId"),
            ],
        ),
    );
    for code in [
        "platform:messaging:dispatch-job:cancelled",
        "platform:messaging:dispatch-job:completed",
    ] {
        m.insert(code, id_and("dispatchJobId", &[]));
    }
    // Go `29e8cb11`: the payload edit records sizes and digests, never the payload.
    m.insert(
        "platform:messaging:dispatch-job:payload-edited",
        id_and(
            "dispatchJobId",
            &[
                req_str("status"),
                req_u32("previousSize"),
                req_str("previousSha256"),
                req_u32("size"),
                req_str("sha256"),
                req_str("contentType"),
                req_str("previousContentType"),
            ],
        ),
    );
    // Go `ad37de21`: the rollup also says whether the run redrove COMPLETED
    // jobs and how many the status guard skipped.
    m.insert(
        "platform:messaging:dispatch-jobs:resent",
        obj(&[
            req_str_array("ids"),
            req_bool("redrive"),
            req_u32("redrivenCount"),
            opt_str_array("redrivenIds"),
            req_u32("skippedInFlight"),
            req_u32("skippedCompleted"),
        ]),
    );
    for code in [
        "platform:portal:app:created",
        "platform:portal:app:updated",
        "platform:portal:app:deleted",
    ] {
        m.insert(
            code,
            id_and(
                "portalAppId",
                &[req_str("clientId"), req_str("code"), req_str("name")],
            ),
        );
    }
    m.insert(
        "platform:portal:identity:ensured",
        id_and(
            "identityId",
            &[
                req_str("clientId"),
                req_str("email"),
                req_bool("created"),
                req_str("source"),
                opt_str("portalAppId"),
                opt_str("portalAppCode"),
            ],
        ),
    );
    m.insert(
        "platform:portal:identity:status-set",
        id_and("identityId", &[req_str("clientId"), req_str("status")]),
    );
    m.insert(
        "platform:portal:identity:deleted",
        id_and("identityId", &[req_str("clientId"), req_str("email")]),
    );
    m.insert(
        "platform:portal:identity:app-granted",
        id_and(
            "identityId",
            &[
                req_str("clientId"),
                req_str("portalAppId"),
                req_str("portalAppCode"),
                req_str("source"),
            ],
        ),
    );
    m.insert(
        "platform:portal:identity:app-revoked",
        id_and(
            "identityId",
            &[
                req_str("clientId"),
                req_str("portalAppId"),
                req_str("portalAppCode"),
            ],
        ),
    );

    emitted_by_rust_only(m);
}

/// Schemas for the types Rust emits that Go does not (Go has no event for
/// the app-docs sync, the permission catalogue or the admin token mint; the
/// function registry and the audit redaction follow Java). Each describes
/// the emitting struct's `Serialize` output.
fn emitted_by_rust_only(m: &mut HashMap<&'static str, Value>) {
    let function_and = |more: &[Prop]| {
        let mut props = vec![req_str("functionId"), req_str("address")];
        props.extend_from_slice(more);
        obj(&props)
    };

    m.insert(
        "platform:admin:app-docs:synced",
        obj(&[
            req_str("applicationCode"),
            req_u32("created"),
            req_u32("updated"),
            req_u32("deleted"),
            req_str_array("syncedCodes"),
        ]),
    );
    m.insert(
        "platform:admin:audit-log:redacted",
        obj(&[req_u32("scanned"), req_u32("redacted")]),
    );
    m.insert(
        "platform:admin:permission:defined",
        obj(&[
            req_str("permissionId"),
            req_str("permission"),
            opt_str("description"),
        ]),
    );
    m.insert(
        "platform:admin:permission:deleted",
        obj(&[req_str("permissionId"), req_str("permission")]),
    );
    m.insert(
        "platform:iam:serviceaccount:token-minted",
        obj(&[req_str("serviceAccountId"), req_str("code")]),
    );

    // Function registry: a function's events carry its id and address.
    m.insert(
        "platform:function:function:created",
        function_and(&[
            req_str("applicationId"),
            opt_str("clientId"),
            req_str("runtime"),
        ]),
    );
    m.insert(
        "platform:function:function:updated",
        function_and(&[opt_str("description"), req_str("status")]),
    );
    m.insert("platform:function:function:deleted", function_and(&[]));
    m.insert(
        "platform:function:version:published",
        function_and(&[
            req_str("versionId"),
            req_u32("version"),
            req_str("digest"),
            req_str("pool"),
            opt_str("signerIssuer"),
            opt_str("signerSubject"),
        ]),
    );
    m.insert(
        "platform:function:version:ready",
        function_and(&[req_str("versionId"), req_u32("version"), req_str("hostId")]),
    );
    m.insert(
        "platform:function:version:retired",
        function_and(&[req_str("versionId"), req_u32("version")]),
    );
    m.insert(
        "platform:function:alias:changed",
        function_and(&[
            req_str("alias"),
            req_str("versionId"),
            req_u32("version"),
            opt_str("previousVersionId"),
        ]),
    );
    m.insert(
        "platform:function:alias:removed",
        function_and(&[req_str("alias"), req_str("versionId"), req_u32("version")]),
    );
    m.insert(
        "platform:function:config:updated",
        function_and(&[req_str_array("keys")]),
    );
    for code in [
        "platform:function:secret:set",
        "platform:function:secret:deleted",
    ] {
        m.insert(code, function_and(&[req_str("key")]));
    }
    // A policy has no function: its owner ("platform" or the client id).
    m.insert(
        "platform:function:policy:updated",
        obj(&[req_str("owner"), req_u32("signerCount")]),
    );
    for code in [
        "platform:function:domain:claimed",
        "platform:function:domain:released",
    ] {
        m.insert(
            code,
            obj(&[req_str("domainId"), req_str("hostname"), req_str("owner")]),
        );
    }
}

/// Look up the schema for a given event type code.
pub fn schema_for(code: &str) -> Option<Value> {
    schemas().remove(code)
}

// ── Schema builder helpers ─────────────────────────────────────────────────

type Prop = (&'static str, Value, bool);

fn obj(props: &[Prop]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for (name, schema, is_required) in props {
        properties.insert((*name).to_string(), schema.clone());
        if *is_required {
            required.push(json!(name));
        }
    }

    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn req_str(name: &'static str) -> Prop {
    (name, json!({"type": "string"}), true)
}

fn opt_str(name: &'static str) -> Prop {
    (name, json!({"type": ["string", "null"]}), false)
}

fn req_bool(name: &'static str) -> Prop {
    (name, json!({"type": "boolean"}), true)
}

fn req_u32(name: &'static str) -> Prop {
    (name, json!({"type": "integer", "minimum": 0}), true)
}

fn opt_bool(name: &'static str) -> Prop {
    (name, json!({"type": ["boolean", "null"]}), false)
}

/// A list the producer leaves out when empty, never `null` (Go's
/// `optStrArray`, `omitempty`; serde `skip_serializing_if = "Vec::is_empty"`).
fn opt_str_array(name: &'static str) -> Prop {
    (
        name,
        json!({"type": "array", "items": {"type": "string"}}),
        false,
    )
}

/// An RFC 3339 timestamp the producer leaves out when unset (Go's
/// `optDateTime`; serde `skip_serializing_if = "Option::is_none"`).
fn opt_date_time(name: &'static str) -> Prop {
    (
        name,
        json!({"type": "string", "format": "date-time"}),
        false,
    )
}

/// A list that is always present but `null` when empty (Go appends to a nil
/// slice).
fn nullable_str_array(name: &'static str) -> Prop {
    (
        name,
        json!({"type": ["array", "null"], "items": {"type": "string"}}),
        true,
    )
}

fn req_str_array(name: &'static str) -> Prop {
    (
        name,
        json!({"type": "array", "items": {"type": "string"}}),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::platform_event_types;
    use std::collections::HashSet;

    #[test]
    fn all_schemas_are_valid_json_schema() {
        let schemas = schemas();
        assert!(
            schemas.len() >= 144,
            "Expected 144+ schemas, got {}",
            schemas.len()
        );
        for (code, schema) in &schemas {
            assert_eq!(
                schema["type"], "object",
                "Schema for {} must be object type",
                code
            );
            assert!(
                schema["properties"].is_object(),
                "Schema for {} must have properties",
                code
            );
            assert!(
                schema["required"].is_array(),
                "Schema for {} must have required array",
                code
            );
        }
    }

    #[test]
    fn user_created_schema_has_expected_fields() {
        let schema = schema_for("platform:iam:user:created").unwrap();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("principalId"));
        assert!(props.contains_key("email"));
        // What `UserCreated` sends (Go F9 follow-up, `4c23adf`).
        assert_eq!(props.len(), 2);
    }

    #[test]
    fn no_webhook_delivery_events() {
        let schemas = schemas();
        for code in schemas.keys() {
            assert!(
                !code.contains("webhook"),
                "No webhook events allowed: {}",
                code
            );
            assert!(
                !code.contains("delivery"),
                "No delivery events allowed: {}",
                code
            );
        }
    }

    #[test]
    fn only_platform_subdomains() {
        let schemas = schemas();
        let subdomains = [
            "iam",
            "admin",
            "messaging",
            "developer",
            "portal",
            "function",
        ];
        for code in schemas.keys() {
            let parts: Vec<&str> = code.split(':').collect();
            assert!(
                parts.len() == 4 && parts[0] == "platform" && subdomains.contains(&parts[1]),
                "Event type '{code}' must be platform:{{{}}}:{{aggregate}}:{{event}}",
                subdomains.join("|"),
            );
        }
    }

    /// Every type a platform use case emits can be subscribed to.
    #[test]
    fn every_emitted_go_event_type_is_catalogued() {
        let schemas = schemas();
        for code in [
            "platform:admin:client:created",
            "platform:admin:role:created",
            "platform:admin:roles:synced",
            "platform:admin:anchor-domain:updated",
            "platform:admin:identity-provider:created",
            "platform:admin:email-domain-mapping:created",
            "platform:admin:oauth-client:secret-rotated",
            "platform:admin:platform-config:property-set",
            "platform:admin:scheduled-job:fired-manually",
            "platform:admin:passkey:authenticated",
            "platform:admin:connection:synced",
            "platform:iam:serviceaccount:deactivated",
            "platform:iam:client:applications-updated",
        ] {
            assert!(schemas.contains_key(code), "{code} is not catalogued");
        }
    }

    #[test]
    fn definitions_and_schemas_are_aligned() {
        let schemas = schemas();
        let defs = platform_event_types::definitions();
        for def in &defs {
            assert!(
                schemas.contains_key(def.code.as_str()),
                "Definition '{}' has no matching schema",
                def.code,
            );
        }
        // Every schema key should be in definitions
        let def_codes: HashSet<&str> = defs.iter().map(|d| d.code.as_str()).collect();
        for code in schemas.keys() {
            assert!(
                def_codes.contains(code),
                "Schema '{}' has no matching definition",
                code,
            );
        }
    }
}

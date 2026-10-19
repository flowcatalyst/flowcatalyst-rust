//! Platform Event Type Definitions
//!
//! Canonical list of platform domain events: Go's seeded catalogue
//! (`internal/platform/seed/event_types.go`), then every type the platform
//! emits that Go's older catalogue lacked; exactly the emitted types (the
//! `it` test `emitted_catalogue_test` fails on drift either way). Used by the startup seeder
//! (`shared::database::seed_platform_event_types`, as Go seeds on every
//! start) and the BFF sync-platform endpoint.

use super::platform_event_schemas;
use crate::event_type::operations::SyncEventTypeInput;
use serde_json::Value;
use std::collections::HashMap;

/// Returns all platform domain event type definitions with JSON schemas.
///
/// Subdomains:
///
///   platform:iam:*       — Identity & access management (users, service
///                          accounts, clients, roles, applications, anchor
///                          domains, auth configs)
///   platform:admin:*     — Platform administration (CORS, identity providers,
///                          email domain mappings, event types, connections,
///                          dispatch pools, subscriptions, processes, ...)
///   platform:messaging:* — Dispatch-job actions
///   platform:developer:* — The application OpenAPI sync
///   platform:portal:*    — Portal apps and identities
///   platform:function:*  — The function registry (Rust and Java only)
///
/// Every type a platform use case emits is here (the `it` test
/// `emitted_catalogue_test` fails on one that is not).
pub fn definitions() -> Vec<SyncEventTypeInput> {
    let mut defs = Vec::new();
    let schemas = platform_event_schemas::schemas();

    // ─── platform:iam ─────────────────────────────────────────────────

    // User
    group(
        &mut defs,
        &schemas,
        "platform:iam:user",
        &[
            "created",
            "updated",
            "activated",
            "deactivated",
            "deleted",
            "roles-assigned",
            "application-access-assigned",
            "client-access-granted",
            "client-access-revoked",
            "logged-in",
            "password-reset-requested",
            "password-reset-completed",
            "developer-credential-set",
            "developer-credential-revoked",
        ],
    );
    // Principals (sync — aggregate is plural)
    push(
        &mut defs,
        &schemas,
        "platform:iam:principals:synced",
        "Principals Synced",
    );

    // Service Account (no hyphen in aggregate)
    group(
        &mut defs,
        &schemas,
        "platform:iam:serviceaccount",
        &[
            "created",
            "updated",
            "deleted",
            "roles-assigned",
            "token-regenerated",
            "secret-regenerated",
        ],
    );

    // Client: the lifecycle events are emitted under `platform:admin:client`
    // (catalogued below with the other emitted types); Go's catalogue listed
    // them as `platform:iam:client:*`, which nothing emits, until its
    // `25c3ed6`. Migration 068 renames the rows an older seed wrote.

    // Role, anchor domain, auth config, identity provider and email domain
    // mapping events are emitted under `platform:admin:*` (below). Older
    // seeds also catalogued `platform:iam:role:*`, `platform:iam:roles:synced`,
    // `platform:iam:anchor-domain:*`, `platform:iam:auth-config:*`,
    // `platform:admin:idp:*` and `platform:admin:edm:*`, which nothing has
    // emitted since `cb1b3e4f`; migration 069 removes them (Go's F9
    // follow-up, `45537af`).

    // Application
    group(
        &mut defs,
        &schemas,
        "platform:iam:application",
        &[
            "created",
            "updated",
            "activated",
            "deactivated",
            "deleted",
            "service-account-provisioned",
            "enabled-for-client",
            "disabled-for-client",
        ],
    );

    // ─── platform:admin ───────────────────────────────────────────────

    // CORS
    group(
        &mut defs,
        &schemas,
        "platform:admin:cors",
        &["origin-added", "origin-deleted"],
    );

    // Event Type (no hyphen in aggregate)
    group(
        &mut defs,
        &schemas,
        "platform:admin:eventtype",
        &[
            "created",
            "updated",
            "archived",
            "deleted",
            "schema-added",
            "schema-finalised",
            "schema-deprecated",
        ],
    );
    push(
        &mut defs,
        &schemas,
        "platform:admin:eventtypes:synced",
        "Event Types Synced",
    );

    // Connection
    group(
        &mut defs,
        &schemas,
        "platform:admin:connection",
        &["created", "updated", "deleted", "synced"],
    );

    // Dispatch Pool
    group(
        &mut defs,
        &schemas,
        "platform:admin:dispatch-pool",
        &["created", "updated", "archived", "deleted"],
    );
    push(
        &mut defs,
        &schemas,
        "platform:admin:dispatch-pools:synced",
        "Dispatch Pools Synced",
    );

    // Subscription
    group(
        &mut defs,
        &schemas,
        "platform:admin:subscription",
        &[
            "created", "updated", "paused", "resumed", "deleted", "synced",
        ],
    );

    // ─── Emitted, but absent from Go's seeded catalogue ───────────────
    //
    // Everything above is Go's older catalogue (`seed/event_types.go`) code
    // for code, less the codes nothing emits (Go's `45537af`; Go also drops
    // `platform:iam:user:password-reset-requested`, which is emitted here),
    // so production rows stay as Go seeded them. Go emits several
    // types under other names than it seeds (the client events were one,
    // until Go catalogued them in `25c3ed6`); these make every emitted type
    // subscribable.
    group(
        &mut defs,
        &schemas,
        "platform:admin:client",
        &[
            "created",
            "updated",
            "activated",
            "suspended",
            "deactivated",
            "deleted",
            "note-added",
        ],
    );
    push(
        &mut defs,
        &schemas,
        "platform:iam:client:applications-updated",
        "Client Applications Updated",
    );
    push(
        &mut defs,
        &schemas,
        "platform:iam:application:client-config-updated",
        "Application Client Config Updated",
    );
    push(
        &mut defs,
        &schemas,
        "platform:iam:serviceaccount:deactivated",
        "Serviceaccount Deactivated",
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:role",
        &[
            "created",
            "updated",
            "deleted",
            "permission-granted",
            "permission-revoked",
        ],
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:dispatch-pool",
        &["suspended", "activated"],
    );
    push(
        &mut defs,
        &schemas,
        "platform:admin:roles:synced",
        "Roles Synced",
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:anchor-domain",
        &["created", "updated", "deleted"],
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:auth-config",
        &["created", "updated", "deleted"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:admin:idp-role-mapping",
        "IdP Role Mapping",
        &["created", "deleted"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:admin:oauth-client",
        "OAuth Client",
        &[
            "created",
            "updated",
            "activated",
            "deactivated",
            "deleted",
            "secret-rotated",
            "previous-secret-revoked",
        ],
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:identity-provider",
        &["created", "updated", "deleted"],
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:email-domain-mapping",
        &["created", "updated", "provider-changed", "deleted"],
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:platform-config",
        &["property-set", "access-granted", "access-revoked"],
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:process",
        &["created", "updated", "archived", "deleted"],
    );
    push(
        &mut defs,
        &schemas,
        "platform:admin:processes:synced",
        "Processes Synced",
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:scheduled-job",
        &[
            "created",
            "updated",
            "paused",
            "resumed",
            "archived",
            "deleted",
            "fired-manually",
        ],
    );
    push(
        &mut defs,
        &schemas,
        "platform:admin:scheduledjobs:synced",
        "Scheduled Jobs Synced",
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:passkey",
        &["registered", "authenticated", "revoked"],
    );
    push(
        &mut defs,
        &schemas,
        "platform:developer:application-openapi:synced",
        "Application OpenAPI Synced",
    );

    // Go's F9 (`8ddaed4`): the dispatch-job actions and the portal, with
    // Go's codes, names and data.
    group(
        &mut defs,
        &schemas,
        "platform:messaging:dispatch-job",
        &["cancelled", "completed", "payload-edited"],
    );
    push(
        &mut defs,
        &schemas,
        "platform:messaging:dispatch-jobs:resent",
        "Dispatch Jobs Resent",
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:portal:app",
        "Portal App",
        &["created", "updated", "deleted"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:portal:identity",
        "Portal Identity",
        &[
            "ensured",
            "status-set",
            "deleted",
            "app-granted",
            "app-revoked",
        ],
    );

    // ─── Emitted by Rust only (no Go counterpart) ─────────────────────
    //
    // Writes Go makes without an event (the app-docs sync, the permission
    // catalogue, the admin token mint) and the Java-shaped function
    // registry and audit redaction (docs/go-mirror/2026-10-11-event-catalogue.md).
    push(
        &mut defs,
        &schemas,
        "platform:admin:app-docs:synced",
        "App Docs Synced",
    );
    push(
        &mut defs,
        &schemas,
        "platform:admin:audit-log:redacted",
        "Audit Log Redacted",
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:permission",
        &["defined", "deleted"],
    );
    push(
        &mut defs,
        &schemas,
        "platform:iam:serviceaccount:token-minted",
        "Serviceaccount Token Minted",
    );
    group(
        &mut defs,
        &schemas,
        "platform:function:function",
        &["created", "updated", "deleted"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:function:version",
        "Function Version",
        &["published", "ready", "retired"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:function:alias",
        "Function Alias",
        &["changed", "removed"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:function:policy",
        "Function Policy",
        &["updated"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:function:config",
        "Function Config",
        &["updated"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:function:secret",
        "Function Secret",
        &["set", "deleted"],
    );
    group_as(
        &mut defs,
        &schemas,
        "platform:function:domain",
        "Function Domain",
        &["claimed", "released"],
    );

    defs
}

/// Add a group of events under the same prefix, each named after the
/// aggregate segment ("User Created").
fn group(
    defs: &mut Vec<SyncEventTypeInput>,
    schemas: &HashMap<&str, Value>,
    prefix: &str,
    events: &[&str],
) {
    let aggregate = prefix.rsplit(':').next().unwrap_or(prefix);
    group_as(defs, schemas, prefix, &title_case(aggregate), events);
}

/// Add a group of events under the same prefix, each named "<label>
/// <Event>" (Go's `groupAs`: "Portal Identity App Granted").
fn group_as(
    defs: &mut Vec<SyncEventTypeInput>,
    schemas: &HashMap<&str, Value>,
    prefix: &str,
    label: &str,
    events: &[&str],
) {
    for event in events {
        let code = format!("{}:{}", prefix, event);
        let name = format!("{} {}", label, title_case(event));
        let schema = schemas.get(code.as_str()).cloned();
        defs.push(SyncEventTypeInput {
            code,
            name,
            description: None,
            schema,
        });
    }
}

/// Add a single event type.
fn push(
    defs: &mut Vec<SyncEventTypeInput>,
    schemas: &HashMap<&str, Value>,
    code: &str,
    name: &str,
) {
    defs.push(SyncEventTypeInput {
        code: code.to_string(),
        name: name.to_string(),
        description: None,
        schema: schemas.get(code).cloned(),
    });
}

fn title_case(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn codes_are_unique_and_named() {
        let defs = definitions();
        let mut seen = HashSet::new();
        for d in &defs {
            assert!(seen.insert(d.code.clone()), "duplicate code {}", d.code);
            assert!(!d.name.is_empty(), "{} has no name", d.code);
        }
    }

    /// Go's catalogue is seeded as Go seeds it, name for name.
    #[test]
    fn go_catalogue_names_are_kept() {
        let defs = definitions();
        let name = |code: &str| {
            defs.iter()
                .find(|d| d.code == code)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            name("platform:iam:user:roles-assigned"),
            "User Roles Assigned"
        );
        assert_eq!(name("platform:iam:principals:synced"), "Principals Synced");
        assert_eq!(
            name("platform:admin:connection:synced"),
            "Connection Synced"
        );
        assert_eq!(
            name("platform:admin:eventtype:schema-added"),
            "Eventtype Schema Added"
        );
        // Go's `groupAs` labels (Go F9).
        assert_eq!(
            name("platform:admin:oauth-client:secret-rotated"),
            "OAuth Client Secret Rotated"
        );
        assert_eq!(
            name("platform:admin:idp-role-mapping:created"),
            "IdP Role Mapping Created"
        );
    }
}

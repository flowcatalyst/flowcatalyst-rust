//! Platform Event Type Definitions
//!
//! Canonical list of platform domain events: Go's seeded catalogue
//! (`internal/platform/seed/event_types.go`) code for code, then every type
//! the platform emits that Go's catalogue lacks. Used by the startup seeder
//! (`shared::database::seed_platform_event_types`, as Go seeds on every
//! start) and the BFF sync-platform endpoint.

use super::platform_event_schemas;
use crate::event_type::operations::SyncEventTypeInput;
use serde_json::Value;
use std::collections::HashMap;

/// Returns all platform domain event type definitions with JSON schemas.
///
/// Two subdomains:
///
///   platform:iam:*   — Identity & access management (users, service accounts,
///                      clients, roles, applications, anchor domains, auth configs)
///   platform:admin:* — Platform administration (CORS, identity providers,
///                      email domain mappings, event types, connections,
///                      dispatch pools, subscriptions)
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

    // Client
    group(
        &mut defs,
        &schemas,
        "platform:iam:client",
        &[
            "created",
            "updated",
            "activated",
            "suspended",
            "deleted",
            "note-added",
        ],
    );

    // Role
    group(
        &mut defs,
        &schemas,
        "platform:iam:role",
        &["created", "updated", "deleted"],
    );
    push(
        &mut defs,
        &schemas,
        "platform:iam:roles:synced",
        "Roles Synced",
    );

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

    // Anchor Domain
    group(
        &mut defs,
        &schemas,
        "platform:iam:anchor-domain",
        &["created", "deleted"],
    );

    // Auth Config
    group(
        &mut defs,
        &schemas,
        "platform:iam:auth-config",
        &["created", "updated", "deleted"],
    );

    // ─── platform:admin ───────────────────────────────────────────────

    // CORS
    group(
        &mut defs,
        &schemas,
        "platform:admin:cors",
        &["origin-added", "origin-deleted"],
    );

    // Identity Provider
    group(
        &mut defs,
        &schemas,
        "platform:admin:idp",
        &["created", "updated", "deleted"],
    );

    // Email Domain Mapping
    group(
        &mut defs,
        &schemas,
        "platform:admin:edm",
        &["created", "updated", "deleted"],
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
    // Everything above is Go's catalogue (`seed/event_types.go`) code for
    // code, so production rows stay as Go seeded them. Go emits several
    // types under other names than it seeds (e.g. `platform:admin:client:*`
    // against the seeded `platform:iam:client:*`); these make every emitted
    // type subscribable.
    group(
        &mut defs,
        &schemas,
        "platform:admin:client",
        &[
            "created",
            "updated",
            "activated",
            "suspended",
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
        &["created", "updated", "deleted"],
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
    group(
        &mut defs,
        &schemas,
        "platform:admin:idp-role-mapping",
        &["created", "deleted"],
    );
    group(
        &mut defs,
        &schemas,
        "platform:admin:oauth-client",
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
        &["created", "updated", "deleted"],
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

    defs
}

/// Add a group of events under the same prefix.
fn group(
    defs: &mut Vec<SyncEventTypeInput>,
    schemas: &HashMap<&str, Value>,
    prefix: &str,
    events: &[&str],
) {
    for event in events {
        let code = format!("{}:{}", prefix, event);
        let aggregate = prefix.rsplit(':').next().unwrap_or(prefix);
        let name = format!("{} {}", title_case(aggregate), title_case(event));
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

    #[test]
    fn codes_are_unique_and_named() {
        let defs = definitions();
        let mut seen = std::collections::HashSet::new();
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
    }
}

//! The SDK classification of the platform API: the `x-sdk` vendor extension
//! (Go `tools/dump-spec/sdkclass.go`, commit `21f9734d`;
//! `docs/sdk-audit-and-python-sdk.md` A.2, A.8 in Go's repo).
//!
//! Every operation of the published document has exactly one class, carried as
//! `x-sdk`:
//!
//! - `app`: what an application SDK calls (sync, ingest, token/OIDC, the reads
//!   and app-delegated writes the SDKs and apps use);
//! - `admin`: the admin-console / operator surface, which belongs to an
//!   optional admin client, not an application SDK;
//! - absent: internal. Legacy aliases and operations that are neither (browser
//!   ceremonies, the function runner's control plane, router-to-platform
//!   config). Listed in [`INTERNAL_OPS`] / [`INTERNAL_TAGS`], so it is a
//!   decision, not a default.
//!
//! `app` is an explicit allow-list by operationId ([`APP_OPS`]). `admin` is by
//! tag ([`ADMIN_TAGS`]; a whole console area). `internal` is an explicit list by
//! operationId and wins over the admin tag. An operation none of the lists
//! decides is reported by [`classify`], and `tests/it/sdk_class_test.rs` fails on
//! it: classification is always a decision.
//!
//! The lists are Go's, verbatim, plus the few members of the Rust platform Go
//! lacks (marked below). They name operations by the operationId Go gives them;
//! the Rust document spells a few differently (the `operation-id` gaps of
//! `tests/data/openapi_go_gaps.baseline.txt`), and those are listed under both
//! spellings until the gap closes.

use serde_json::{json, Value};

/// What an application SDK calls, by operationId.
pub const APP_OPS: &[&str] = &[
    // Declarative definition sync (the application path for definitions).
    "syncRoles",
    "syncEventTypes",
    "syncSubscriptions",
    "syncConnections",
    "syncDispatchPools",
    "syncProcesses",
    "syncProcessesByBody",
    "syncPrincipals",
    "syncScheduledJobs",
    "syncAppDocs",
    "syncOpenapi",
    "syncUsers", // POST /api/principals/sync: user sync
    // Ingest (outbox processors and direct publishers).
    "createEvent",
    "batchIngestEvents",
    "createDispatchJob",
    "batchCreateDispatchJobs",
    "batchIngestAuditLogs",
    // Scheduled-job runner: read, completion, log.
    "listScheduledJobs",
    "getScheduledJob",
    "getScheduledJobByCode",
    "getScheduledJobInstance",
    "listScheduledJobInstanceLogs",
    "completeScheduledJobInstance",
    "writeScheduledJobInstanceLog",
    // Reads of definitions (A.8 "Reads").
    "listRoles",
    "getRole",
    "getRoleByCode",
    "getRolesByApplication",
    "listPermissions",
    "getPermission",
    "listEventTypes",
    "getEventType",
    "getEventTypeByCode",
    "listSubscriptions",
    "getSubscription",
    "listConnections",
    "getConnection",
    "listProcesses",
    "getProcess",
    "getProcessByCode",
    "listDispatchPools",
    "getDispatchPool",
    "getApplication",
    "getApplicationByCode",
    "listApplicationRoles",
    "listClients",
    "getClient",
    "getClientByIdentifier",
    // Operations the Inhance apps use through the Laravel request() escape hatch.
    "addEventTypeVersion",
    "getPlatformConfigProperty",
    "setPlatformConfigProperty",
    "deletePlatformConfigProperty",
    "listPlatformConfigProperties",
    "getEmailDomainMappingByDomain",
    "setPrincipalClientAssociation",
    // App-delegated user management (A.3: used by all three Inhance apps).
    "getPrincipal",
    "listPrincipals",
    "createUser",
    "updatePrincipal",
    "resetPrincipalPassword",
    // Scoped reads of an application's own events and dispatch jobs, and the
    // administration of its own users and invitations: what the application's
    // admin pages call as its service account (F20). The reads share the
    // console's operations; an application service account holding only the
    // application-service view is confined to its own applications by the
    // handler.
    "listEvents",
    "getEvent",
    "listDispatchJobs",
    "getDispatchJob",
    "listDispatchJobAttempts",
    "dispatchJobsByEvent",
    "listApplicationUsers",
    "getApplicationUser",
    "inviteApplicationUser",
    "listApplicationInvitations",
    "resendApplicationInvitation",
    "revokeApplicationInvitation",
    // Who am I (served by plain chi handlers; described by me.RegisterSpec).
    "getMe",
    "listMyApplications",
    "listMyClients",
    "getMyClient",
    "listMyClientApplications",
    // Token, JWKS and OIDC (plain chi handlers; described by oauthapi.RegisterSpec).
    "oauthToken",
    "oauthIntrospect",
    "oauthRevoke",
    "oauthUserinfo",
    "openidConfiguration",
    "jwks",
    // Rust spelling of `createDispatchJob` until the operation-id gap closes.
    "postApiDispatchJobs",
];

/// Deliberately neither app nor admin, by operationId.
pub const INTERNAL_OPS: &[&str] = &[
    // Legacy aliases and duplicates of a canonical operation (A.2 class 3).
    "listDispatchJobsRaw",
    "listDispatchJobsRawAlias",
    "getDispatchJobRaw",        // /raw, /list-raw
    "dispatchJobsByEventAlias", // by-event vs event
    "listEventsRaw",
    "listEventsRawAlias",                                      // /raw, /list-raw
    "regenerateOAuthClientSecret",                             // alias of rotate-secret
    "regenerateServiceAccountSigningSecret_regenerate-secret", // alias of regenerate-signing-secret
    "regenerateServiceAccountAuthToken_regenerate-token",      // alias of regenerate-auth-token
    "searchClientsByQuery",                                    // GET twin of POST /clients/search
    "listAuditLogsRecent",                                     // alias for list
    "addEventTypeSchema",                                      // alias of addEventTypeVersion
    // Function runner control plane: machine-to-machine from the function
    // runner to the platform, not console and not application surface.
    "getFunctionRunnerArtifact",
    "getFunctionRunnerDesired",
    "postFunctionRunnerDispatchJob",
    "postFunctionRunnerEvent",
    "postFunctionRunnerHeartbeat",
    // Passkey browser ceremonies (session-bound; the credential list/delete
    // operations stay admin).
    "webauthnAuthenticateBegin",
    "webauthnAuthenticateComplete",
    "webauthnRegisterBegin",
    "webauthnRegisterComplete",
    // Served to the router host / browsers, not to a console or an application.
    "getRouterConfig",
    "publicAllowedOrigins",
];

/// The console areas whose operations are admin unless the operationId is
/// listed in [`INTERNAL_OPS`] or [`APP_OPS`].
pub const ADMIN_TAGS: &[&str] = &[
    "anchor-domains",
    "applications",
    "audit-logs",
    "auth-configs",
    "clients",
    "connections",
    "cors-origins",
    "dispatch-jobs",
    "dispatch-pools",
    "docs",
    "email-domain-mappings",
    "event-types",
    "events",
    "function-domains",
    "functions",
    "identity-providers",
    "idp-role-mappings",
    "login-attempts",
    "oauth-clients",
    "platform-config",
    "portal-apps",
    "portal-users",
    "principals",
    "processes",
    "reset-approvals",
    "roles",
    "scheduled-jobs",
    "service-accounts",
    "subscriptions",
    "webauthn",
    // Rust-only function-model tag (Go has no function policies); an area of
    // the console like `functions`. To be revisited with the function model.
    "function-policies",
];

/// Whole areas that are internal: the session ceremonies of the Rust platform's
/// own `/auth/*` endpoints and the router-dashboard `/api/monitoring/*`
/// reads, neither of which Go documents. Neither is an SDK surface.
pub const INTERNAL_TAGS: &[&str] = &["auth", "monitoring"];

/// The vendor extension's name.
pub const EXTENSION: &str = "x-sdk";

/// One operation's class: `Some("app")`, `Some("admin")`, `None` for internal,
/// or an `Err` naming why the lists do not decide it.
fn class_of(operation_id: &str, first_tag: Option<&str>) -> Result<Option<&'static str>, String> {
    let in_app = APP_OPS.contains(&operation_id);
    let in_internal = INTERNAL_OPS.contains(&operation_id);
    match (in_app, in_internal) {
        (true, true) => Err(format!(
            "{operation_id} is in both the app and internal lists"
        )),
        (true, false) => Ok(Some("app")),
        (false, true) => Ok(None),
        (false, false) => match first_tag {
            Some(t) if ADMIN_TAGS.contains(&t) => Ok(Some("admin")),
            Some(t) if INTERNAL_TAGS.contains(&t) => Ok(None),
            _ => Err(format!(
                "{operation_id} (tag {first_tag:?}) is unclassified: add it to APP_OPS or INTERNAL_OPS, or its tag to ADMIN_TAGS or INTERNAL_TAGS in shared/sdk_class.rs"
            )),
        },
    }
}

/// Stamp `x-sdk` on every app and admin operation of `doc` and return what the
/// lists failed to decide (empty when every operation is classified). An
/// undecided operation is left unstamped, i.e. internal; the tests fail on it.
pub fn classify(doc: &mut Value) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(paths) = doc.get_mut("paths").and_then(Value::as_object_mut) else {
        return problems;
    };
    for item in paths.values_mut() {
        let Some(item) = item.as_object_mut() else {
            continue;
        };
        for (method, op) in item.iter_mut() {
            if !matches!(method.as_str(), "get" | "put" | "post" | "delete" | "patch") {
                continue;
            }
            let id = op
                .get("operationId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let tag = op
                .pointer("/tags/0")
                .and_then(Value::as_str)
                .map(str::to_string);
            match class_of(&id, tag.as_deref()) {
                Ok(Some(class)) => {
                    if let Some(obj) = op.as_object_mut() {
                        obj.insert(EXTENSION.to_string(), json!(class));
                    }
                }
                Ok(None) => {}
                Err(why) => problems.push(why),
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lists_decide_each_class() {
        assert_eq!(class_of("syncRoles", Some("roles")), Ok(Some("app")));
        assert_eq!(class_of("createRole", Some("roles")), Ok(Some("admin")));
        assert_eq!(class_of("getRouterConfig", Some("dispatch")), Ok(None));
        assert_eq!(class_of("postAuthLogin", Some("auth")), Ok(None));
        // An app operation wins over its tag being an admin area.
        assert_eq!(class_of("listRoles", Some("roles")), Ok(Some("app")));
        // An internal operation wins over its tag being an admin area.
        assert_eq!(class_of("listEventsRaw", Some("events")), Ok(None));
    }

    #[test]
    fn an_undecided_operation_is_an_error() {
        assert!(class_of("somethingNew", Some("brand-new-area")).is_err());
        assert!(class_of("somethingNew", None).is_err());
    }

    #[test]
    fn the_app_and_internal_lists_do_not_overlap() {
        for id in APP_OPS {
            assert!(!INTERNAL_OPS.contains(id), "{id} is in both lists");
        }
    }

    #[test]
    fn classify_stamps_the_extension_and_reports_the_undecided() {
        let mut doc = json!({ "paths": {
            "/a": { "post": { "operationId": "syncRoles", "tags": ["roles"] } },
            "/b": { "get": { "operationId": "listClientsX", "tags": ["clients"] } },
            "/c": { "get": { "operationId": "getRouterConfig", "tags": ["dispatch"] } },
            "/d": { "get": { "operationId": "newThing", "tags": ["unknown"] } },
        } });
        let problems = classify(&mut doc);
        assert_eq!(doc["paths"]["/a"]["post"]["x-sdk"], "app");
        assert_eq!(doc["paths"]["/b"]["get"]["x-sdk"], "admin");
        assert!(doc["paths"]["/c"]["get"].get("x-sdk").is_none());
        assert!(doc["paths"]["/d"]["get"].get("x-sdk").is_none());
        assert_eq!(problems.len(), 1, "{problems:?}");
    }
}

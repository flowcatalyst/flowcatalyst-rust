//! Definition types for syncing FlowCatalyst primitives to the platform.
//!
//! These are the user-facing builder types. Each `*Definition` describes one
//! resource declaratively; the orchestrator translates a bundle of them into
//! the platform's per-category sync wire shapes.
//!
//! All structs ship `make(...)` plus a chain of `with_*` builders so they can
//! be assembled inline at the call site.

use crate::client::dispatch_pools::SyncDispatchPoolItem;
use crate::client::principals::SyncPrincipalItem;
use crate::client::processes::SyncProcessInput;
use crate::client::roles::SyncRoleItem;
use crate::client::scheduled_jobs::SyncScheduledJobItem;
use crate::client::subscriptions::{SyncEventTypeBinding, SyncSubscriptionItem};
use crate::client::CreateEventTypeRequest;

// ───────────────────────────────────────────────────────────────────────────
// Role
// ───────────────────────────────────────────────────────────────────────────

/// A role declaration.
///
/// `name` is the short name — the platform stores it prefixed with the
/// application code (`"orders:" + name`). Do NOT include the prefix yourself.
#[derive(Debug, Clone, Default)]
pub struct RoleDefinition {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub client_managed: bool,
}

impl RoleDefinition {
    pub fn make(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn with_client_managed(mut self, client_managed: bool) -> Self {
        self.client_managed = client_managed;
        self
    }

    pub(crate) fn into_wire(self) -> SyncRoleItem {
        SyncRoleItem {
            name: self.name,
            display_name: self.display_name,
            description: self.description,
            permissions: self.permissions,
            client_managed: self.client_managed,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// EventType
// ───────────────────────────────────────────────────────────────────────────

/// An event-type declaration. `code` must be the full four-segment identifier
/// `{app}:{subdomain}:{aggregate}:{event}`.
#[derive(Debug, Clone, Default)]
pub struct EventTypeDefinition {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
}

impl EventTypeDefinition {
    pub fn make(code: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            name: name.into(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub(crate) fn into_wire(self) -> CreateEventTypeRequest {
        CreateEventTypeRequest {
            code: self.code,
            name: self.name,
            description: self.description,
            schema: None,
            client_id: None,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Subscription
// ───────────────────────────────────────────────────────────────────────────

/// A single event-type binding inside a subscription.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionBinding {
    pub event_type_code: String,
    pub filter: Option<String>,
}

impl SubscriptionBinding {
    pub fn make(event_type_code: impl Into<String>) -> Self {
        Self {
            event_type_code: event_type_code.into(),
            filter: None,
        }
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    pub(crate) fn into_wire(self) -> SyncEventTypeBinding {
        SyncEventTypeBinding {
            event_type_code: self.event_type_code,
            filter: self.filter,
        }
    }
}

/// A subscription declaration. Either `target` (webhook URL) or
/// `connection_id` (named connection) must end up populated.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionDefinition {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub target: String,
    pub connection_id: Option<String>,
    pub event_types: Vec<SubscriptionBinding>,
    pub dispatch_pool_code: Option<String>,
    pub mode: Option<String>,
    pub max_retries: Option<u32>,
    pub timeout_seconds: Option<u32>,
    pub data_only: bool,
}

impl SubscriptionDefinition {
    pub fn make(
        code: impl Into<String>,
        name: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            name: name.into(),
            target: target.into(),
            ..Self::default()
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_connection_id(mut self, connection_id: impl Into<String>) -> Self {
        self.connection_id = Some(connection_id.into());
        self
    }

    pub fn with_event_types(mut self, bindings: Vec<SubscriptionBinding>) -> Self {
        self.event_types = bindings;
        self
    }

    pub fn add_event_type(mut self, binding: SubscriptionBinding) -> Self {
        self.event_types.push(binding);
        self
    }

    pub fn with_dispatch_pool_code(mut self, code: impl Into<String>) -> Self {
        self.dispatch_pool_code = Some(code.into());
        self
    }

    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = Some(mode.into());
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = Some(max_retries);
        self
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = Some(timeout_seconds);
        self
    }

    pub fn with_data_only(mut self, data_only: bool) -> Self {
        self.data_only = data_only;
        self
    }

    pub(crate) fn into_wire(self) -> SyncSubscriptionItem {
        SyncSubscriptionItem {
            code: self.code,
            name: self.name,
            description: self.description,
            target: self.target,
            connection_id: self.connection_id,
            event_types: self
                .event_types
                .into_iter()
                .map(SubscriptionBinding::into_wire)
                .collect(),
            dispatch_pool_code: self.dispatch_pool_code,
            mode: self.mode,
            max_retries: self.max_retries,
            timeout_seconds: self.timeout_seconds,
            data_only: self.data_only,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// DispatchPool
// ───────────────────────────────────────────────────────────────────────────

/// A dispatch-pool declaration.
#[derive(Debug, Clone, Default)]
pub struct DispatchPoolDefinition {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    /// Concurrent in-flight deliveries. Defaults to 10 server-side.
    pub concurrency: Option<u32>,
    /// Requests per minute. `None` = concurrency-only (no rate limit).
    pub rate_limit: Option<u32>,
}

impl DispatchPoolDefinition {
    pub fn make(code: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_concurrency(mut self, concurrency: u32) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    pub fn with_rate_limit(mut self, rate_limit: u32) -> Self {
        self.rate_limit = Some(rate_limit);
        self
    }

    pub(crate) fn into_wire(self) -> SyncDispatchPoolItem {
        SyncDispatchPoolItem {
            code: self.code,
            name: Some(self.name),
            concurrency: self.concurrency,
            rate_limit: self.rate_limit,
            description: self.description,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Principal
// ───────────────────────────────────────────────────────────────────────────

/// A principal (user) declaration. Matched by email.
#[derive(Debug, Clone)]
pub struct PrincipalDefinition {
    pub email: String,
    pub name: String,
    /// Role short names (no `<app>:` prefix — the platform adds it).
    pub roles: Vec<String>,
    /// Defaults to `true` if not set.
    pub active: Option<bool>,
    /// Optional pre-hashed password, used only when the sync creates the
    /// user (never applied to an existing one).
    pub password_hash: Option<String>,
}

impl PrincipalDefinition {
    pub fn make(email: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            name: name.into(),
            roles: Vec::new(),
            active: None,
            password_hash: None,
        }
    }

    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }

    pub fn with_active(mut self, active: bool) -> Self {
        self.active = Some(active);
        self
    }

    /// A pre-hashed password (bcrypt, argon2) for the platform to store if,
    /// and only if, the sync creates this user.
    pub fn with_password_hash(mut self, hash: impl Into<String>) -> Self {
        self.password_hash = Some(hash.into());
        self
    }

    pub(crate) fn into_wire(self) -> SyncPrincipalItem {
        SyncPrincipalItem {
            email: self.email,
            name: self.name,
            roles: self.roles,
            active: self.active.unwrap_or(true),
            password_hash: self.password_hash,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Process
// ───────────────────────────────────────────────────────────────────────────

/// A process (workflow documentation) declaration. `code` is the full
/// three-segment identifier `{app}:{subdomain}:{process-name}`.
#[derive(Debug, Clone, Default)]
pub struct ProcessDefinition {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    /// Diagram source. Stored verbatim — typically Mermaid.
    pub body: String,
    /// Diagram language. Defaults to `mermaid` if `None`.
    pub diagram_type: Option<String>,
    pub tags: Vec<String>,
}

impl ProcessDefinition {
    pub fn make(code: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_diagram_type(mut self, diagram_type: impl Into<String>) -> Self {
        self.diagram_type = Some(diagram_type.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub(crate) fn into_wire(self) -> SyncProcessInput {
        SyncProcessInput {
            code: self.code,
            name: self.name,
            description: self.description,
            body: self.body,
            diagram_type: self.diagram_type,
            tags: self.tags,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Scheduled Job
// ───────────────────────────────────────────────────────────────────────────

/// A scheduled-job declaration.
///
/// `code` is the full identifier the platform uses (the per-application
/// convention is `{app}:{job-name}`, but the SDK doesn't enforce a shape).
/// `crons` accepts standard 5-field cron expressions; the platform's
/// scheduler evaluates them in `timezone` (defaults to UTC).
#[derive(Debug, Clone, Default)]
pub struct ScheduledJobDefinition {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub crons: Vec<String>,
    pub timezone: Option<String>,
    pub payload: Option<serde_json::Value>,
    /// `true` lets the platform fire a new tick even when the previous
    /// invocation is still running. Default `false` — see the SDK's
    /// `LockProvider` for in-app de-dupe of concurrent fires.
    pub concurrent: bool,
    /// `true` if the consumer reports back via
    /// `POST /api/scheduled-jobs/instances/{id}/complete`. The platform
    /// then tracks per-instance completion status instead of treating
    /// the webhook delivery as the success signal.
    pub tracks_completion: bool,
    pub timeout_seconds: Option<i32>,
    pub delivery_max_attempts: Option<i32>,
    pub target_url: Option<String>,
}

impl ScheduledJobDefinition {
    pub fn make(code: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_crons(mut self, crons: Vec<String>) -> Self {
        self.crons = crons;
        self
    }

    pub fn add_cron(mut self, cron: impl Into<String>) -> Self {
        self.crons.push(cron.into());
        self
    }

    pub fn with_timezone(mut self, timezone: impl Into<String>) -> Self {
        self.timezone = Some(timezone.into());
        self
    }

    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn with_concurrent(mut self, concurrent: bool) -> Self {
        self.concurrent = concurrent;
        self
    }

    pub fn with_tracks_completion(mut self, tracks_completion: bool) -> Self {
        self.tracks_completion = tracks_completion;
        self
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: i32) -> Self {
        self.timeout_seconds = Some(timeout_seconds);
        self
    }

    pub fn with_delivery_max_attempts(mut self, attempts: i32) -> Self {
        self.delivery_max_attempts = Some(attempts);
        self
    }

    pub fn with_target_url(mut self, target_url: impl Into<String>) -> Self {
        self.target_url = Some(target_url.into());
        self
    }

    pub(crate) fn into_wire(self) -> SyncScheduledJobItem {
        SyncScheduledJobItem {
            code: self.code,
            name: self.name,
            description: self.description,
            crons: self.crons,
            timezone: self.timezone,
            payload: self.payload,
            concurrent: self.concurrent,
            tracks_completion: self.tracks_completion,
            timeout_seconds: self.timeout_seconds,
            delivery_max_attempts: self.delivery_max_attempts,
            target_url: self.target_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Owner decision 22 of 2026-09-25: a sync carries a password hash for
    /// users it creates; the platform reports the existing users it ignored.
    #[test]
    fn principal_password_hash_is_sent_only_when_set_and_ignored_list_is_read() {
        let plain = PrincipalDefinition::make("a@example.com", "A").into_wire();
        assert!(serde_json::to_value(&plain)
            .unwrap()
            .get("passwordHash")
            .is_none());

        let hashed = PrincipalDefinition::make("a@example.com", "A")
            .with_password_hash("$2y$10$hash")
            .into_wire();
        assert_eq!(
            serde_json::to_value(&hashed).unwrap()["passwordHash"],
            "$2y$10$hash"
        );

        let result: crate::client::SyncResult = serde_json::from_value(serde_json::json!({
            "applicationCode": "hr", "created": 0, "updated": 1, "deleted": 0,
            "syncedCodes": ["a@example.com"], "passwordHashIgnored": ["a@example.com"]
        }))
        .unwrap();
        assert_eq!(
            result.password_hash_ignored,
            vec!["a@example.com".to_string()]
        );

        let omitted: crate::client::SyncResult = serde_json::from_value(serde_json::json!({
            "applicationCode": "hr", "created": 1, "updated": 0, "deleted": 0,
            "syncedCodes": ["b@example.com"]
        }))
        .unwrap();
        assert!(omitted.password_hash_ignored.is_empty());
    }

    #[test]
    fn role_definition_wire_round_trip() {
        let def = RoleDefinition::make("admin")
            .with_display_name("Administrator")
            .with_description("Full admin")
            .with_permissions(vec!["orders:admin:*:*".into()])
            .with_client_managed(true);
        let wire = def.into_wire();
        let json = serde_json::to_value(&wire).unwrap();
        assert_eq!(json["name"], "admin");
        assert_eq!(json["displayName"], "Administrator");
        assert_eq!(json["clientManaged"], true);
        assert_eq!(json["permissions"][0], "orders:admin:*:*");
    }

    #[test]
    fn dispatch_pool_wire_uses_camel_case_and_skips_none() {
        let def = DispatchPoolDefinition::make("fast", "Fast Pool").with_concurrency(20);
        let wire = def.into_wire();
        let json = serde_json::to_value(&wire).unwrap();
        assert_eq!(json["code"], "fast");
        assert_eq!(json["name"], "Fast Pool");
        assert_eq!(json["concurrency"], 20);
        // rate_limit was never set — must be absent (skip_serializing_if).
        assert!(json.get("rateLimit").is_none());
    }

    #[test]
    fn principal_active_defaults_to_true_when_unset() {
        let def = PrincipalDefinition::make("u@example.com", "User");
        assert_eq!(def.into_wire().active, true);

        let def_false = PrincipalDefinition::make("u@example.com", "User").with_active(false);
        assert_eq!(def_false.into_wire().active, false);
    }

    #[test]
    fn process_wire_carries_body_verbatim() {
        let def = ProcessDefinition::make("orders:fulfilment:flow", "Flow")
            .with_body("graph TD\n  A --> B")
            .with_tags(vec!["core".into()]);
        let wire = def.into_wire();
        assert_eq!(wire.body, "graph TD\n  A --> B");
        assert_eq!(wire.tags, vec!["core".to_string()]);
        // diagram_type defaults to None → platform applies `mermaid`.
        assert!(wire.diagram_type.is_none());
    }

    #[test]
    fn subscription_collects_bindings_and_serializes_camel_case() {
        let def = SubscriptionDefinition::make(
            "ship-handler",
            "Ship Handler",
            "https://example.com/hook",
        )
        .add_event_type(
            SubscriptionBinding::make("orders:fulfilment:shipment:shipped")
                .with_filter("subject like 'orders.%'"),
        )
        .with_max_retries(5)
        .with_data_only(true);
        let wire = def.into_wire();
        let json = serde_json::to_value(&wire).unwrap();
        assert_eq!(json["code"], "ship-handler");
        assert_eq!(json["maxRetries"], 5);
        assert_eq!(json["dataOnly"], true);
        assert_eq!(
            json["eventTypes"][0]["eventTypeCode"],
            "orders:fulfilment:shipment:shipped"
        );
        assert_eq!(
            json["eventTypes"][0]["filter"],
            "subject like 'orders.%'"
        );
    }
}

//! What promoting a manifest to an alias would do (Java
//! `function/operations/PromotePlan.java`, spec
//! `function-manifest-authoring.md` M2.1), read from current state and never
//! applied by itself. [`TriggerSync::plan`](super::TriggerSync::plan)
//! computes it with no writes; [`TriggerSync::apply`](super::TriggerSync::apply)
//! performs exactly this plan. Promote and the `manifest/check` dry run
//! share this one computation.
//!
//! A named alias's wiring is [`Wiring::HttpOnly`]: promoting it changes no
//! pool, subscription, schedule or public route.

use crate::usecase::UseCaseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotePlan {
    pub alias: String,
    /// The alias's version before this promote; `None` when it is unset.
    pub from_version: Option<i32>,
    pub to_version: i32,
    /// Config and secret keys the settings check would find missing, in
    /// first-seen order. Never by itself a conflict.
    pub settings_missing: Vec<String>,
    pub wiring: Wiring,
    /// What would make applying this plan fail.
    pub conflicts: Vec<Conflict>,
}

/// `PUBLIC_ROUTE_TAKEN` or `TRIGGER_KEY_COLLISION`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub code: String,
    pub message: String,
}

impl Conflict {
    /// The error applying the plan fails with (Java
    /// `FunctionTriggerSync.conflictException`): a trigger-key collision is
    /// an internal error, anything else a `409`.
    pub fn to_error(&self) -> UseCaseError {
        if self.code == "TRIGGER_KEY_COLLISION" {
            UseCaseError::internal(self.code.clone(), self.message.clone())
        } else {
            UseCaseError::business_rule(self.code.clone(), self.message.clone())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wiring {
    Live {
        pool: PoolAction,
        subscriptions: Vec<SubscriptionAction>,
        schedules: Vec<ScheduleAction>,
        public_routes: PublicRoutesAction,
    },
    HttpOnly,
}

/// One pool per function, never deleted by a promote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolAction {
    Create {
        key: String,
    },
    Update {
        key: String,
        changed_fields: Vec<String>,
    },
    Unchanged {
        key: String,
    },
}

/// Per event type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionAction {
    Create {
        trigger_key: String,
        event_type: String,
    },
    Update {
        trigger_key: String,
        event_type: String,
        changed_fields: Vec<String>,
    },
    Delete {
        trigger_key: String,
        event_type: String,
    },
    Unchanged {
        trigger_key: String,
        event_type: String,
    },
}

impl SubscriptionAction {
    pub fn trigger_key(&self) -> &str {
        match self {
            Self::Create { trigger_key, .. }
            | Self::Update { trigger_key, .. }
            | Self::Delete { trigger_key, .. }
            | Self::Unchanged { trigger_key, .. } => trigger_key,
        }
    }
}

/// Per `(cron, timezone)`; `timezone` is `None` when the manifest names none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleAction {
    Create {
        trigger_key: String,
        cron: String,
        timezone: Option<String>,
    },
    Update {
        trigger_key: String,
        cron: String,
        timezone: Option<String>,
        changed_fields: Vec<String>,
    },
    Delete {
        trigger_key: String,
        cron: String,
        timezone: Option<String>,
    },
    Unchanged {
        trigger_key: String,
        cron: String,
        timezone: Option<String>,
    },
}

impl ScheduleAction {
    pub fn trigger_key(&self) -> &str {
        match self {
            Self::Create { trigger_key, .. }
            | Self::Update { trigger_key, .. }
            | Self::Delete { trigger_key, .. }
            | Self::Unchanged { trigger_key, .. } => trigger_key,
        }
    }
}

/// One set-valued diff, not one action per route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicRoutesAction {
    Replace {
        added: Vec<RouteKey>,
        removed: Vec<RouteKey>,
    },
    Unchanged,
}

/// A route's identity, `aliasPrefixes` included: a route that only gained or
/// lost a prefix is still a difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteKey {
    pub hostname: String,
    pub path_prefix: String,
    pub alias_prefixes: Vec<String>,
}

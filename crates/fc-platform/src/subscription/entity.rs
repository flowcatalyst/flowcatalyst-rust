//! Subscription Entity — matches TypeScript Subscription domain

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubscriptionStatus {
    Active,
    Paused,
}

impl Default for SubscriptionStatus {
    fn default() -> Self { Self::Active }
}

impl SubscriptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Paused => "PAUSED",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "PAUSED" => Self::Paused,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubscriptionSource {
    Code,
    Api,
    Ui,
}

impl Default for SubscriptionSource {
    fn default() -> Self { Self::Ui }
}

impl SubscriptionSource {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Code => "CODE", Self::Api => "API", Self::Ui => "UI" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "CODE" => Self::Code, "API" => Self::Api, _ => Self::Ui }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DispatchMode {
    Immediate,
    BlockOnError,
}

impl Default for DispatchMode {
    fn default() -> Self { Self::Immediate }
}

impl DispatchMode {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Immediate => "IMMEDIATE", Self::BlockOnError => "BLOCK_ON_ERROR" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "BLOCK_ON_ERROR" => Self::BlockOnError, _ => Self::Immediate }
    }
}

/// Event type binding stored in msg_subscription_event_types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeBinding {
    pub event_type_id: Option<String>,
    pub event_type_code: String,
    pub spec_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

impl EventTypeBinding {
    pub fn new(event_type_code: impl Into<String>) -> Self {
        Self {
            event_type_id: None,
            event_type_code: event_type_code.into(),
            spec_version: None,
            filter: None,
        }
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    pub fn matches(&self, event_type_code: &str) -> bool {
        let pattern_parts: Vec<&str> = self.event_type_code.split(':').collect();
        let event_parts: Vec<&str> = event_type_code.split(':').collect();
        if pattern_parts.len() != event_parts.len() {
            return false;
        }
        pattern_parts.iter().zip(event_parts.iter()).all(|(p, e)| *p == "*" || p == e)
    }
}

/// Custom configuration entry stored in msg_subscription_custom_configs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

/// Subscription domain entity — matches TypeScript Subscription interface
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub code: String,
    pub application_code: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub client_scoped: bool,
    pub event_types: Vec<EventTypeBinding>,
    /// Connection ID — references msg_connections
    pub connection_id: String,
    pub queue: Option<String>,
    pub custom_config: Vec<ConfigEntry>,
    pub source: SubscriptionSource,
    pub status: SubscriptionStatus,
    pub max_age_seconds: i32,
    pub dispatch_pool_id: Option<String>,
    pub dispatch_pool_code: Option<String>,
    pub delay_seconds: i32,
    pub sequence: i32,
    pub mode: DispatchMode,
    pub timeout_seconds: i32,
    pub max_retries: i32,
    pub service_account_id: Option<String>,
    pub data_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Subscription {
    pub fn new(code: impl Into<String>, name: impl Into<String>, connection_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::Subscription),
            code: code.into(),
            application_code: None,
            name: name.into(),
            description: None,
            client_id: None,
            client_identifier: None,
            client_scoped: false,
            event_types: vec![],
            connection_id: connection_id.into(),
            queue: None,
            custom_config: vec![],
            source: SubscriptionSource::Ui,
            status: SubscriptionStatus::Active,
            max_age_seconds: 86400,
            dispatch_pool_id: None,
            dispatch_pool_code: None,
            delay_seconds: 0,
            sequence: 99,
            mode: DispatchMode::Immediate,
            timeout_seconds: 30,
            max_retries: 3,
            service_account_id: None,
            data_only: true,
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self { self.description = Some(desc.into()); self }
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self { self.client_id = Some(id.into()); self }
    pub fn with_dispatch_pool_id(mut self, id: impl Into<String>) -> Self { self.dispatch_pool_id = Some(id.into()); self }
    pub fn with_service_account_id(mut self, id: impl Into<String>) -> Self { self.service_account_id = Some(id.into()); self }
    pub fn with_mode(mut self, mode: DispatchMode) -> Self { self.mode = mode; self }
    pub fn with_data_only(mut self, data_only: bool) -> Self { self.data_only = data_only; self }
    pub fn with_event_type_binding(mut self, binding: EventTypeBinding) -> Self { self.event_types.push(binding); self }

    pub fn matches_event_type(&self, event_type_code: &str) -> bool {
        self.event_types.iter().any(|b| b.matches(event_type_code))
    }

    pub fn matches_client(&self, client_id: Option<&str>) -> bool {
        match (&self.client_id, client_id) {
            (None, _) => true,
            (Some(sub_client), Some(event_client)) => sub_client == event_client,
            (Some(_), None) => false,
        }
    }

    pub fn pause(&mut self) { self.status = SubscriptionStatus::Paused; self.updated_at = Utc::now(); }
    pub fn resume(&mut self) { self.status = SubscriptionStatus::Active; self.updated_at = Utc::now(); }
    pub fn is_active(&self) -> bool { self.status == SubscriptionStatus::Active }
}

impl From<crate::entities::msg_subscriptions::Model> for Subscription {
    fn from(m: crate::entities::msg_subscriptions::Model) -> Self {
        Self {
            id: m.id,
            code: m.code,
            application_code: m.application_code,
            name: m.name,
            description: m.description,
            client_id: m.client_id,
            client_identifier: m.client_identifier,
            client_scoped: m.client_scoped,
            event_types: vec![], // loaded separately
            connection_id: m.connection_id.unwrap_or_default(),
            queue: m.queue,
            custom_config: vec![], // loaded separately
            source: SubscriptionSource::from_str(&m.source),
            status: SubscriptionStatus::from_str(&m.status),
            max_age_seconds: m.max_age_seconds,
            dispatch_pool_id: m.dispatch_pool_id,
            dispatch_pool_code: m.dispatch_pool_code,
            delay_seconds: m.delay_seconds,
            sequence: m.sequence,
            mode: DispatchMode::from_str(&m.mode),
            timeout_seconds: m.timeout_seconds,
            max_retries: m.max_retries,
            service_account_id: m.service_account_id,
            data_only: m.data_only,
            created_by: None,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}

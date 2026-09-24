//! Process Domain Events

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

fn subject(id: &str) -> String {
    format!("platform.process.{}", id)
}
fn message_group(id: &str) -> String {
    format!("platform:process:{}", id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub process_id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub application: String,
    pub subdomain: String,
    pub process_name: String,
}

impl_domain_event!(ProcessCreated);

impl ProcessCreated {
    const EVENT_TYPE: &'static str = "platform:admin:process:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, process_id: &str) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            Self::SPEC_VERSION,
            Self::SOURCE,
            subject(process_id),
            message_group(process_id),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub process_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_changed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl_domain_event!(ProcessUpdated);

impl ProcessUpdated {
    const EVENT_TYPE: &'static str = "platform:admin:process:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, process_id: &str) -> EventMetadata {
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            Self::SPEC_VERSION,
            Self::SOURCE,
            subject(process_id),
            message_group(process_id),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessArchived {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub process_id: String,
    pub code: String,
}

impl_domain_event!(ProcessArchived);

impl ProcessArchived {
    const EVENT_TYPE: &'static str = "platform:admin:process:archived";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, process_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject(process_id),
                message_group(process_id),
            ),
            process_id: process_id.to_string(),
            code: code.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub process_id: String,
    pub code: String,
}

impl_domain_event!(ProcessDeleted);

impl ProcessDeleted {
    const EVENT_TYPE: &'static str = "platform:admin:process:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(ctx: &ExecutionContext, process_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject(process_id),
                message_group(process_id),
            ),
            process_id: process_id.to_string(),
            code: code.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessesSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(ProcessesSynced);

impl ProcessesSynced {
    const EVENT_TYPE: &'static str = "platform:admin:processes:synced";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:admin";

    pub fn new(
        ctx: &ExecutionContext,
        application_code: &str,
        created: u32,
        updated: u32,
        deleted: u32,
        synced_codes: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                format!("platform.application.{}", application_code),
                format!("platform:application:{}", application_code),
            ),
            application_code: application_code.to_string(),
            created,
            updated,
            deleted,
            synced_codes,
        }
    }
}

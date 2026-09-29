//! Publishing events on a function's behalf, for every runtime (Java
//! `ControlPlaneEvents` over the reconciler's own control plane): the
//! host's own checks (type and dedup id present, data is JSON), the emit
//! defaults for correlation and causation ids, then `POST
//! /control/functions/events`. The WASM runtime's `flowcatalyst:function/events`
//! and the JS runtime's `events` module both come here, so a refusal reads
//! the same whichever runtime a function is written for.

use std::sync::Arc;

use fc_function_abi::{emit_error, EventEmitError, FunctionAddress};
use serde_json::Value;
use tokio::runtime::Handle;

use crate::control_plane::{ControlPlane, EmitItem, EmitRequest};
use crate::java;

/// An event a function publishes: the `flowcatalyst:function/events`
/// `outbound-event` record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutboundEvent {
    /// The event type code; must not be blank.
    pub event_type: String,
    /// Accepted for forward compatibility; not carried by the events route.
    pub source: Option<String>,
    pub subject: Option<String>,
    /// Accepted for forward compatibility; not carried by the events route.
    pub data_content_type: Option<String>,
    /// The payload as JSON text; `None` is an event without one.
    pub data: Option<String>,
    /// `None` takes the invocation's correlation id.
    pub correlation_id: Option<String>,
    /// `None` takes the invocation's causation id.
    pub causation_id: Option<String>,
    pub message_group: Option<String>,
    /// Must not be blank.
    pub dedup_id: String,
}

/// Why the host did not publish an event: its own refusal before the
/// platform (an `INVALID_EVENT…` or `DEDUP_ID_REQUIRED` code), or the
/// platform's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum EmitFailure {
    Invalid(String),
    Platform(EventEmitError),
}

impl EmitFailure {
    /// Whether the platform could not be reached (worth a retry).
    pub fn is_unavailable(&self) -> bool {
        matches!(self, EmitFailure::Platform(e) if e.code() == emit_error::UNAVAILABLE)
    }
}

/// Where emitted events go: the control plane, spoken for one loaded
/// version. The call runs on the host's own runtime (not a guest's), so the
/// control plane's connections live with the reconciler's.
#[derive(Clone)]
pub struct Emitter {
    pub control_plane: Arc<dyn ControlPlane>,
    pub host_id: String,
    pub host_runtime: Option<Handle>,
}

impl Emitter {
    /// Publishes `event` for `address` at `version`, with `defaults` as the
    /// `(correlationId, causationId)` it falls back to. The id the platform
    /// stored it under.
    pub async fn emit(
        &self,
        address: &FunctionAddress,
        version: i32,
        event: OutboundEvent,
        defaults: (String, Option<String>),
    ) -> Result<String, EmitFailure> {
        if java::is_blank(&event.event_type) {
            return Err(EmitFailure::Invalid(
                emit_error::INVALID_EVENT_TYPE_REQUIRED.to_owned(),
            ));
        }
        if java::is_blank(&event.dedup_id) {
            return Err(EmitFailure::Invalid(
                emit_error::DEDUP_ID_REQUIRED.to_owned(),
            ));
        }
        let data = match &event.data {
            None => Value::Null,
            Some(text) => serde_json::from_str(text).map_err(|_| {
                EmitFailure::Invalid(emit_error::INVALID_EVENT_DATA_NOT_JSON.to_owned())
            })?,
        };
        let (correlation_id, causation_id) = defaults;
        let request = EmitRequest {
            host_id: self.host_id.clone(),
            address: address.clone(),
            version,
            events: vec![EmitItem {
                event_type: event.event_type,
                subject: event.subject,
                dedup_id: event.dedup_id,
                data,
                correlation_id: event.correlation_id.or(Some(correlation_id)),
                causation_id: event.causation_id.or(causation_id),
                message_group: event.message_group,
            }],
        };
        let control_plane = self.control_plane.clone();
        let sent = match &self.host_runtime {
            Some(runtime) => runtime
                .spawn(async move { control_plane.emit(&request).await })
                .await
                .unwrap_or_else(|_| Err(EventEmitError::unavailable())),
            None => control_plane.emit(&request).await,
        };
        sent.map_err(EmitFailure::Platform)
    }
}

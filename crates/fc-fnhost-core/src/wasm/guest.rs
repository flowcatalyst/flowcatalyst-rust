//! A store's data: WASI (no preopens, no environment, no arguments, no
//! sockets; clocks and random), `wasi:http` with the egress policy, the
//! memory limits, and the host side of the `flowcatalyst:function`
//! interfaces (`wit/flowcatalyst-function`).

use std::collections::HashMap;
use std::sync::Arc;

use fc_function_abi::{emit_error, Caller, FunctionAddress};
use wasmtime::component::{HasSelf, Linker, ResourceTable};
use wasmtime::StoreLimits;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use super::egress::EgressHooks;
use super::output::GuestLogger;

wasmtime::component::bindgen!({
    path: "../../wit/flowcatalyst-function",
    world: "imports",
    imports: {
        "flowcatalyst:function/events.emit": async,
        "flowcatalyst:function/events.emit-event": async,
    },
});

use flowcatalyst::function::{config, events, invocation, log, secrets};

/// The host side of every import a function can have: WASI 0.2 (the proxy
/// and CLI sets), `wasi:http`, and `flowcatalyst:function`. A component that
/// imports only part of it (a pure `wasi:http/proxy` one imports none of
/// ours) links against the same linker.
pub fn linker(engine: &wasmtime::Engine) -> wasmtime::Result<Linker<GuestState>> {
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
    Imports::add_to_linker::<GuestState, HasSelf<GuestState>>(&mut linker, |state| state)?;
    Ok(linker)
}

pub use crate::emit::Emitter;

/// What every invocation of one loaded version shares. Holds secret
/// values: deliberately not `Debug`.
pub struct FunctionShared {
    pub address: FunctionAddress,
    pub version: i32,
    pub logger: GuestLogger,
    /// Only the keys the manifest declares.
    pub config: HashMap<String, String>,
    /// Only the keys the manifest declares, and only non-empty values.
    pub secrets: HashMap<String, String>,
    pub allow: Arc<super::egress::HttpAllowlist>,
    /// Each memory's `StoreLimits` cap, in bytes: `limits.wasmMemoryMb`, or
    /// the component's own declared maximum when that is smaller.
    pub memory_limit: usize,
    /// The response body's cap, in bytes: `limits.wasmMemoryMb`.
    pub response_cap: usize,
    pub emitter: Emitter,
}

use crate::emit::EmitFailure;

impl FunctionShared {
    /// Publishes `event` (the WIT record) through the shared [`Emitter`].
    async fn emit(
        self: Arc<Self>,
        event: events::OutboundEvent,
        defaults: (String, Option<String>),
    ) -> Result<String, EmitFailure> {
        let event = crate::emit::OutboundEvent {
            event_type: event.type_,
            source: event.source,
            subject: event.subject,
            data_content_type: event.data_content_type,
            data: event.data,
            correlation_id: event.correlation_id,
            causation_id: event.causation_id,
            message_group: event.message_group,
            dedup_id: event.dedup_id,
        };
        self.emitter
            .emit(&self.address, self.version, event, defaults)
            .await
    }
}

/// The host's own code for event data that is not JSON (Java's host reads
/// the whole event as JSON, so its nearest code is `INVALID_EVENT: not JSON`).
pub const INVALID_EVENT_DATA_NOT_JSON: &str = emit_error::INVALID_EVENT_DATA_NOT_JSON;

/// What the guest can learn about its invocation, and the emit defaults.
pub struct InvocationData {
    pub context: invocation::InvocationContext,
}

impl InvocationData {
    pub fn from(context: &crate::invoke::InvocationContext) -> Self {
        let caller = match &context.caller {
            Caller::Platform => invocation::Caller::Platform,
            Caller::Anonymous => invocation::Caller::Anonymous,
            Caller::Principal(p) => invocation::Caller::Principal(invocation::Principal {
                id: p.id.clone(),
                principal_type: p.principal_type.clone(),
                tier: p.tier.clone(),
                clients: p.clients.clone(),
                roles: p.roles.clone(),
                applications: p.applications.clone(),
                all_applications: p.all_applications,
                permissions: p.permissions.iter().cloned().collect(),
            }),
        };
        Self {
            context: invocation::InvocationContext {
                invocation_id: context.invocation_id.clone(),
                address: context.address.render(),
                version: context.version,
                caller,
                correlation_id: context.correlation_id.clone(),
                causation_id: context.causation_id.clone(),
                original_host: context.original_host.clone(),
                original_path: context.original_path.clone(),
                remote_address: context.remote_address.clone(),
                path_params: context
                    .path_params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            },
        }
    }
}

/// One store's data.
pub struct GuestState {
    pub wasi: WasiCtx,
    pub http: WasiHttpCtx,
    pub table: ResourceTable,
    pub limits: StoreLimits,
    pub hooks: EgressHooks,
    pub function: Arc<FunctionShared>,
    pub invocation: InvocationData,
}

impl WasiView for GuestState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for GuestState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}

impl config::Host for GuestState {
    fn get(&mut self, key: String) -> Option<String> {
        self.function.config.get(&key).cloned()
    }
}

/// Never logs: a secret value must not reach a log line.
impl secrets::Host for GuestState {
    fn get(&mut self, key: String) -> Option<String> {
        self.function.secrets.get(&key).cloned()
    }
}

impl log::Host for GuestState {
    fn log(&mut self, level: log::Level, message: String) {
        let level = match level {
            log::Level::Trace => tracing::Level::TRACE,
            log::Level::Debug => tracing::Level::DEBUG,
            log::Level::Info => tracing::Level::INFO,
            log::Level::Warn => tracing::Level::WARN,
            log::Level::Error => tracing::Level::ERROR,
        };
        self.function.logger.line(level, &message);
    }
}

impl GuestState {
    fn emit_defaults(&self) -> (String, Option<String>) {
        (
            self.invocation.context.correlation_id.clone(),
            self.invocation.context.causation_id.clone(),
        )
    }
}

impl events::Host for GuestState {
    /// 0.1.0's emit: no id, and a refusal without its reason.
    async fn emit(&mut self, event: events::OutboundEvent) -> Result<(), events::EmitError> {
        use events::EmitError;
        let defaults = self.emit_defaults();
        match self.function.clone().emit(event, defaults).await {
            Ok(_) => Ok(()),
            Err(EmitFailure::Invalid(code)) => Err(EmitError::Invalid(code)),
            Err(EmitFailure::Platform(e)) if e.code() == emit_error::UNAVAILABLE => {
                Err(EmitError::Unavailable)
            }
            Err(EmitFailure::Platform(e)) => Err(EmitError::Refused(events::Refusal {
                code: e.code().to_owned(),
                status: e.status(),
            })),
        }
    }

    /// 0.1.1 (Java 571fdff1, owner ruling 11): the event id on success, the
    /// platform's reason on a refusal.
    async fn emit_event(
        &mut self,
        event: events::OutboundEvent,
    ) -> Result<String, events::EmitEventError> {
        use events::EmitEventError;
        let defaults = self.emit_defaults();
        self.function
            .clone()
            .emit(event, defaults)
            .await
            .map_err(|failure| match failure {
                EmitFailure::Invalid(code) => EmitEventError::Invalid(code),
                EmitFailure::Platform(e) if e.code() == emit_error::UNAVAILABLE => {
                    EmitEventError::Unavailable(e.message().to_owned())
                }
                EmitFailure::Platform(e) => EmitEventError::Refused(events::RefusalReason {
                    code: e.code().to_owned(),
                    status: e.status(),
                    message: e.message().to_owned(),
                }),
            })
    }
}

impl invocation::Host for GuestState {
    fn context(&mut self) -> invocation::InvocationContext {
        self.invocation.context.clone()
    }
}

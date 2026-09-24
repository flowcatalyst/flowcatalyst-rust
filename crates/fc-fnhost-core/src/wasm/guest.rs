//! A store's data: WASI (no preopens, no environment, no arguments, no
//! sockets; clocks and random), `wasi:http` (outbound calls refused for now), the
//! memory limits, and the host side of the `flowcatalyst:function`
//! interfaces (`wit/flowcatalyst-function`).

use std::collections::HashMap;
use std::sync::Arc;

use fc_function_abi::{emit_error, Caller, FunctionAddress};
use serde_json::Value;
use tokio::runtime::Handle;
use wasmtime::component::{HasSelf, Linker, ResourceTable};
use wasmtime::StoreLimits;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use super::output::GuestLogger;
use crate::control_plane::{ControlPlane, EmitItem, EmitRequest};

wasmtime::component::bindgen!({
    path: "../../wit/flowcatalyst-function",
    world: "imports",
    imports: {
        "flowcatalyst:function/events.emit": async,
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

/// Where emitted events go: the control plane, spoken for one loaded
/// version. The call runs on the host's own runtime (not the guest
/// runtime), so the control plane's connections live with the reconciler's.
pub struct Emitter {
    pub control_plane: Arc<dyn ControlPlane>,
    pub host_id: String,
    pub host_runtime: Option<Handle>,
}

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
    /// Each memory's `StoreLimits` cap, in bytes: `limits.wasmMemoryMb`, or
    /// the component's own declared maximum when that is smaller.
    pub memory_limit: usize,
    /// The response body's cap, in bytes: `limits.wasmMemoryMb`.
    pub response_cap: usize,
    pub emitter: Emitter,
}

impl FunctionShared {
    async fn emit(
        self: Arc<Self>,
        event: events::OutboundEvent,
        defaults: (String, Option<String>),
    ) -> Result<(), events::EmitError> {
        use events::EmitError;
        if crate::java::is_blank(&event.type_) {
            return Err(EmitError::Invalid(
                emit_error::INVALID_EVENT_TYPE_REQUIRED.to_owned(),
            ));
        }
        if crate::java::is_blank(&event.dedup_id) {
            return Err(EmitError::Invalid(emit_error::DEDUP_ID_REQUIRED.to_owned()));
        }
        let data = match &event.data {
            None => Value::Null,
            Some(text) => serde_json::from_str(text)
                .map_err(|_| EmitError::Invalid(INVALID_EVENT_DATA_NOT_JSON.to_owned()))?,
        };
        let (correlation_id, causation_id) = defaults;
        let request = EmitRequest {
            host_id: self.emitter.host_id.clone(),
            address: self.address.clone(),
            version: self.version,
            events: vec![EmitItem {
                event_type: event.type_,
                subject: event.subject,
                dedup_id: event.dedup_id,
                data,
                correlation_id: event.correlation_id.or(Some(correlation_id)),
                causation_id: event.causation_id.or(causation_id),
                message_group: event.message_group,
            }],
        };
        let control_plane = self.emitter.control_plane.clone();
        let sent = match &self.emitter.host_runtime {
            Some(runtime) => runtime
                .spawn(async move { control_plane.emit(&request).await })
                .await
                .unwrap_or_else(|_| Err(fc_function_abi::EventEmitError::unavailable())),
            None => control_plane.emit(&request).await,
        };
        match sent {
            Ok(()) => Ok(()),
            Err(e) if e.code() == emit_error::UNAVAILABLE => Err(EmitError::Unavailable),
            Err(e) => Err(EmitError::Refused(events::Refusal {
                code: e.code().to_owned(),
                status: e.status(),
            })),
        }
    }
}

/// The host's own code for event data that is not JSON (Java's host reads
/// the whole event as JSON, so its nearest code is `INVALID_EVENT: not JSON`).
pub const INVALID_EVENT_DATA_NOT_JSON: &str = "INVALID_EVENT: data is not JSON";

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

/// Outbound HTTP is refused until the host's egress policy is wired in.
pub struct DenyOutbound;

impl wasmtime_wasi_http::WasiHttpHooks for DenyOutbound {
    fn send_request(
        &mut self,
        _request: http::Request<wasmtime_wasi_http::WasiBody>,
        _options: Option<wasmtime_wasi_http::RequestOptions>,
        _fut: Box<dyn std::future::Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
    ) -> Box<
        dyn std::future::Future<
                Output = wasmtime_wasi_http::Result<(
                    http::Response<wasmtime_wasi_http::WasiBody>,
                    Box<dyn std::future::Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
                )>,
            > + Send,
    > {
        Box::new(async { Err(wasmtime_wasi_http::Error::HttpRequestDenied) })
    }
}

/// One store's data.
pub struct GuestState {
    pub wasi: WasiCtx,
    pub http: WasiHttpCtx,
    pub table: ResourceTable,
    pub limits: StoreLimits,
    pub hooks: DenyOutbound,
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

impl events::Host for GuestState {
    async fn emit(&mut self, event: events::OutboundEvent) -> Result<(), events::EmitError> {
        let defaults = (
            self.invocation.context.correlation_id.clone(),
            self.invocation.context.causation_id.clone(),
        );
        self.function.clone().emit(event, defaults).await
    }
}

impl invocation::Host for GuestState {
    fn context(&mut self) -> invocation::InvocationContext {
        self.invocation.context.clone()
    }
}

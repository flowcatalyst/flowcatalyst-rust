//! The FlowCatalyst function host's reusable core: a drop-in for Java's
//! `fc-fnhost` (`../flowcatalyst-javalin` at `0118cdca`).
//!
//! The management interface the host consumes (desired state, heartbeat,
//! events, artifact download, OAuth token) is Java's, unchanged, so this
//! host runs against the Java platform's `/control/functions/*` as-is, and
//! the listeners answer callers with Java's HTTP contract. A function
//! runtime plugs in through [`loader::FunctionLoader`], and the listeners
//! reach it only through [`invoke::Invoker`]; [`wasm`] is the one this host
//! ships (WASI 0.2 components, where Java runs Extism modules).
//!
//! | Module | Java source |
//! |---|---|
//! | [`env`] | `fnhost/reconcile/HostEnv.java`, `server/EnvReader.java`, `fnhost/route/TrustedProxies.java` |
//! | [`logging`] | `server/Logging.java`, `server/GoJsonEncoder.java` |
//! | [`observability`] | `fnhost/http/FnObservability.java` |
//! | [`metrics`] | `fnhost/metrics/FnMetrics.java` |
//! | [`host`] | `fnhost/FnHost.java`, `fnhost/FnHostMain.java` |
//! | [`token`], [`control_plane`] | `fnhost/reconcile/{TokenSource,HttpControlPlane,ControlPlane}.java` |
//! | [`desired`], [`heartbeat`] | `fnhost/reconcile/{DesiredDocument,HeartbeatReport}.java` |
//! | [`reconciler`], [`reconcile_loop`] | `fnhost/reconcile/{Reconciler,ReconcileLoop}.java` |
//! | [`artifact`] | `platform/function/artifact/*Store*.java`, `fnhost/reconcile/PlatformArtifactStore.java` |
//! | [`signature`], [`digest`] | `platform/function/artifact/{Signatures,SignatureVerifier,TrustRoot}.java`, `platform/function/{Digest,SignerIdentity}.java` — re-exported from `fc-function-signing`, which the platform also depends on |
//! | [`loader`], [`registry`] | `fnhost/load/{FunctionLoader,LoadedFunction,FunctionRegistry}.java` |
//! | [`invoke`] | `LoadedFunction.invoke`, `fnhost/http/InvocationRunner.java` (the runtime-agnostic seam) |
//! | [`listener`] | `fnhost/http/*`, `fnhost/route/PublicRouteTable.java` |
//! | [`manifest`], [`route_pattern`] | `platform/function/{Manifest,EndpointAuth,HttpMethod,RoutePattern}.java` (the stored reader) |
//! | [`tsid`] | `sdk/tsid/Tsid.java` |
//! | [`wasm`] | `fnhost/wasm/*`, `fnhost/load/WasmFunctionLoader.java`, `fnhost/context/*` (redesigned for WASI 0.2 components) |

pub(crate) mod java;

pub mod artifact;
pub mod clock;
pub mod control_plane;
pub mod desired;
pub mod env;
pub mod fingerprint;
pub mod heartbeat;
pub mod host;
pub mod invoke;
pub mod listener;
pub mod loader;
pub mod logging;
pub mod manifest;
pub mod metrics;
pub mod observability;
pub mod reconcile_loop;
pub mod reconciler;
pub mod registry;
pub mod route_pattern;
pub mod token;
pub mod tsid;
pub mod wasm;

// The Sigstore bundle verifier and its `Digest`/`SignerIdentity` value types
// are shared with the platform (checked at publish) and are no longer
// duplicated here; see `fc-function-signing`. Re-exported under their old
// paths so every existing `crate::digest::*` / `crate::signature::*` call
// site in this crate, and every downstream user of
// `fc_fnhost_core::{digest,signature}`, keeps compiling unchanged.
pub use fc_function_signing::{digest, signature};

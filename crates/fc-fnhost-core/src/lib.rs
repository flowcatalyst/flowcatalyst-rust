//! The FlowCatalyst function host's reusable core: a drop-in for Java's
//! `fc-fnhost` (`../flowcatalyst-javalin` at `0118cdca`).
//!
//! The management interface the host consumes (desired state, heartbeat,
//! events, artifact download, OAuth token) is Java's, unchanged, so this
//! host runs against the Java platform's `/control/functions/*` as-is, and
//! the listeners answer callers with Java's HTTP contract. The function
//! runtime is not here: it plugs in through [`loader::FunctionLoader`], and
//! the listeners reach it only through [`invoke::Invoker`].
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
//! | [`signature`] | `platform/function/artifact/{Signatures,SignatureVerifier,TrustRoot}.java` |
//! | [`loader`], [`registry`] | `fnhost/load/{FunctionLoader,LoadedFunction,FunctionRegistry}.java` |
//! | [`invoke`] | `LoadedFunction.invoke`, `fnhost/http/InvocationRunner.java` (the runtime-agnostic seam) |
//! | [`listener`] | `fnhost/http/*`, `fnhost/route/PublicRouteTable.java` |
//! | [`manifest`], [`route_pattern`] | `platform/function/{Manifest,EndpointAuth,HttpMethod,RoutePattern}.java` (the stored reader) |
//! | [`tsid`] | `sdk/tsid/Tsid.java` |

pub(crate) mod java;

pub mod artifact;
pub mod clock;
pub mod control_plane;
pub mod desired;
pub mod digest;
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
pub mod signature;
pub mod token;
pub mod tsid;

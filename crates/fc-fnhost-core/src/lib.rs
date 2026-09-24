//! The FlowCatalyst function host's reusable core: a drop-in for Java's
//! `fc-fnhost` (`../flowcatalyst-javalin` at `0118cdca`). This commit holds
//! the reconciler (H3); the process around it (H2) follows.
//!
//! The management interface the host consumes (desired state, heartbeat,
//! events, artifact download, OAuth token) is Java's, unchanged, so this
//! host runs against the Java platform's `/control/functions/*` as-is. The
//! function runtime is not here: it plugs in through [`loader::FunctionLoader`].
//!
//! | Module | Java source |
//! |---|---|
//! | [`token`], [`control_plane`] | `fnhost/reconcile/{TokenSource,HttpControlPlane,ControlPlane}.java` |
//! | [`desired`], [`heartbeat`] | `fnhost/reconcile/{DesiredDocument,HeartbeatReport}.java` |
//! | [`reconciler`], [`reconcile_loop`] | `fnhost/reconcile/{Reconciler,ReconcileLoop}.java` |
//! | [`artifact`] | `platform/function/artifact/*Store*.java`, `fnhost/reconcile/PlatformArtifactStore.java` |
//! | [`signature`] | `platform/function/artifact/{Signatures,SignatureVerifier,TrustRoot}.java` |
//! | [`loader`], [`registry`] | `fnhost/load/{FunctionLoader,LoadedFunction,FunctionRegistry}.java` |

pub(crate) mod java;

pub mod artifact;
pub mod clock;
pub mod control_plane;
pub mod desired;
pub mod digest;
pub mod fingerprint;
pub mod heartbeat;
pub mod loader;
pub mod reconcile_loop;
pub mod reconciler;
pub mod registry;
pub mod signature;
pub mod token;

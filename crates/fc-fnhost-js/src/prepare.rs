//! Loading one version: the bundle is checked by running it once, in an
//! isolate like a request's (from the process's base snapshot), which also
//! leaves V8's code cache for it. Every request's isolate then compiles the
//! bundle from that cache and runs its top-level code afresh: no state
//! survives from one request to the next, as no state survives a
//! component's instance.
//!
//! Why not a snapshot per version, with the top-level code already run?
//! V8 cannot make a snapshot while other isolates of the process run: the
//! snapshot creator writes the read-only space V8 shares between them (see
//! [`crate::engine::base_snapshot`]). A host serving functions while it
//! loads another always has other isolates running.
//!
//! Refusals (the heartbeat's `LOAD:<code>`):
//! - `JS_INVALID`: not UTF-8, or not a module V8 can compile;
//! - `JS_IMPORT_NOT_ALLOWED`: an import other than the host's modules;
//! - `JS_ENTRYPOINT_NOT_EXPORTED`: the entrypoint export is missing, or is
//!   neither a function nor an object with a `fetch` method;
//! - `JS_INIT_FAILED`: the top-level code threw, used a request-only host
//!   API, ran out of memory or did not finish within the init timeout.

use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::isolate::StartError;
use crate::isolate::{Isolate, Limits};
use crate::modules::{FunctionModules, VersionCode};
use crate::ops::{HostState, VersionShared};
use std::str;
use std::thread;
use tokio::runtime::Builder;
use tokio::time;

pub const JS_INVALID: &str = "JS_INVALID";
pub const JS_IMPORT_NOT_ALLOWED: &str = "JS_IMPORT_NOT_ALLOWED";
pub const JS_ENTRYPOINT_NOT_EXPORTED: &str = "JS_ENTRYPOINT_NOT_EXPORTED";
pub const JS_INIT_FAILED: &str = "JS_INIT_FAILED";

/// The bootstrap script (see `js/bootstrap.js`), in the base snapshot.
pub const BOOTSTRAP: &str = include_str!("../js/bootstrap.js");

/// A load refusal: its code and detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: &'static str,
    pub detail: String,
}

impl Refusal {
    fn new(reason: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}

/// A checked version, ready for requests.
pub struct Prepared {
    pub code: VersionCode,
    /// How long the check took.
    pub took: Duration,
}

/// Checks `bundle` and makes its code cache, in an isolate made as a
/// request's is (from `base` when there is one). Blocking: call it on a thread
/// of its own (it creates and drops an isolate, on a current-thread
/// runtime).
#[expect(
    clippy::let_underscore_must_use,
    reason = "the receiver may already be gone (shutdown, or an abandoned caller): nobody is left to notify; the watchdog thread only waits on a channel and stops the isolate; its panic has no recovery here"
)]
pub fn prepare(
    base: Option<&'static [u8]>,
    bundle: &[u8],
    entrypoint: &str,
    limits: Limits,
    version: Arc<VersionShared>,
    init_timeout: Duration,
) -> Result<Prepared, Refusal> {
    let started = Instant::now();
    let text = str::from_utf8(bundle)
        .map_err(|e| Refusal::new(JS_INVALID, format!("the bundle is not UTF-8: {e}")))?;
    let mut code = VersionCode::new(text.strip_prefix('\u{feff}').unwrap_or(text), entrypoint);
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Refusal::new(JS_INIT_FAILED, format!("no runtime to load on: {e}")))?;
    let modules = FunctionModules::new(code.clone());
    let host = Rc::new(HostState {
        version,
        invocation: None,
    });
    let mut isolate = Isolate::from_base(base, limits, host, modules.clone())
        .map_err(|why| Refusal::new(JS_INIT_FAILED, why))?;
    // The init timeout: top-level code that spins is stopped from here.
    let (done, watch) = mpsc::channel::<()>();
    let terminator = isolate.terminator();
    let watchdog = thread::Builder::new()
        .name("fn-js-init".into())
        .spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = watch.recv_timeout(init_timeout) {
                terminator.stop();
            }
        })
        .map_err(|e| Refusal::new(JS_INIT_FAILED, format!("no init watchdog: {e}")))?;
    let started_module =
        runtime.block_on(async { time::timeout(init_timeout, isolate.start()).await });
    let _ = done.send(());
    let _ = watchdog.join();
    let out_of_memory = isolate.stops().out_of_memory.load(Ordering::Acquire);
    let stopped = isolate.stops().stopped.load(Ordering::Acquire);
    drop(isolate);
    let failed = |why: String| {
        if out_of_memory {
            Refusal::new(
                JS_INIT_FAILED,
                "the top-level code ran past the memory limit (limits.wasmMemoryMb)",
            )
        } else if stopped {
            Refusal::new(
                JS_INIT_FAILED,
                format!(
                    "the top-level code did not finish within {} ms",
                    init_timeout.as_millis()
                ),
            )
        } else {
            Refusal::new(JS_INIT_FAILED, why)
        }
    };
    match started_module {
        Err(_) => {
            return Err(Refusal::new(
                JS_INIT_FAILED,
                format!(
                    "the top-level code did not finish within {} ms",
                    init_timeout.as_millis()
                ),
            ))
        }
        Ok(Err(StartError::Load(why))) => {
            return Err(match modules.refused() {
                Some(refused) => Refusal::new(JS_IMPORT_NOT_ALLOWED, refused),
                None => Refusal::new(JS_INVALID, why),
            })
        }
        Ok(Err(StartError::Evaluate(why))) if why.contains("EntrypointError") => {
            return Err(Refusal::new(JS_ENTRYPOINT_NOT_EXPORTED, why))
        }
        Ok(Err(StartError::Evaluate(why))) => return Err(failed(why)),
        Ok(Ok(_)) => {}
    }
    code.code_cache = modules.made_cache().map(Into::into);
    Ok(Prepared {
        code,
        took: started.elapsed(),
    })
}

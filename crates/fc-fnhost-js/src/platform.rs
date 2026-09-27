//! The V8 platform's foreground tasks, for isolates that live for one
//! request.
//!
//! V8 hands an isolate's foreground tasks (GC finalisation steps, the
//! memory reducer, …) to the embedder's platform, some to run soon and some
//! after a delay. deno_core's own platform schedules a delayed task on
//! tokio and, when the delay is up, pushes it onto the isolate's queue; a
//! task whose isolate has meanwhile been disposed is then destroyed after
//! its isolate, and a V8 task's destructor reaches into the isolate that
//! made it (its cancelable-task manager). With an isolate per request that
//! happens constantly: the host crashed with `pointer being freed was not
//! allocated` in `BackingStore::~BackingStore` during a later isolate's
//! teardown, in about half of the runs of 2,000 requests.
//!
//! This platform keeps every task in a registry keyed by its isolate, and
//! the isolate's owner decides its end:
//! - tasks to run soon run on the isolate's own thread, entered, each time
//!   the isolate is polled ([`run_pending`]); a task posted from a
//!   background thread wakes that poll;
//! - delayed tasks are kept, not run: they are heap heuristics for an
//!   isolate that lives for seconds at most;
//! - before the isolate is disposed, [`close`] destroys whatever is left
//!   while the isolate still exists, and refuses (destroys at once) any task
//!   posted during disposal; [`forget`] drops the entry afterwards.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::{LazyLock, Mutex};
use std::task::Waker;

use deno_core::v8;

#[derive(Default)]
struct Entry {
    tasks: VecDeque<v8::Task>,
    delayed: Vec<v8::Task>,
    waker: Option<Waker>,
    closed: bool,
}

static ENTRIES: LazyLock<Mutex<HashMap<usize, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn entries() -> std::sync::MutexGuard<'static, HashMap<usize, Entry>> {
    // A panic while the lock is held cannot leave an entry half-updated.
    ENTRIES.lock().unwrap_or_else(|e| e.into_inner())
}

/// The registry's key for an isolate.
pub fn key(isolate: v8::UnsafeRawIsolatePtr) -> usize {
    // SAFETY: `UnsafeRawIsolatePtr` is `#[repr(transparent)]` over the
    // C++ isolate pointer, the pointer V8 passes to the platform's hooks.
    unsafe { std::mem::transmute::<v8::UnsafeRawIsolatePtr, usize>(isolate) }
}

const _: () =
    assert!(std::mem::size_of::<v8::UnsafeRawIsolatePtr>() == std::mem::size_of::<usize>());

fn post(isolate: *mut c_void, task: v8::Task, delayed: bool) {
    let mut map = entries();
    let entry = map.entry(isolate as usize).or_default();
    if entry.closed {
        drop(map);
        // The isolate is being disposed: it still exists, so the task can
        // be destroyed now.
        drop(task);
        return;
    }
    if delayed {
        entry.delayed.push(task);
    } else {
        entry.tasks.push_back(task);
        if let Some(waker) = entry.waker.take() {
            waker.wake();
        }
    }
}

/// The platform's hooks.
pub struct FunctionPlatform;

impl v8::PlatformImpl for FunctionPlatform {
    fn post_task(&self, isolate: *mut c_void, task: v8::Task) {
        post(isolate, task, false);
    }

    fn post_non_nestable_task(&self, isolate: *mut c_void, task: v8::Task) {
        post(isolate, task, false);
    }

    fn post_delayed_task(&self, isolate: *mut c_void, task: v8::Task, _delay: f64) {
        post(isolate, task, true);
    }

    fn post_non_nestable_delayed_task(&self, isolate: *mut c_void, task: v8::Task, _delay: f64) {
        post(isolate, task, true);
    }

    fn post_idle_task(&self, _isolate: *mut c_void, task: v8::IdleTask) {
        // Idle tasks are off (`new_custom_platform(_, false, …)`); one that
        // arrives anyway is destroyed where it was posted.
        drop(task);
    }
}

/// The platform V8 is started with: V8's default platform, with at most 4
/// background threads (as deno_core's own), and these hooks.
pub fn new() -> v8::SharedRef<v8::Platform> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .min(4);
    v8::new_custom_platform(threads, false, false, FunctionPlatform).make_shared()
}

/// Runs the isolate's pending foreground tasks, and remembers `waker` so a
/// task posted from another thread wakes the isolate's poll. The caller has
/// the isolate entered, on its own thread.
pub fn run_pending(isolate: usize, waker: &Waker) {
    loop {
        let task = {
            let mut map = entries();
            let entry = map.entry(isolate).or_default();
            if !entry.waker.as_ref().is_some_and(|w| w.will_wake(waker)) {
                entry.waker = Some(waker.clone());
            }
            entry.tasks.pop_front()
        };
        match task {
            // Run outside the lock: a task may post more.
            Some(task) => task.run(),
            None => return,
        }
    }
}

/// The isolate is about to be disposed: destroys its pending tasks while it
/// still exists, and every task posted from now on as it arrives.
pub fn close(isolate: usize) {
    let (tasks, delayed) = {
        let mut map = entries();
        let entry = map.entry(isolate).or_default();
        entry.closed = true;
        entry.waker = None;
        (
            std::mem::take(&mut entry.tasks),
            std::mem::take(&mut entry.delayed),
        )
    };
    drop(tasks);
    drop(delayed);
}

/// The isolate is gone: its entry goes too (a later isolate may get the same
/// address).
pub fn forget(isolate: usize) {
    let entry = entries().remove(&isolate);
    drop(entry);
}

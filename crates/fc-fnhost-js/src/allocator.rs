//! The `ArrayBuffer` allocator every isolate gets: V8's backing stores
//! (`ArrayBuffer`, typed arrays, `Uint8Array` bodies) live outside the V8
//! heap, so the heap limit alone would let a function allocate without
//! bound. Each isolate gets its own counter, capped at the function's
//! memory limit; an allocation past it fails, which V8 reports to the
//! function as a `RangeError` ("Array buffer allocation failed") it may
//! catch, and otherwise the call answers 500.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use deno_core::v8;

/// One isolate's `ArrayBuffer` budget.
#[derive(Debug)]
pub struct Budget {
    cap: usize,
    used: AtomicUsize,
}

impl Budget {
    pub fn new(cap: usize) -> Arc<Self> {
        Arc::new(Self {
            cap,
            used: AtomicUsize::new(0),
        })
    }

    /// Bytes currently allocated.
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    fn reserve(&self, len: usize) -> bool {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(len).filter(|total| *total <= self.cap)
            })
            .is_ok()
    }

    fn release(&self, len: usize) {
        self.used.fetch_sub(len, Ordering::AcqRel);
    }
}

// SAFETY (all four): V8 calls these with the `handle` it was given in
// `new_rust_allocator`, a pointer from `Arc::into_raw`, which stays valid
// until `drop` (below) takes that reference back. `len` is the size V8
// asked for; `free` gets the pointer and length of an earlier allocation.

unsafe extern "C" fn allocate(budget: &Budget, len: usize) -> *mut c_void {
    if !budget.reserve(len) {
        return std::ptr::null_mut();
    }
    // `calloc` of 0 may return null; V8 treats null as a failure, so ask
    // for at least one byte.
    let data = unsafe { libc::calloc(len.max(1), 1) };
    if data.is_null() {
        budget.release(len);
    }
    data
}

unsafe extern "C" fn allocate_uninitialized(budget: &Budget, len: usize) -> *mut c_void {
    if !budget.reserve(len) {
        return std::ptr::null_mut();
    }
    let data = unsafe { libc::malloc(len.max(1)) };
    if data.is_null() {
        budget.release(len);
    }
    data
}

unsafe extern "C" fn free(budget: &Budget, data: *mut c_void, len: usize) {
    budget.release(len);
    unsafe { libc::free(data) };
}

unsafe extern "C" fn drop(budget: *const Budget) {
    // Takes back the reference `capped` gave V8.
    unsafe { Arc::from_raw(budget) };
}

static VTABLE: v8::RustAllocatorVtable<Budget> = v8::RustAllocatorVtable {
    allocate,
    allocate_uninitialized,
    free,
    drop,
};

/// An allocator drawing on `budget`, for `CreateParams::array_buffer_allocator`.
pub fn capped(budget: &Arc<Budget>) -> v8::UniqueRef<v8::Allocator> {
    // SAFETY: the handle is an `Arc` reference V8 owns until it calls
    // `drop`; the vtable is static.
    unsafe { v8::new_rust_allocator(Arc::into_raw(budget.clone()), &VTABLE) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_refuses_past_its_cap_and_frees_back() {
        let budget = Budget::new(100);
        assert!(budget.reserve(60));
        assert!(!budget.reserve(41), "61 + 41 is over 100");
        assert!(budget.reserve(40));
        assert_eq!(budget.used(), 100);
        budget.release(60);
        assert_eq!(budget.used(), 40);
        assert!(budget.reserve(60));
    }
}

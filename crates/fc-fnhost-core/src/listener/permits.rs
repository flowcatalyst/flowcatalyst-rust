//! Host-wide then per-function invocation permits (Java
//! `fnhost/http/Permits.java`), both `try_acquire`: never a queue. A
//! per-function semaphore is sized from the manifest's
//! `limits.maxConcurrency`, created lazily and replaced when a reconcile
//! changes the size; permits checked out of a replaced semaphore go back to
//! it, where nobody looks any more.

use std::collections::HashMap;
use std::sync::Arc;

use fc_function_abi::FunctionAddress;
use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::metrics::PermitsView;

pub struct Permits {
    host: Arc<Semaphore>,
    per_function: Mutex<HashMap<FunctionAddress, (i32, Arc<Semaphore>)>>,
}

/// Both permits of one admitted call; dropping it releases both.
#[derive(Debug)]
pub struct Grant {
    _function: OwnedSemaphorePermit,
    _host: OwnedSemaphorePermit,
}

impl Permits {
    /// `host_max_concurrency` is `FC_FN_MAX_CONCURRENCY`, at least 1.
    pub fn new(host_max_concurrency: usize) -> Self {
        assert!(
            host_max_concurrency >= 1,
            "hostMaxConcurrency must be at least 1"
        );
        Self {
            host: Arc::new(Semaphore::new(host_max_concurrency)),
            per_function: Mutex::new(HashMap::new()),
        }
    }

    /// The host permit, then the function's; `None` when either is
    /// exhausted (the host permit is returned if the function's fails).
    pub fn try_acquire(&self, address: &FunctionAddress, max_concurrency: i32) -> Option<Grant> {
        let host = self.host.clone().try_acquire_owned().ok()?;
        let function = self
            .semaphore_for(address, max_concurrency)
            .try_acquire_owned()
            .ok()?; // `host` drops here: released
        Some(Grant {
            _function: function,
            _host: host,
        })
    }

    pub fn host_available(&self) -> usize {
        self.host.available_permits()
    }

    /// `None` until the address has been sized.
    pub fn function_available(&self, address: &FunctionAddress) -> Option<usize> {
        self.per_function
            .lock()
            .get(address)
            .map(|(_, semaphore)| semaphore.available_permits())
    }

    fn semaphore_for(&self, address: &FunctionAddress, size: i32) -> Arc<Semaphore> {
        let size = size.max(1);
        let mut per_function = self.per_function.lock();
        match per_function.get(address) {
            Some((existing, semaphore)) if *existing == size => semaphore.clone(),
            _ => {
                let semaphore = Arc::new(Semaphore::new(size as usize));
                per_function.insert(address.clone(), (size, semaphore.clone()));
                semaphore
            }
        }
    }
}

impl PermitsView for Permits {
    fn host_available(&self) -> i64 {
        Permits::host_available(self) as i64
    }

    fn known_addresses(&self) -> Vec<FunctionAddress> {
        self.per_function.lock().keys().cloned().collect()
    }

    fn function_available(&self, address: &FunctionAddress) -> Option<i64> {
        Permits::function_available(self, address).map(|n| n as i64)
    }

    fn forget(&self, address: &FunctionAddress) {
        self.per_function.lock().remove(address);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(raw: &str) -> FunctionAddress {
        FunctionAddress::parse(raw).unwrap()
    }

    #[test]
    fn a_refused_function_permit_returns_the_host_permit() {
        let permits = Permits::new(4);
        let first = permits.try_acquire(&a("a.b.c"), 1).unwrap();
        assert!(permits.try_acquire(&a("a.b.c"), 1).is_none());
        assert_eq!(permits.host_available(), 3);
        assert_eq!(permits.function_available(&a("a.b.c")), Some(0));
        drop(first);
        assert_eq!(permits.host_available(), 4);
        assert_eq!(permits.function_available(&a("a.b.c")), Some(1));
    }

    #[test]
    fn the_host_cap_is_shared_across_functions() {
        let permits = Permits::new(1);
        let _held = permits.try_acquire(&a("a.b.one"), 5).unwrap();
        assert!(permits.try_acquire(&a("a.b.two"), 5).is_none());
        assert_eq!(permits.function_available(&a("a.b.two")), None);
    }

    #[test]
    fn a_resize_replaces_the_semaphore() {
        let permits = Permits::new(10);
        let held = permits.try_acquire(&a("a.b.c"), 1).unwrap();
        let resized = permits.try_acquire(&a("a.b.c"), 2).unwrap();
        assert_eq!(permits.function_available(&a("a.b.c")), Some(1));
        drop(held);
        drop(resized);
        assert_eq!(permits.function_available(&a("a.b.c")), Some(2));
        PermitsView::forget(&permits, &a("a.b.c"));
        assert_eq!(permits.function_available(&a("a.b.c")), None);
    }
}

//! One live [`LoadedFunction`] per address (Java
//! `fnhost/load/FunctionRegistry.java`), bounded by `FC_FN_MAX_LOADED`: a
//! warm entry is never evicted; a lazy one is, least recently accessed
//! first, to make room for a new address.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use fc_function_abi::FunctionAddress;
use parking_lot::Mutex;

use crate::clock::SharedClock;
use crate::loader::LoadedFunction;

/// One entry, as of the moment [`FunctionRegistry::snapshot`] was taken.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub address: FunctionAddress,
    pub function: Arc<LoadedFunction>,
    pub warm: bool,
    pub last_accessed: DateTime<Utc>,
}

/// Registering a new address would exceed the cap and every entry is warm.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("function registry is at capacity ({0}) and every loaded entry is warm")]
pub struct RegistryFull(pub usize);

/// What a [`FunctionRegistry::put`] pushed out. The caller closes both.
#[derive(Debug, Default)]
pub struct Displaced {
    /// The previous version of the same address.
    pub previous: Option<Arc<LoadedFunction>>,
    /// A least-recently-used lazy entry evicted to make room. (Java drops
    /// it without closing it; the Rust host hands it back to be closed.)
    pub evicted: Option<Arc<LoadedFunction>>,
}

struct Slot {
    function: Arc<LoadedFunction>,
    warm: bool,
    last_accessed: DateTime<Utc>,
    /// Access order: higher is more recent.
    sequence: u64,
}

struct Inner {
    slots: HashMap<FunctionAddress, Slot>,
    next_sequence: u64,
}

pub struct FunctionRegistry {
    max_loaded: usize,
    clock: SharedClock,
    inner: Mutex<Inner>,
}

impl FunctionRegistry {
    pub fn new(max_loaded: usize, clock: SharedClock) -> Self {
        assert!(max_loaded >= 1, "max_loaded must be at least 1");
        Self {
            max_loaded,
            clock,
            inner: Mutex::new(Inner {
                slots: HashMap::new(),
                next_sequence: 0,
            }),
        }
    }

    /// The live function for `address`, counted as an access (LRU order and
    /// the idle clock). The invoke path uses this; inspection uses [`peek`].
    ///
    /// [`peek`]: FunctionRegistry::peek
    pub fn get(&self, address: &FunctionAddress) -> Option<Arc<LoadedFunction>> {
        let now = self.clock.now();
        let mut inner = self.inner.lock();
        inner.next_sequence += 1;
        let sequence = inner.next_sequence;
        let slot = inner.slots.get_mut(address)?;
        slot.last_accessed = now;
        slot.sequence = sequence;
        Some(slot.function.clone())
    }

    /// [`get`](FunctionRegistry::get) without counting as an access, so a
    /// reconcile cycle never keeps a lazy entry from going idle.
    pub fn peek(&self, address: &FunctionAddress) -> Option<Arc<LoadedFunction>> {
        self.inner
            .lock()
            .slots
            .get(address)
            .map(|slot| slot.function.clone())
    }

    /// Every entry, least recently accessed first.
    pub fn snapshot(&self) -> Vec<Snapshot> {
        let inner = self.inner.lock();
        let mut slots: Vec<(&FunctionAddress, &Slot)> = inner.slots.iter().collect();
        slots.sort_by_key(|(_, slot)| slot.sequence);
        slots
            .into_iter()
            .map(|(address, slot)| Snapshot {
                address: address.clone(),
                function: slot.function.clone(),
                warm: slot.warm,
                last_accessed: slot.last_accessed,
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn warm_count(&self) -> usize {
        self.inner.lock().slots.values().filter(|s| s.warm).count()
    }

    /// Removes and returns `address`'s entry; the caller closes it.
    pub fn remove(&self, address: &FunctionAddress) -> Option<Arc<LoadedFunction>> {
        self.inner
            .lock()
            .slots
            .remove(address)
            .map(|slot| slot.function)
    }

    /// Registers `function` as the live version for its address.
    pub fn put(
        &self,
        function: Arc<LoadedFunction>,
        warm: bool,
    ) -> Result<Displaced, RegistryFull> {
        let now = self.clock.now();
        let mut inner = self.inner.lock();
        let address = function.address().clone();
        let mut evicted = None;
        if !inner.slots.contains_key(&address) && inner.slots.len() >= self.max_loaded {
            let victim = inner
                .slots
                .iter()
                .filter(|(_, slot)| !slot.warm)
                .min_by_key(|(_, slot)| slot.sequence)
                .map(|(address, _)| address.clone())
                .ok_or(RegistryFull(self.max_loaded))?;
            evicted = inner.slots.remove(&victim).map(|slot| slot.function);
        }
        inner.next_sequence += 1;
        let sequence = inner.next_sequence;
        let previous = inner
            .slots
            .insert(
                address,
                Slot {
                    function,
                    warm,
                    last_accessed: now,
                    sequence,
                },
            )
            .map(|slot| slot.function);
        Ok(Displaced { previous, evicted })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;
    use crate::loader::FunctionInstance;
    use async_trait::async_trait;

    struct Nothing;

    #[async_trait]
    impl FunctionInstance for Nothing {
        async fn close(&self) {}
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn function(address: &str, version: i32) -> Arc<LoadedFunction> {
        LoadedFunction::new(
            FunctionAddress::parse(address).unwrap(),
            version,
            Arc::new(Nothing),
        )
    }

    #[test]
    fn evicts_the_least_recently_used_lazy_entry_never_a_warm_one() {
        let registry = FunctionRegistry::new(2, Arc::new(SystemClock));
        registry.put(function("a.a.lazy", 1), false).unwrap();
        registry.put(function("a.a.warm", 1), true).unwrap();
        let out = registry.put(function("a.a.new", 1), false).unwrap();
        assert_eq!(out.evicted.unwrap().address().render(), "a.a.lazy");
        let err = RegistryFull(2);
        registry.put(function("a.a.warm2", 1), true).unwrap(); // evicts a.a.new (lazy)
        assert_eq!(
            registry.put(function("a.a.third", 1), false).unwrap_err(),
            err
        );
    }

    #[test]
    fn get_counts_as_access_peek_does_not() {
        let registry = FunctionRegistry::new(2, Arc::new(SystemClock));
        registry.put(function("a.a.one", 1), false).unwrap();
        registry.put(function("a.a.two", 1), false).unwrap();
        registry.peek(&FunctionAddress::parse("a.a.one").unwrap());
        assert_eq!(registry.snapshot()[0].address.render(), "a.a.one");
        registry.get(&FunctionAddress::parse("a.a.one").unwrap());
        assert_eq!(registry.snapshot()[0].address.render(), "a.a.two");
    }

    #[test]
    fn put_returns_the_displaced_version() {
        let registry = FunctionRegistry::new(4, Arc::new(SystemClock));
        registry.put(function("a.a.one", 1), true).unwrap();
        let out = registry.put(function("a.a.one", 2), true).unwrap();
        assert_eq!(out.previous.unwrap().version(), 1);
        assert_eq!(
            registry
                .peek(&FunctionAddress::parse("a.a.one").unwrap())
                .unwrap()
                .version(),
            2
        );
    }
}

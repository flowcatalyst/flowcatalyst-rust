//! Versions loaded for versioned calls and alias-prefixed public calls
//! (Java `fnhost/http/PinnedVersions.java`): a small LRU (8) beside the
//! registry, never the registry's live slot. An entry is closed when it is
//! evicted, when its version leaves desired state, or when its settings
//! change underneath it (the next call reloads). Versioned calls are the
//! rare, human-driven path, so loads are simply serialised.

use std::sync::Arc;

use fc_function_abi::FunctionAddress;
use parking_lot::Mutex;

use crate::desired::Entry;
use crate::fingerprint::settings_fingerprint;
use crate::loader::LoadedFunction;
use crate::reconciler::Reconciler;

pub const CAPACITY: usize = 8;

type Key = (FunctionAddress, i32);

struct Cached {
    function: Arc<LoadedFunction>,
    fingerprint: String,
}

pub struct PinnedVersions {
    reconciler: Arc<Reconciler>,
    /// Least recently used first.
    cache: Mutex<Vec<(Key, Cached)>>,
    loading: tokio::sync::Mutex<()>,
}

impl PinnedVersions {
    pub fn new(reconciler: Arc<Reconciler>) -> Self {
        Self {
            reconciler,
            cache: Mutex::new(Vec::new()),
            loading: tokio::sync::Mutex::new(()),
        }
    }

    /// The cached version, or a fresh load through
    /// [`Reconciler::load_pinned`]; `None` when it cannot be loaded (not
    /// prepared, or refused).
    pub async fn get_or_load(&self, entry: &Entry) -> Option<Arc<LoadedFunction>> {
        let key: Key = (entry.address.clone(), entry.version);
        if let Some(hit) = self.touch(&key) {
            return Some(hit);
        }
        let _loading = self.loading.lock().await;
        if let Some(hit) = self.touch(&key) {
            return Some(hit);
        }
        let loaded = self.reconciler.load_pinned(entry).await?;
        let evicted = {
            let mut cache = self.cache.lock();
            let evicted = (cache.len() >= CAPACITY).then(|| cache.remove(0).1.function);
            cache.push((
                key,
                Cached {
                    function: loaded.clone(),
                    fingerprint: settings_fingerprint(&entry.config, &entry.secrets),
                },
            ));
            evicted
        };
        if let Some(evicted) = evicted {
            close_later(evicted);
        }
        Some(loaded)
    }

    fn touch(&self, key: &Key) -> Option<Arc<LoadedFunction>> {
        let mut cache = self.cache.lock();
        let position = cache.iter().position(|(k, _)| k == key)?;
        let hit = cache.remove(position);
        let function = hit.1.function.clone();
        cache.push(hit);
        Some(function)
    }

    /// Closes every entry whose version is no longer in the current
    /// document, or whose settings changed. Runs after every reconcile.
    pub fn sweep(&self) {
        let document = self.reconciler.document();
        let stale: Vec<Arc<LoadedFunction>> = {
            let mut cache = self.cache.lock();
            let mut stale = Vec::new();
            cache.retain(|((address, version), cached)| {
                let current = document
                    .as_ref()
                    .and_then(|d| d.entry_for(address, *version));
                let keep = current.is_some_and(|entry| {
                    settings_fingerprint(&entry.config, &entry.secrets) == cached.fingerprint
                });
                if !keep {
                    stale.push(cached.function.clone());
                }
                keep
            });
            stale
        };
        for function in stale {
            close_later(function);
        }
    }

    pub fn len(&self) -> usize {
        self.cache.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Closes everything (listener shutdown).
    pub async fn close(&self) {
        let all: Vec<_> = self
            .cache
            .lock()
            .drain(..)
            .map(|(_, c)| c.function)
            .collect();
        for function in all {
            function.close().await;
        }
    }
}

/// A close waits (bounded) for in-flight calls, so it never runs inline on
/// a reconcile or a request.
fn close_later(function: Arc<LoadedFunction>) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(async move { function.close().await });
        }
        Err(_) => tracing::warn!(
            address = %function.address(),
            version = function.version(),
            "no runtime to close an evicted pinned version on"
        ),
    }
}

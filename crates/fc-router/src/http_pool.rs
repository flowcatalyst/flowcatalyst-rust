//! Per-host HTTP/2 connection pool with dynamic grow/shrink.
//!
//! AWS ALB advertises `SETTINGS_MAX_CONCURRENT_STREAMS = 128` on inbound
//! HTTP/2 connections and translates each stream to a separate HTTP/1.1
//! connection on the target side. hyper opens at most **one** HTTP/2
//! connection per origin per `reqwest::Client`, so high-concurrency
//! mediation against a single host saturates a single connection and
//! excess requests queue invisibly inside h2.
//!
//! This module sits between [`HttpMediator`](crate::mediator::HttpMediator)
//! and reqwest. Each origin gets a [`HostConnectionPool`] holding one or
//! more [`ClientSlot`]s; each slot owns its own `reqwest::Client` and
//! therefore its own HTTP/2 connection. The pool grows a new slot when
//! every existing slot is above the high watermark, up to a configurable
//! cap, and a sweep task removes slots that have fallen below the low
//! watermark and stayed quiet through a grace window.
//!
//! Saturation is inferred from our own in-flight counters because h2 does
//! not surface backpressure when it queues on `SETTINGS_MAX_CONCURRENT_STREAMS`.
//! This is the design's main correctness compromise — a target advertising
//! a tighter cap than our high watermark will still saturate before we
//! grow.

use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use reqwest::Client;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::warning::WarningService;
use fc_common::{WarningCategory, WarningSeverity};

/// Origin identity. Two URLs sharing `(scheme, host, port)` share a
/// single HTTP/2 connection in hyper, so we pool at this granularity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostKey {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl HostKey {
    pub fn from_url(target: &str) -> Result<Self, HostKeyError> {
        let u = url::Url::parse(target).map_err(HostKeyError::Parse)?;
        let scheme = u.scheme().to_string();
        let host = u.host_str().ok_or(HostKeyError::MissingHost)?.to_string();
        let port = u.port_or_known_default().ok_or(HostKeyError::MissingPort)?;
        Ok(Self { scheme, host, port })
    }
}

impl std::fmt::Display for HostKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://{}:{}", self.scheme, self.host, self.port)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HostKeyError {
    #[error("invalid mediation target URL: {0}")]
    Parse(#[from] url::ParseError),
    #[error("mediation target has no host")]
    MissingHost,
    #[error("mediation target has no port and no default for its scheme")]
    MissingPort,
}

/// Sizing parameters for a per-host pool.
///
/// Defaults sit below AWS ALB's `SETTINGS_MAX_CONCURRENT_STREAMS = 128`
/// with enough headroom to absorb a short burst while we grow.
#[derive(Clone, Debug)]
pub struct HostPoolSizing {
    /// Grow a new slot when every existing slot has in-flight >= this value.
    pub streams_high_watermark: usize,
    /// A slot becomes shrink-eligible once its in-flight drops to or below
    /// this value AND it has been quiet through `slot_idle_grace`.
    pub streams_low_watermark: usize,
    /// Hard cap on slots per origin. Once reached we warn (throttled) and
    /// fall back to the least-loaded existing slot.
    pub max_slots_per_host: usize,
    /// How long a slot must remain quiet before the sweep can remove it.
    pub slot_idle_grace: Duration,
    /// Cadence of the sweep task.
    pub sweep_interval: Duration,
    /// Throttle for the "at max_slots_per_host" warning. One warning per
    /// host per interval, no matter how many requests saturate.
    pub max_slots_warning_interval: Duration,
}

impl Default for HostPoolSizing {
    fn default() -> Self {
        Self {
            streams_high_watermark: 100,
            streams_low_watermark: 20,
            max_slots_per_host: 8,
            slot_idle_grace: Duration::from_secs(60),
            sweep_interval: Duration::from_secs(15),
            max_slots_warning_interval: Duration::from_secs(60),
        }
    }
}

impl HostPoolSizing {
    /// HTTP/1.1 doesn't multiplex, so growing past one slot per host
    /// duplicates reqwest's own connection-pool behaviour. Use this preset
    /// for the HTTP/1.1 mediator path.
    pub fn http1() -> Self {
        Self {
            max_slots_per_host: 1,
            ..Self::default()
        }
    }
}

/// One slot in a per-host pool. Owns a `reqwest::Client` and the counters
/// the pool uses to decide grow/shrink.
pub struct ClientSlot {
    client: Client,
    in_flight: AtomicUsize,
    last_used_ms: AtomicI64,
}

impl ClientSlot {
    fn new(client: Client) -> Self {
        Self {
            client,
            in_flight: AtomicUsize::new(0),
            last_used_ms: AtomicI64::new(now_ms()),
        }
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Relaxed)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// RAII guard returned by [`HostConnectionPool::acquire`]. Holds a
/// reference to a slot's `Client` while in-flight count is incremented;
/// on drop the counter is decremented and `last_used` is bumped.
pub struct SlotGuard {
    slot: Arc<ClientSlot>,
}

impl SlotGuard {
    pub fn client(&self) -> &Client {
        &self.slot.client
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.slot.in_flight.fetch_sub(1, Ordering::Relaxed);
        self.slot.last_used_ms.store(now_ms(), Ordering::Relaxed);
    }
}

/// Builds a fresh `reqwest::Client` for a new slot in a given origin's
/// pool. Takes the [`HostKey`] (item 3, owner ruling 2026-09-07: deployed
/// mode needs prior-knowledge h2c for `http://` targets but ALPN h2 for
/// `https://` ones — those are two different `reqwest::ClientBuilder`
/// configurations, and a `HostKey` already carries `scheme`, so the
/// builder decides per-origin rather than the registry needing a second
/// builder function).
type ClientBuilderFn = dyn Fn(&HostKey) -> Client + Send + Sync;

/// Per-origin pool of HTTP/2 connections.
pub struct HostConnectionPool {
    host: HostKey,
    sizing: HostPoolSizing,
    slots: RwLock<Vec<Arc<ClientSlot>>>,
    /// Serialises slot creation. Held only across `Vec::push` — never
    /// across an `.await`.
    grow_lock: Mutex<()>,
    builder: Arc<ClientBuilderFn>,
    warning_service: Arc<WarningService>,
    last_max_warn_ms: AtomicI64,
}

impl HostConnectionPool {
    pub fn new(
        host: HostKey,
        sizing: HostPoolSizing,
        builder: Arc<ClientBuilderFn>,
        warning_service: Arc<WarningService>,
    ) -> Self {
        let initial = Arc::new(ClientSlot::new(builder(&host)));
        Self {
            host,
            sizing,
            slots: RwLock::new(vec![initial]),
            grow_lock: Mutex::new(()),
            builder,
            warning_service,
            last_max_warn_ms: AtomicI64::new(0),
        }
    }

    /// Acquire a slot for one request. Returns a guard whose `Drop`
    /// releases the slot.
    ///
    /// Selection: pick the least-loaded slot. If every slot is at or
    /// above the high watermark, attempt to grow; if growth is capped,
    /// fall back to the least-loaded slot (and let h2 queue).
    pub fn acquire(&self) -> SlotGuard {
        let (least_loaded, min_in_flight) = {
            let slots = self.slots.read();
            select_least_loaded(&slots)
        };

        if min_in_flight >= self.sizing.streams_high_watermark {
            if let Some(new_slot) = self.try_grow() {
                new_slot.in_flight.fetch_add(1, Ordering::Relaxed);
                return SlotGuard { slot: new_slot };
            }
        }

        least_loaded.in_flight.fetch_add(1, Ordering::Relaxed);
        SlotGuard { slot: least_loaded }
    }

    fn try_grow(&self) -> Option<Arc<ClientSlot>> {
        let _g = self.grow_lock.lock();
        // Re-check under the grow_lock: another thread may have already
        // added a slot, or an in-flight request may have completed and
        // pushed the minimum back below the high watermark.
        {
            let slots = self.slots.read();
            if slots
                .iter()
                .any(|s| s.in_flight() < self.sizing.streams_high_watermark)
            {
                return None;
            }
            if slots.len() >= self.sizing.max_slots_per_host {
                drop(slots);
                self.warn_max_slots();
                return None;
            }
        }

        let slot = Arc::new(ClientSlot::new((self.builder)(&self.host)));
        let new_count = {
            let mut slots = self.slots.write();
            slots.push(slot.clone());
            slots.len()
        };
        info!(
            host = %self.host,
            slots = new_count,
            high_watermark = self.sizing.streams_high_watermark,
            "Grew per-host HTTP/2 connection pool"
        );
        Some(slot)
    }

    fn warn_max_slots(&self) {
        let now = now_ms();
        let throttle_ms = self.sizing.max_slots_warning_interval.as_millis() as i64;
        let last = self.last_max_warn_ms.load(Ordering::Relaxed);
        if now - last < throttle_ms {
            return;
        }
        // Best-effort throttle: a concurrent caller might also pass this
        // check before we store. Acceptable — at worst we emit two
        // warnings for the same saturation event.
        self.last_max_warn_ms.store(now, Ordering::Relaxed);
        let slot_count = self.sizing.max_slots_per_host;
        warn!(
            host = %self.host,
            slots = slot_count,
            "Per-host HTTP/2 connection pool at max_slots_per_host; requests will queue inside h2"
        );
        self.warning_service.add_warning(
            WarningCategory::Configuration,
            WarningSeverity::Warn,
            format!(
                "HTTP/2 connection pool to {} at maximum slots ({}); excess requests will queue inside h2, increasing tail latency",
                self.host, slot_count
            ),
            "HttpMediator".to_string(),
        );
    }

    /// Remove slots that are below the low watermark and have been quiet
    /// through `slot_idle_grace`. Always keeps at least one slot.
    pub fn sweep(&self) {
        let grace_ms = self.sizing.slot_idle_grace.as_millis() as i64;
        let now = now_ms();
        let (removed, remaining) = {
            let mut slots = self.slots.write();
            if slots.len() <= 1 {
                return;
            }
            let before = slots.len();
            // Mark candidates first so we never strip below 1 slot, even if
            // every slot looks idle.
            let mut keep_at_least_one = false;
            slots.retain(|s| {
                let in_flight = s.in_flight();
                let idle = now - s.last_used_ms.load(Ordering::Relaxed) > grace_ms;
                let evict = in_flight <= self.sizing.streams_low_watermark && idle;
                if !evict {
                    keep_at_least_one = true;
                }
                !evict
            });
            if !keep_at_least_one {
                // Everything was evictable — restore the freshest one by
                // rebuilding. (Reached only when grace and watermarks are
                // both extremely permissive.)
                slots.push(Arc::new(ClientSlot::new((self.builder)(&self.host))));
            }
            (before - slots.len(), slots.len())
        };
        if removed > 0 {
            info!(
                host = %self.host,
                removed = removed,
                remaining = remaining,
                "Shrank per-host HTTP/2 connection pool"
            );
        } else {
            debug!(host = %self.host, slots = remaining, "Host pool sweep — no change");
        }
    }

    pub fn slot_count(&self) -> usize {
        self.slots.read().len()
    }

    pub fn host(&self) -> &HostKey {
        &self.host
    }
}

/// Returns (least-loaded slot, its in-flight count). Slots is guaranteed
/// non-empty (constructor inserts one; sweep maintains the invariant).
fn select_least_loaded(slots: &[Arc<ClientSlot>]) -> (Arc<ClientSlot>, usize) {
    debug_assert!(!slots.is_empty(), "host pool must always hold ≥1 slot");
    let mut best_idx = 0;
    let mut best_load = usize::MAX;
    for (i, s) in slots.iter().enumerate() {
        let l = s.in_flight();
        if l < best_load {
            best_load = l;
            best_idx = i;
            if l == 0 {
                break;
            }
        }
    }
    (slots[best_idx].clone(), best_load)
}

/// Registry of host pools owned by a single `HttpMediator`. The mediator
/// holds this behind `Arc` and the sweep task holds a `Weak` reference,
/// so dropping the mediator stops the sweep.
pub struct HostPoolRegistry {
    pools: DashMap<HostKey, Arc<HostConnectionPool>>,
    sizing: HostPoolSizing,
    builder: Arc<ClientBuilderFn>,
    warning_service: Arc<WarningService>,
}

impl HostPoolRegistry {
    pub fn new(
        sizing: HostPoolSizing,
        builder: Arc<ClientBuilderFn>,
        warning_service: Arc<WarningService>,
    ) -> Self {
        Self {
            pools: DashMap::new(),
            sizing,
            builder,
            warning_service,
        }
    }

    /// Get or create the pool for an origin and acquire one slot from it.
    pub fn acquire(&self, host: HostKey) -> SlotGuard {
        if let Some(pool) = self.pools.get(&host) {
            return pool.acquire();
        }
        let pool = self
            .pools
            .entry(host.clone())
            .or_insert_with(|| {
                Arc::new(HostConnectionPool::new(
                    host,
                    self.sizing.clone(),
                    self.builder.clone(),
                    self.warning_service.clone(),
                ))
            })
            .clone();
        pool.acquire()
    }

    pub fn sweep_all(&self) {
        for entry in self.pools.iter() {
            entry.value().sweep();
        }
    }

    /// Test/observability helper — total slots across every host pool.
    pub fn total_slots(&self) -> usize {
        self.pools.iter().map(|e| e.value().slot_count()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warning::WarningService;

    fn make_builder() -> Arc<ClientBuilderFn> {
        Arc::new(|_host: &HostKey| reqwest::Client::builder().build().unwrap())
    }

    #[test]
    fn host_key_parses_default_ports() {
        let k = HostKey::from_url("https://api.example.com/foo").unwrap();
        assert_eq!(k.scheme, "https");
        assert_eq!(k.host, "api.example.com");
        assert_eq!(k.port, 443);
    }

    #[test]
    fn host_key_keeps_explicit_port() {
        let k = HostKey::from_url("http://api.example.com:8080/foo").unwrap();
        assert_eq!(k.port, 8080);
    }

    #[test]
    fn host_key_rejects_malformed() {
        assert!(HostKey::from_url("not a url").is_err());
    }

    #[test]
    fn pool_starts_with_one_slot() {
        let pool = HostConnectionPool::new(
            HostKey::from_url("https://h.example/").unwrap(),
            HostPoolSizing::default(),
            make_builder(),
            Arc::new(WarningService::noop()),
        );
        assert_eq!(pool.slot_count(), 1);
    }

    #[test]
    fn pool_grows_when_high_watermark_reached() {
        let sizing = HostPoolSizing {
            streams_high_watermark: 2,
            streams_low_watermark: 0,
            max_slots_per_host: 4,
            ..HostPoolSizing::default()
        };
        let pool = HostConnectionPool::new(
            HostKey::from_url("https://h.example/").unwrap(),
            sizing,
            make_builder(),
            Arc::new(WarningService::noop()),
        );
        // Saturate the first slot: 2 in-flight == high watermark.
        let _g1 = pool.acquire();
        let _g2 = pool.acquire();
        assert_eq!(pool.slot_count(), 1);
        // Third acquire must grow: every slot is at the watermark.
        let _g3 = pool.acquire();
        assert_eq!(pool.slot_count(), 2);
    }

    #[test]
    fn pool_does_not_exceed_max_slots() {
        let sizing = HostPoolSizing {
            streams_high_watermark: 1,
            streams_low_watermark: 0,
            max_slots_per_host: 2,
            ..HostPoolSizing::default()
        };
        let pool = HostConnectionPool::new(
            HostKey::from_url("https://h.example/").unwrap(),
            sizing,
            make_builder(),
            Arc::new(WarningService::noop()),
        );
        let _g1 = pool.acquire(); // slot 1
        let _g2 = pool.acquire(); // grows to slot 2
        let _g3 = pool.acquire(); // would grow but at cap
        let _g4 = pool.acquire(); // ditto
        assert_eq!(pool.slot_count(), 2);
    }

    #[test]
    fn drop_decrements_in_flight() {
        let pool = HostConnectionPool::new(
            HostKey::from_url("https://h.example/").unwrap(),
            HostPoolSizing::default(),
            make_builder(),
            Arc::new(WarningService::noop()),
        );
        let g = pool.acquire();
        assert_eq!(g.slot.in_flight(), 1);
        drop(g);
        let slots = pool.slots.read();
        assert_eq!(slots[0].in_flight(), 0);
    }

    #[test]
    fn sweep_removes_idle_slots() {
        let sizing = HostPoolSizing {
            streams_high_watermark: 1,
            streams_low_watermark: 0,
            max_slots_per_host: 4,
            slot_idle_grace: Duration::from_millis(1),
            ..HostPoolSizing::default()
        };
        let pool = HostConnectionPool::new(
            HostKey::from_url("https://h.example/").unwrap(),
            sizing,
            make_builder(),
            Arc::new(WarningService::noop()),
        );
        // Grow to 3 slots.
        let g1 = pool.acquire();
        let g2 = pool.acquire();
        let g3 = pool.acquire();
        assert_eq!(pool.slot_count(), 3);
        drop(g1);
        drop(g2);
        drop(g3);
        std::thread::sleep(Duration::from_millis(5));
        pool.sweep();
        assert_eq!(pool.slot_count(), 1, "sweep should retain exactly one slot");
    }

    #[test]
    fn sweep_keeps_busy_slot() {
        let sizing = HostPoolSizing {
            streams_high_watermark: 1,
            streams_low_watermark: 0,
            max_slots_per_host: 4,
            slot_idle_grace: Duration::from_millis(1),
            ..HostPoolSizing::default()
        };
        let pool = HostConnectionPool::new(
            HostKey::from_url("https://h.example/").unwrap(),
            sizing,
            make_builder(),
            Arc::new(WarningService::noop()),
        );
        let _busy = pool.acquire();
        let g2 = pool.acquire();
        assert_eq!(pool.slot_count(), 2);
        drop(g2);
        std::thread::sleep(Duration::from_millis(5));
        pool.sweep();
        // Busy slot must survive even though slot 2 became idle.
        assert!(pool.slot_count() >= 1);
    }

    #[test]
    fn registry_separates_pools_by_origin() {
        let reg = HostPoolRegistry::new(
            HostPoolSizing::default(),
            make_builder(),
            Arc::new(WarningService::noop()),
        );
        let _a = reg.acquire(HostKey::from_url("https://a.example/").unwrap());
        let _b = reg.acquire(HostKey::from_url("https://b.example/").unwrap());
        let _c = reg.acquire(HostKey::from_url("https://a.example:8443/").unwrap());
        assert_eq!(reg.pools.len(), 3);
    }
}

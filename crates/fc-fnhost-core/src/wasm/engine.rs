//! The one wasmtime [`Engine`] every WASM function shares: the pooling
//! allocator, epoch interruption driven by a ticker thread, and the
//! fingerprint that keys the `.cwasm` cache.

use std::hash::Hasher;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, OptLevel, PoolingAllocationConfig};

/// The wasmtime release this host is built against (the exact pin in
/// `Cargo.toml`). Part of the `.cwasm` fingerprint.
pub const WASMTIME_VERSION: &str = "49.0.1";

/// The largest linear memory any guest can have: wasm32's 4 GiB. Each
/// function's own cap (`limits.wasmMemoryMb`) is enforced per store by
/// `StoreLimits`; this only sizes the pool's memory slots, which are
/// virtual address space (reserved, not committed).
pub const MAX_MEMORY_BYTES: usize = 4 << 30;

/// How often the epoch advances: the granularity of deadlines and of the
/// time slice a running guest gets before it yields.
pub const EPOCH_TICK: Duration = Duration::from_millis(1);

/// Core instances, tables and memories one component may use. A Rust
/// `wasm32-wasip2` component has one memory and a handful of core
/// instances (its module plus the canonical-ABI shims).
const CORE_INSTANCES_PER_COMPONENT: u32 = 16;
const MEMORIES_PER_COMPONENT: u32 = 2;
const TABLES_PER_COMPONENT: u32 = 16;

/// What shapes the engine. `Default` is production's shape apart from the
/// pool size.
#[derive(Debug, Clone)]
pub struct EngineSettings {
    /// Component instances alive at once, host-wide (the pool's slots).
    /// Instance-per-request makes this the number of invocations in flight,
    /// so the host sizes it from `FC_FN_MAX_CONCURRENCY`.
    pub max_instances: u32,
    /// Cranelift's optimisation level. Changing it changes the
    /// fingerprint: `.cwasm` files compiled at another level are not reused.
    pub opt_level: OptLevel,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            max_instances: 64,
            opt_level: OptLevel::Speed,
        }
    }
}

/// The engine configuration: pooling allocator, epoch interruption,
/// component model. Async is always on in wasmtime 49.
pub fn config(settings: &EngineSettings) -> Config {
    let n = settings.max_instances.max(1);
    let mut pool = PoolingAllocationConfig::default();
    pool.total_component_instances(n)
        .total_core_instances(n.saturating_mul(CORE_INSTANCES_PER_COMPONENT))
        .total_memories(n.saturating_mul(MEMORIES_PER_COMPONENT))
        .total_tables(n.saturating_mul(TABLES_PER_COMPONENT))
        .total_stacks(n)
        .max_memory_size(MAX_MEMORY_BYTES)
        .max_core_instances_per_component(CORE_INSTANCES_PER_COMPONENT)
        .max_memories_per_component(MEMORIES_PER_COMPONENT)
        .max_tables_per_component(TABLES_PER_COMPONENT);
    let mut config = Config::new();
    config
        .allocation_strategy(InstanceAllocationStrategy::Pooling(pool))
        .epoch_interruption(true)
        .wasm_component_model(true)
        .cranelift_opt_level(settings.opt_level);
    config
}

pub fn engine(settings: &EngineSettings) -> wasmtime::Result<Engine> {
    Engine::new(&config(settings))
}

/// What a `.cwasm` is only valid for: this wasmtime release plus every
/// engine setting that affects compiled code (target, Cranelift flags,
/// tunables, wasm features), as wasmtime's own
/// [`Engine::precompile_compatibility_hash`] defines it. Hex, 32 chars.
///
/// Two engines with the same fingerprint can load each other's `.cwasm`;
/// any change (an upgrade, a new opt level) gives a new fingerprint, so a
/// stale `.cwasm` is never even looked at.
pub fn fingerprint(engine: &Engine) -> String {
    let mut hasher = Sha256Hasher(Sha256::new());
    hasher.0.update(b"wasmtime ");
    hasher.0.update(WASMTIME_VERSION.as_bytes());
    hasher.0.update(b"\0");
    std::hash::Hash::hash(&engine.precompile_compatibility_hash(), &mut hasher);
    hex::encode(&hasher.0.finalize()[..16])
}

/// Feeds a `Hash` impl's bytes into sha256 (`finish` is unused).
struct Sha256Hasher(Sha256);

impl Hasher for Sha256Hasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

/// Advances the engine's epoch every [`EPOCH_TICK`] until dropped.
pub struct EpochTicker {
    stop: Arc<AtomicBool>,
}

impl EpochTicker {
    pub fn start(engine: &Engine) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let weak = engine.weak();
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("fn-epoch".into())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(EPOCH_TICK);
                    match weak.upgrade() {
                        Some(engine) => engine.increment_epoch(),
                        None => return,
                    }
                }
            })?;
        Ok(Self { stop })
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_is_stable_for_one_config_and_changes_with_it() {
        let speed = EngineSettings::default();
        let a = fingerprint(&engine(&speed).unwrap());
        let b = fingerprint(&engine(&speed).unwrap());
        assert_eq!(a, b, "two engines with one config share their .cwasm");
        assert_eq!(a.len(), 32);
        let size = EngineSettings {
            opt_level: OptLevel::SpeedAndSize,
            ..speed.clone()
        };
        assert_ne!(
            a,
            fingerprint(&engine(&size).unwrap()),
            "a codegen setting is part of the fingerprint"
        );
        let bigger_pool = EngineSettings {
            max_instances: 128,
            ..speed
        };
        assert_eq!(
            a,
            fingerprint(&engine(&bigger_pool).unwrap()),
            "the pool size is a runtime setting, not a codegen one"
        );
    }
}

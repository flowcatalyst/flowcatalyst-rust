//! Java `function/FunctionLimits.java` and `function/ClientCeilings.java`,
//! with the `FC_FN_*` defaults Java reads in `server/Env.java:600-605`.

/// A limit component that is zero or negative. Names the component, as
/// Java's `IllegalArgumentException` does.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{name} must be > 0, got {value}")]
pub struct NonPositiveLimit {
    pub name: &'static str,
    pub value: i32,
}

fn require_positive(value: i32, name: &'static str) -> Result<i32, NonPositiveLimit> {
    if value <= 0 {
        Err(NonPositiveLimit { name, value })
    } else {
        Ok(value)
    }
}

/// The platform's default function limits. A manifest without an explicit
/// limit gets these, clamped to the client's ceiling ([`ClientCeilings`]).
/// `max_warm_per_host` is carried for the host's warm-capacity check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionLimits {
    max_duration_ms: i32,
    max_concurrency: i32,
    wasm_memory_mb: i32,
    db_pool_size: i32,
    max_warm_per_host: i32,
}

impl FunctionLimits {
    pub const DEFAULT_MAX_DURATION_MS: i32 = 30_000;
    pub const DEFAULT_MAX_CONCURRENCY: i32 = 32;
    pub const DEFAULT_WASM_MEMORY_MB: i32 = 64;
    pub const DEFAULT_DB_POOL_SIZE: i32 = 4;
    pub const DEFAULT_MAX_WARM_PER_HOST: i32 = 200;

    /// Every component must be positive.
    pub fn new(
        max_duration_ms: i32,
        max_concurrency: i32,
        wasm_memory_mb: i32,
        db_pool_size: i32,
        max_warm_per_host: i32,
    ) -> Result<FunctionLimits, NonPositiveLimit> {
        Ok(FunctionLimits {
            max_duration_ms: require_positive(max_duration_ms, "maxDurationMs")?,
            max_concurrency: require_positive(max_concurrency, "maxConcurrency")?,
            wasm_memory_mb: require_positive(wasm_memory_mb, "wasmMemoryMb")?,
            db_pool_size: require_positive(db_pool_size, "dbPoolSize")?,
            max_warm_per_host: require_positive(max_warm_per_host, "maxWarmPerHost")?,
        })
    }

    /// The platform defaults: 30000 ms, 32, 64 MB, 4, 200.
    pub fn defaults() -> FunctionLimits {
        FunctionLimits {
            max_duration_ms: Self::DEFAULT_MAX_DURATION_MS,
            max_concurrency: Self::DEFAULT_MAX_CONCURRENCY,
            wasm_memory_mb: Self::DEFAULT_WASM_MEMORY_MB,
            db_pool_size: Self::DEFAULT_DB_POOL_SIZE,
            max_warm_per_host: Self::DEFAULT_MAX_WARM_PER_HOST,
        }
    }

    /// From the process environment: `FC_FN_DEFAULT_MAX_DURATION_MS`,
    /// `FC_FN_DEFAULT_MAX_CONCURRENCY`, `FC_FN_DEFAULT_WASM_MEMORY_MB`,
    /// `FC_FN_DEFAULT_DB_POOL_SIZE` and `FC_FN_MAX_WARM_PER_HOST`.
    pub fn from_env() -> Result<FunctionLimits, NonPositiveLimit> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// As Java's `Env`: an unset or unparseable value (Java's
    /// `Integer.parseInt`: optional sign, decimal digits, nothing else) takes
    /// the default; a parseable one that is zero or negative is an error, so
    /// startup fails rather than arming a pool with a non-positive limit.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<FunctionLimits, NonPositiveLimit> {
        let int = |name: &str, default: i32| {
            lookup(name)
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(default)
        };
        Self::new(
            int(
                "FC_FN_DEFAULT_MAX_DURATION_MS",
                Self::DEFAULT_MAX_DURATION_MS,
            ),
            int(
                "FC_FN_DEFAULT_MAX_CONCURRENCY",
                Self::DEFAULT_MAX_CONCURRENCY,
            ),
            int("FC_FN_DEFAULT_WASM_MEMORY_MB", Self::DEFAULT_WASM_MEMORY_MB),
            int("FC_FN_DEFAULT_DB_POOL_SIZE", Self::DEFAULT_DB_POOL_SIZE),
            int("FC_FN_MAX_WARM_PER_HOST", Self::DEFAULT_MAX_WARM_PER_HOST),
        )
    }

    pub fn max_duration_ms(&self) -> i32 {
        self.max_duration_ms
    }

    pub fn max_concurrency(&self) -> i32 {
        self.max_concurrency
    }

    pub fn wasm_memory_mb(&self) -> i32 {
        self.wasm_memory_mb
    }

    pub fn db_pool_size(&self) -> i32 {
        self.db_pool_size
    }

    pub fn max_warm_per_host(&self) -> i32 {
        self.max_warm_per_host
    }
}

/// The per-client ceilings a manifest's limits may not exceed. A client with
/// no policy row gets [`ClientCeilings::of`], every ceiling equal to the
/// platform default, so a function may lower a limit but never raise one.
/// An absent limit resolves to `min(default, ceiling)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientCeilings {
    max_duration_ms: i32,
    max_concurrency: i32,
    wasm_memory_mb: i32,
    db_pool_size: i32,
}

impl ClientCeilings {
    /// Every component must be positive.
    pub fn new(
        max_duration_ms: i32,
        max_concurrency: i32,
        wasm_memory_mb: i32,
        db_pool_size: i32,
    ) -> Result<ClientCeilings, NonPositiveLimit> {
        Ok(ClientCeilings {
            max_duration_ms: require_positive(max_duration_ms, "maxDurationMs")?,
            max_concurrency: require_positive(max_concurrency, "maxConcurrency")?,
            wasm_memory_mb: require_positive(wasm_memory_mb, "wasmMemoryMb")?,
            db_pool_size: require_positive(db_pool_size, "dbPoolSize")?,
        })
    }

    /// A client with no policy row: every ceiling equals the default.
    pub fn of(defaults: &FunctionLimits) -> ClientCeilings {
        ClientCeilings {
            max_duration_ms: defaults.max_duration_ms,
            max_concurrency: defaults.max_concurrency,
            wasm_memory_mb: defaults.wasm_memory_mb,
            db_pool_size: defaults.db_pool_size,
        }
    }

    /// Ceiling for `limits.maxDurationMs` and an endpoint's `timeoutMs`.
    pub fn max_duration_ms(&self) -> i32 {
        self.max_duration_ms
    }

    pub fn max_concurrency(&self) -> i32 {
        self.max_concurrency
    }

    pub fn wasm_memory_mb(&self) -> i32 {
        self.wasm_memory_mb
    }

    /// Ceiling for `db[].poolSize`.
    pub fn db_pool_size(&self) -> i32 {
        self.db_pool_size
    }
}

/// Java `FunctionLimitsTest` and `ClientCeilingsTest`.
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn defaults_match_the_spec_table() {
        let d = FunctionLimits::defaults();
        assert_eq!(d.max_duration_ms(), 30_000);
        assert_eq!(d.max_concurrency(), 32);
        assert_eq!(d.wasm_memory_mb(), 64);
        assert_eq!(d.db_pool_size(), 4);
        assert_eq!(d.max_warm_per_host(), 200);
    }

    #[test]
    fn every_component_rejects_non_positive() {
        for name in [
            "maxDurationMs",
            "maxConcurrency",
            "wasmMemoryMb",
            "dbPoolSize",
            "maxWarmPerHost",
        ] {
            for value in [0, -1] {
                let v = |n: &str, d: i32| if n == name { value } else { d };
                let err = FunctionLimits::new(
                    v("maxDurationMs", 30_000),
                    v("maxConcurrency", 32),
                    v("wasmMemoryMb", 64),
                    v("dbPoolSize", 4),
                    v("maxWarmPerHost", 200),
                )
                .unwrap_err();
                assert_eq!(err.name, name);
                assert!(err.to_string().contains(name));
            }
        }
    }

    #[test]
    fn ceilings_of_defaults_and_rejection() {
        let c = ClientCeilings::of(&FunctionLimits::defaults());
        assert_eq!(
            (
                c.max_duration_ms(),
                c.max_concurrency(),
                c.wasm_memory_mb(),
                c.db_pool_size()
            ),
            (30_000, 32, 64, 4)
        );
        assert_eq!(
            ClientCeilings::new(1, 0, 1, 1).unwrap_err().name,
            "maxConcurrency"
        );
    }

    #[test]
    fn env_reading_follows_java() {
        let env: HashMap<&str, &str> = HashMap::from([
            ("FC_FN_DEFAULT_MAX_DURATION_MS", "+5000"),
            ("FC_FN_DEFAULT_MAX_CONCURRENCY", " 8"),
            ("FC_FN_DEFAULT_WASM_MEMORY_MB", "nope"),
            ("FC_FN_DEFAULT_DB_POOL_SIZE", "2"),
        ]);
        let limits = FunctionLimits::from_lookup(|k| env.get(k).map(|v| v.to_string())).unwrap();
        assert_eq!(limits.max_duration_ms(), 5000);
        assert_eq!(limits.max_concurrency(), 32, "unparseable falls back");
        assert_eq!(limits.wasm_memory_mb(), 64);
        assert_eq!(limits.db_pool_size(), 2);
        assert_eq!(limits.max_warm_per_host(), 200);

        let err = FunctionLimits::from_lookup(|k| {
            (k == "FC_FN_MAX_WARM_PER_HOST").then(|| "0".to_string())
        })
        .unwrap_err();
        assert_eq!(err.name, "maxWarmPerHost");
    }
}

//! The host's own series (Java `fnhost/metrics/FnMetrics.java`, spec
//! `function-host-process.md` §2): the same `fc_fn_*` names and labels. An
//! address's series are removed when it leaves desired state, so a host
//! that has served thousands of short-lived functions does not export
//! thousands of dead series.
//!
//! Exposition is OpenMetrics text (`prometheus-client`), which is also what
//! Java serves a Prometheus scraper (it negotiates OpenMetrics by `Accept`);
//! the series names a scraper stores are identical.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use fc_function_abi::FunctionAddress;
use parking_lot::{Mutex, RwLock};
use prometheus_client::collector::Collector;
use prometheus_client::encoding::{DescriptorEncoder, EncodeMetric};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::{ConstGauge, Gauge};
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

use crate::exec::ExecBudget;
use crate::invoke::Consumption;
use crate::reconciler::ReconcileObserver;
use crate::registry::FunctionRegistry;
use prometheus_client::encoding::text;
use prometheus_client::metrics::MetricType;
use std::fmt;
use std::fmt::Formatter;
use std::mem;

/// 5 ms … 60 s.
const DURATION_BUCKETS: [f64; 13] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
];

/// Fuel per invocation: 10k (a trivial handler is tens of thousands) to
/// 10G (seconds of pure compute).
const FUEL_BUCKETS: [f64; 11] = [1e4, 3e4, 1e5, 3e5, 1e6, 3e6, 1e7, 3e7, 1e8, 1e9, 1e10];

/// Peak linear memory per invocation: 256 KiB … 4 GiB (wasm32's ceiling).
const MEMORY_BUCKETS: [f64; 13] = [
    262_144.0,
    1_048_576.0,
    2_097_152.0,
    4_194_304.0,
    8_388_608.0,
    16_777_216.0,
    33_554_432.0,
    67_108_864.0,
    134_217_728.0,
    268_435_456.0,
    536_870_912.0,
    1_073_741_824.0,
    4_294_967_296.0,
];

/// The `client` label of a platform-owned function (the
/// `fnr_client_policies` key the platform uses for itself).
pub const PLATFORM_CLIENT_LABEL: &str = fc_function_model::FunctionOwner::PLATFORM_KEY;

pub const CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";

type Labels = Vec<(String, String)>;

/// Which listener an invocation came in on (`entry` label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerEntry {
    Private,
    Public,
}

impl ListenerEntry {
    pub fn wire_value(self) -> &'static str {
        match self {
            ListenerEntry::Private => "private",
            ListenerEntry::Public => "public",
        }
    }
}

/// What the listener's permits expose to `fc_fn_permits_available` (H5).
pub trait PermitsView: Send + Sync {
    fn host_available(&self) -> i64;
    fn known_addresses(&self) -> Vec<FunctionAddress>;
    /// `None` when the address has no permits yet.
    fn function_available(&self, address: &FunctionAddress) -> Option<i64>;
    fn forget(&self, address: &FunctionAddress);
}

fn new_histogram() -> Histogram {
    Histogram::new(DURATION_BUCKETS)
}

fn new_fuel_histogram() -> Histogram {
    Histogram::new(FUEL_BUCKETS)
}

fn new_memory_histogram() -> Histogram {
    Histogram::new(MEMORY_BUCKETS)
}

pub struct FnMetrics {
    registry: Registry,
    invocations: Family<Labels, Counter>,
    duration: Family<Labels, Histogram, fn() -> Histogram>,
    active: Family<Labels, Gauge>,
    fuel_total: Family<Labels, Counter>,
    fuel: Family<Labels, Histogram, fn() -> Histogram>,
    peak_memory: Family<Labels, Histogram, fn() -> Histogram>,
    /// The `(address, client)` label sets the consumption series use, per
    /// address, for the sweep.
    consumption_labels: Mutex<HashMap<String, HashSet<Labels>>>,
    load_errors: Family<Labels, Counter>,
    reconcile_total: Family<Labels, Counter>,
    last_reconcile_success: Gauge<f64, AtomicU64>,
    permits: Arc<RwLock<Option<Arc<dyn PermitsView>>>>,
    budget: Arc<RwLock<Option<ExecBudget>>>,
    /// The invocation label sets created per address, for the sweep.
    invocation_labels: Mutex<HashMap<String, HashSet<Labels>>>,
    known_addresses: Mutex<HashSet<FunctionAddress>>,
}

struct RegistryGauges {
    registry: Arc<FunctionRegistry>,
}

impl fmt::Debug for RegistryGauges {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("RegistryGauges")
    }
}

impl Collector for RegistryGauges {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), fmt::Error> {
        let loaded = ConstGauge::new(self.registry.len() as i64);
        loaded.encode(encoder.encode_descriptor(
            "fc_fn_loaded",
            "Loaded functions",
            None,
            loaded.metric_type(),
        )?)?;
        let warm = ConstGauge::new(self.registry.warm_count() as i64);
        warm.encode(encoder.encode_descriptor(
            "fc_fn_warm",
            "Loaded functions exempt from LRU eviction",
            None,
            warm.metric_type(),
        )?)?;
        Ok(())
    }
}

struct PermitsGauge {
    permits: Arc<RwLock<Option<Arc<dyn PermitsView>>>>,
}

impl fmt::Debug for PermitsGauge {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("PermitsGauge")
    }
}

impl Collector for PermitsGauge {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), fmt::Error> {
        let Some(permits) = self.permits.read().clone() else {
            return Ok(());
        };
        let mut metric = encoder.encode_descriptor(
            "fc_fn_permits_available",
            "Invocation permits currently available",
            None,
            MetricType::Gauge,
        )?;
        let host: Labels = vec![
            ("scope".into(), "host".into()),
            ("address".into(), String::new()),
        ];
        ConstGauge::new(permits.host_available()).encode(metric.encode_family(&host)?)?;
        for address in permits.known_addresses() {
            if let Some(available) = permits.function_available(&address) {
                let labels: Labels = vec![
                    ("scope".into(), "function".into()),
                    ("address".into(), address.render()),
                ];
                ConstGauge::new(available).encode(metric.encode_family(&labels)?)?;
            }
        }
        Ok(())
    }
}

/// The host-wide executing budget (`FC_FN_MAX_EXECUTING`, every runtime
/// together; Rust host only): `fc_fn_executing`, `fc_fn_executing_waiting`
/// and `fc_fn_executing_limit`, once the host has a budget.
struct ExecutingGauges {
    budget: Arc<RwLock<Option<ExecBudget>>>,
}

impl fmt::Debug for ExecutingGauges {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("ExecutingGauges")
    }
}

impl Collector for ExecutingGauges {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), fmt::Error> {
        let Some(budget) = self.budget.read().clone() else {
            return Ok(());
        };
        for (name, help, value) in [
            (
                "fc_fn_executing",
                "Guests executing now, every runtime together (FC_FN_MAX_EXECUTING permits in use)",
                budget.executing(),
            ),
            (
                "fc_fn_executing_waiting",
                "Guests ready to run, queued for an FC_FN_MAX_EXECUTING permit",
                budget.waiting(),
            ),
            (
                "fc_fn_executing_limit",
                "FC_FN_MAX_EXECUTING: guests that may execute at once, every runtime together",
                budget.limit(),
            ),
        ] {
            let gauge = ConstGauge::new(value as i64);
            gauge.encode(encoder.encode_descriptor(name, help, None, gauge.metric_type())?)?;
        }
        Ok(())
    }
}

fn labels(pairs: &[(&str, &str)]) -> Labels {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

impl FnMetrics {
    pub fn new(function_registry: Arc<FunctionRegistry>) -> Self {
        let mut registry = Registry::default();
        let invocations = Family::<Labels, Counter>::default();
        registry.register(
            "fc_fn_invocations",
            "Function invocations, by outcome and listener entry",
            invocations.clone(),
        );
        let duration =
            Family::<Labels, Histogram, fn() -> Histogram>::new_with_constructor(new_histogram);
        registry.register(
            "fc_fn_duration_seconds",
            "Invocation time, only when the function was entered",
            duration.clone(),
        );
        let active = Family::<Labels, Gauge>::default();
        registry.register("fc_fn_active", "Invocations in flight", active.clone());
        let fuel_total = Family::<Labels, Counter>::default();
        registry.register(
            "fc_fn_fuel",
            "wasmtime fuel spent by invocations, by function and client",
            fuel_total.clone(),
        );
        let fuel = Family::<Labels, Histogram, fn() -> Histogram>::new_with_constructor(
            new_fuel_histogram,
        );
        registry.register(
            "fc_fn_invocation_fuel",
            "wasmtime fuel spent per invocation, by function and client",
            fuel.clone(),
        );
        let peak_memory = Family::<Labels, Histogram, fn() -> Histogram>::new_with_constructor(
            new_memory_histogram,
        );
        registry.register(
            "fc_fn_invocation_peak_memory_bytes",
            "Peak guest linear memory per invocation, by function and client",
            peak_memory.clone(),
        );
        let load_errors = Family::<Labels, Counter>::default();
        registry.register(
            "fc_fn_load_errors",
            "LoadOutcome.Refused reasons and prepare failures",
            load_errors.clone(),
        );
        let reconcile_total = Family::<Labels, Counter>::default();
        registry.register(
            "fc_fn_reconcile",
            "Reconcile cycles, by outcome",
            reconcile_total.clone(),
        );
        let last_reconcile_success = Gauge::<f64, AtomicU64>::default();
        registry.register(
            "fc_fn_last_reconcile_success_timestamp_seconds",
            "Unix time of the last successful reconcile",
            last_reconcile_success.clone(),
        );
        registry.register_collector(Box::new(RegistryGauges {
            registry: function_registry,
        }));
        let permits: Arc<RwLock<Option<Arc<dyn PermitsView>>>> = Arc::new(RwLock::new(None));
        registry.register_collector(Box::new(PermitsGauge {
            permits: permits.clone(),
        }));
        let budget: Arc<RwLock<Option<ExecBudget>>> = Arc::new(RwLock::new(None));
        registry.register_collector(Box::new(ExecutingGauges {
            budget: budget.clone(),
        }));
        Self {
            registry,
            invocations,
            duration,
            active,
            fuel_total,
            fuel,
            peak_memory,
            consumption_labels: Mutex::new(HashMap::new()),
            load_errors,
            reconcile_total,
            last_reconcile_success,
            permits,
            budget,
            invocation_labels: Mutex::new(HashMap::new()),
            known_addresses: Mutex::new(HashSet::new()),
        }
    }

    /// The Prometheus scrape, OpenMetrics text.
    pub fn encode(&self) -> Result<String, fmt::Error> {
        let mut out = String::new();
        text::encode(&mut out, &self.registry)?;
        Ok(out)
    }

    // ── invocation observer (the listener, H5) ───────────────────────────

    pub fn permits_ready(&self, permits: Arc<dyn PermitsView>) {
        *self.permits.write() = Some(permits);
    }

    /// The executing budget the host's runtimes share, for
    /// `fc_fn_executing*`.
    pub fn budget_ready(&self, budget: ExecBudget) {
        *self.budget.write() = Some(budget);
    }

    fn count_invocation(
        &self,
        address: String,
        version: String,
        outcome: &str,
        entry: ListenerEntry,
    ) {
        let set = labels(&[
            ("address", &address),
            ("version", &version),
            ("outcome", outcome),
            ("entry", entry.wire_value()),
        ]);
        self.invocations.get_or_create(&set).inc();
        if address != "-" {
            self.invocation_labels
                .lock()
                .entry(address)
                .or_default()
                .insert(set);
        }
    }

    /// A host refusal: `address` is `-` when unknown, `version` always `-`.
    pub fn refused(&self, outcome: &str, address: Option<&FunctionAddress>, entry: ListenerEntry) {
        let address = address
            .map(FunctionAddress::render)
            .unwrap_or_else(|| "-".to_owned());
        self.count_invocation(address, "-".to_owned(), outcome, entry);
    }

    pub fn entered(&self, address: &FunctionAddress) {
        self.active
            .get_or_create(&labels(&[("address", &address.render())]))
            .inc();
    }

    pub fn exited(&self, address: &FunctionAddress) {
        self.active
            .get_or_create(&labels(&[("address", &address.render())]))
            .dec();
    }

    pub fn completed(
        &self,
        address: &FunctionAddress,
        version: i32,
        outcome: &str,
        elapsed: Duration,
        entry: ListenerEntry,
    ) {
        let rendered = address.render();
        self.count_invocation(rendered.clone(), version.to_string(), outcome, entry);
        self.duration
            .get_or_create(&labels(&[("address", &rendered)]))
            .observe(elapsed.as_secs_f64());
    }

    /// What one invocation consumed (owner decision #13): fuel as a
    /// running total and a per-invocation histogram, and the peak linear
    /// memory, all labelled `address` and `client` (the owning client's id,
    /// or `PLATFORM`). Per client: `sum by (client)`.
    pub fn consumed(
        &self,
        address: &FunctionAddress,
        client_id: Option<&str>,
        consumption: Consumption,
    ) {
        if consumption == Consumption::default() {
            return;
        }
        let rendered = address.render();
        let set = labels(&[
            ("address", &rendered),
            ("client", client_id.unwrap_or(PLATFORM_CLIENT_LABEL)),
        ]);
        if let Some(fuel) = consumption.fuel {
            self.fuel_total.get_or_create(&set).inc_by(fuel);
            self.fuel.get_or_create(&set).observe(fuel as f64);
        }
        if let Some(bytes) = consumption.peak_memory_bytes {
            self.peak_memory.get_or_create(&set).observe(bytes as f64);
        }
        self.consumption_labels
            .lock()
            .entry(rendered)
            .or_default()
            .insert(set);
    }

    // ── cardinality: an address leaving desired state drops its series ──

    pub fn sweep_dead_addresses(&self, currently_desired: &HashSet<FunctionAddress>) {
        let previous = mem::replace(&mut *self.known_addresses.lock(), currently_desired.clone());
        for address in previous.difference(currently_desired) {
            let rendered = address.render();
            let by_address = labels(&[("address", &rendered)]);
            self.active.remove(&by_address);
            self.duration.remove(&by_address);
            if let Some(sets) = self.invocation_labels.lock().remove(&rendered) {
                for set in sets {
                    self.invocations.remove(&set);
                }
            }
            if let Some(sets) = self.consumption_labels.lock().remove(&rendered) {
                for set in sets {
                    self.fuel_total.remove(&set);
                    self.fuel.remove(&set);
                    self.peak_memory.remove(&set);
                }
            }
            if let Some(permits) = self.permits.read().clone() {
                permits.forget(address);
            }
        }
    }
}

impl ReconcileObserver for FnMetrics {
    fn load_error(&self, reason: &str) {
        self.load_errors
            .get_or_create(&labels(&[("reason", reason)]))
            .inc();
    }

    fn reconciled(&self, outcome: &str, success: bool, now: DateTime<Utc>) {
        self.reconcile_total
            .get_or_create(&labels(&[("outcome", outcome)]))
            .inc();
        if success {
            self.last_reconcile_success
                .set(now.timestamp_millis() as f64 / 1000.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;
    use crate::exec::ExecBudget;
    use crate::invoke::Consumption;

    fn metrics() -> FnMetrics {
        FnMetrics::new(Arc::new(FunctionRegistry::new(10, Arc::new(SystemClock))))
    }

    #[test]
    fn exposes_the_fc_fn_names() {
        let m = metrics();
        m.load_error("ARTIFACT:DigestMismatch");
        m.reconciled("changed", true, Utc::now());
        m.reconciled("failed", false, Utc::now());
        let text = m.encode().unwrap();
        for series in [
            "fc_fn_load_errors_total{reason=\"ARTIFACT:DigestMismatch\"} 1",
            "fc_fn_reconcile_total{outcome=\"changed\"} 1",
            "fc_fn_reconcile_total{outcome=\"failed\"} 1",
            "fc_fn_last_reconcile_success_timestamp_seconds ",
            "fc_fn_loaded 0",
            "fc_fn_warm 0",
        ] {
            assert!(text.contains(series), "{series} missing from:\n{text}");
        }
    }

    #[tokio::test]
    async fn the_shared_executing_budget_is_exposed_once_the_host_has_one() {
        let m = metrics();
        assert!(!m.encode().unwrap().contains("fc_fn_executing"));
        let budget = ExecBudget::new(3);
        m.budget_ready(budget.clone());
        // Scraped from inside a guest's poll: that guest holds a permit.
        let observed = budget.run(async { m.encode().unwrap() }).await;
        for series in [
            "fc_fn_executing 1",
            "fc_fn_executing_waiting 0",
            "fc_fn_executing_limit 3",
        ] {
            assert!(
                observed.contains(series),
                "{series} missing from:\n{observed}"
            );
        }
        assert!(m.encode().unwrap().contains("fc_fn_executing 0"));
    }

    #[test]
    fn failure_does_not_move_the_success_timestamp() {
        let m = metrics();
        let t = chrono::TimeZone::timestamp_opt(&Utc, 1_000, 0).unwrap();
        m.reconciled("not_modified", true, t);
        m.reconciled("failed", false, t + chrono::Duration::seconds(50));
        assert!(m
            .encode()
            .unwrap()
            .contains("fc_fn_last_reconcile_success_timestamp_seconds 1000.0"));
    }

    #[test]
    fn consumption_is_exported_per_function_and_client() {
        let m = metrics();
        let a = FunctionAddress::parse("a.b.c").unwrap();
        let used = |fuel, memory| Consumption {
            fuel: Some(fuel),
            peak_memory_bytes: Some(memory),
        };
        m.consumed(&a, Some("clt_1"), used(40_000, 2 << 20));
        m.consumed(&a, Some("clt_1"), used(60_000, 1 << 20));
        m.consumed(&a, None, used(5, 1));
        // A runtime that meters nothing adds no series.
        m.consumed(&a, Some("clt_2"), Consumption::default());
        let text = m.encode().unwrap();
        for series in [
            r#"fc_fn_fuel_total{address="a.b.c",client="clt_1"} 100000"#,
            r#"fc_fn_fuel_total{address="a.b.c",client="PLATFORM"} 5"#,
            r#"fc_fn_invocation_fuel_count{address="a.b.c",client="clt_1"} 2"#,
            r#"fc_fn_invocation_fuel_sum{address="a.b.c",client="clt_1"} 100000.0"#,
            r#"fc_fn_invocation_peak_memory_bytes_count{address="a.b.c",client="clt_1"} 2"#,
            r#"fc_fn_invocation_peak_memory_bytes_sum{address="a.b.c",client="clt_1"} 3145728.0"#,
        ] {
            assert!(text.contains(series), "{series} missing from:\n{text}");
        }
        assert!(!text.contains("clt_2"), "{text}");
    }

    #[test]
    fn series_for_an_address_leaving_desired_state_are_swept() {
        let m = metrics();
        let gone = FunctionAddress::parse("a.b.gone").unwrap();
        let kept = FunctionAddress::parse("a.b.kept").unwrap();
        m.sweep_dead_addresses(&[gone.clone(), kept.clone()].into());
        for address in [&gone, &kept] {
            m.entered(address);
            m.exited(address);
            m.completed(
                address,
                1,
                "ok",
                Duration::from_millis(3),
                ListenerEntry::Private,
            );
        }
        m.refused("not_found", None, ListenerEntry::Public);
        m.consumed(
            &gone,
            Some("clt_1"),
            Consumption {
                fuel: Some(10),
                peak_memory_bytes: Some(1 << 20),
            },
        );
        assert!(m.encode().unwrap().contains("a.b.gone"));
        m.sweep_dead_addresses(&[kept.clone()].into());
        let text = m.encode().unwrap();
        assert!(!text.contains("a.b.gone"), "{text}");
        assert!(text.contains("a.b.kept"));
        assert!(text.contains("address=\"-\""));
    }
}

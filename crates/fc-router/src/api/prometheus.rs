//! The router's pool, queue and breaker series for `/metrics`, rendered from
//! snapshots at scrape time (Go: `internal/router/api/prometheus.go`).
//!
//! The names and labels are the established metrics contract, so existing
//! dashboards and alerts keep working on either implementation:
//!
//! Per pool (label `pool`):
//! - `fc_pool_queue_size`, `fc_pool_active_workers`, `fc_pool_message_groups`
//! - `fc_messages_processed_total{success,result}`
//! - `fc_messages_submitted_total`
//! - `fc_messages_rejected_total{reason}`
//! - `fc_rate_limit_exceeded_total`
//! - `fc_mediation_duration_seconds` (histogram)
//!
//! Global: `fc_in_pipeline_messages`, `fc_router_panics_recovered_total`.
//!
//! Per queue: `fc_queue_pending_messages`, `fc_queue_in_flight_messages`,
//! `fc_consumer_messages_received_total`, `fc_queue_messages_total{outcome}`,
//! `fc_consumer_polls_total`, `fc_consumer_errors_total{type}`.
//!
//! Per breaker (label `target`): `fc_circuit_breaker_open`,
//! `fc_circuit_breaker_calls_total{outcome}`.
//!
//! Rendering from a snapshot, rather than setting gauges on the global
//! recorder as things happen, means a removed pool or queue drops out of the
//! next scrape instead of keeping its last value for ever.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use fc_common::PoolStats;
use fc_queue::QueueMetrics;

use crate::circuit_breaker_registry::{CircuitBreakerState, CircuitBreakerStats};
use crate::event_counters::{ConsumerEventSnapshot, PoolEventSnapshot, MEDIATION_BUCKETS_SECONDS};

/// Everything one scrape reads.
pub(crate) struct RouterSnapshot {
    pub pools: Vec<PoolStats>,
    /// Event-time counters, by pool code.
    pub pool_events: HashMap<String, PoolEventSnapshot>,
    pub queues: Vec<QueueMetrics>,
    /// Poll counters, by queue name.
    pub consumers: Vec<(String, ConsumerEventSnapshot)>,
    pub breakers: HashMap<String, CircuitBreakerStats>,
    pub in_pipeline: usize,
    pub panics: u64,
}

struct Family {
    kind: &'static str,
    help: &'static str,
    samples: Vec<String>,
}

/// Series grouped by family, so each family's samples are contiguous under
/// one `# HELP` / `# TYPE` header, as the text format requires.
#[derive(Default)]
struct Families(BTreeMap<&'static str, Family>);

impl Families {
    fn family(
        &mut self,
        name: &'static str,
        kind: &'static str,
        help: &'static str,
    ) -> &mut Family {
        self.0.entry(name).or_insert_with(|| Family {
            kind,
            help,
            samples: Vec::new(),
        })
    }

    fn gauge(&mut self, name: &'static str, help: &'static str, labels: &[(&str, &str)], v: f64) {
        self.family(name, "gauge", help)
            .samples
            .push(sample(name, labels, v));
    }

    /// `name` carries the `_total` suffix.
    fn counter(&mut self, name: &'static str, help: &'static str, labels: &[(&str, &str)], v: f64) {
        self.family(name, "counter", help)
            .samples
            .push(sample(name, labels, v));
    }

    fn histogram(
        &mut self,
        name: &'static str,
        help: &'static str,
        labels: &[(&str, &str)],
        cumulative: &[u64],
        count: u64,
        sum: f64,
    ) {
        let family = self.family(name, "histogram", help);
        for (bound, n) in MEDIATION_BUCKETS_SECONDS.iter().zip(cumulative) {
            let le = bound.to_string();
            family.samples.push(sample(
                &format!("{name}_bucket"),
                &with_le(labels, &le),
                *n as f64,
            ));
        }
        family.samples.push(sample(
            &format!("{name}_bucket"),
            &with_le(labels, "+Inf"),
            count as f64,
        ));
        family
            .samples
            .push(sample(&format!("{name}_sum"), labels, sum));
        family
            .samples
            .push(sample(&format!("{name}_count"), labels, count as f64));
    }

    #[expect(
        clippy::let_underscore_must_use,
        reason = "writing to a String cannot fail"
    )]
    fn render(self, out: &mut String) {
        for (name, family) in self.0 {
            let _ = writeln!(out, "# HELP {name} {}", family.help);
            let _ = writeln!(out, "# TYPE {name} {}", family.kind);
            for line in family.samples {
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
}

fn with_le<'a>(labels: &[(&'a str, &'a str)], le: &'a str) -> Vec<(&'a str, &'a str)> {
    let mut all = labels.to_vec();
    all.push(("le", le));
    all
}

fn sample(name: &str, labels: &[(&str, &str)], value: f64) -> String {
    if labels.is_empty() {
        return format!("{name} {value}");
    }
    let labels = labels
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", escape(v)))
        .collect::<Vec<_>>()
        .join(",");
    format!("{name}{{{labels}}} {value}")
}

/// Escape a label value for the text format.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Trim an SQS queue URL to its last segment, so label cardinality stays
/// bounded (`my-queue`, not `https://sqs.../my-queue`).
fn normalise_queue_id(id: &str) -> &str {
    match id.rfind('/') {
        Some(i) if i + 1 < id.len() => &id[i + 1..],
        _ => id,
    }
}

/// Append the router's series for `snapshot` to `out`.
pub(crate) fn render(out: &mut String, snapshot: &RouterSnapshot) {
    let mut f = Families::default();

    let mut pools: Vec<&PoolStats> = snapshot.pools.iter().collect();
    pools.sort_by(|a, b| a.pool_code.cmp(&b.pool_code));
    for pool in pools {
        let code = pool.pool_code.as_str();
        let by_pool = [("pool", code)];
        f.gauge(
            "fc_pool_queue_size",
            "Messages buffered in group queues awaiting dispatch.",
            &by_pool,
            pool.queue_size as f64,
        );
        f.gauge(
            "fc_pool_active_workers",
            "Currently active workers per pool.",
            &by_pool,
            pool.active_workers as f64,
        );
        f.gauge(
            "fc_pool_message_groups",
            "Distinct message groups currently holding buffered work.",
            &by_pool,
            pool.message_group_count as f64,
        );
        if let Some(m) = &pool.metrics {
            f.counter(
                "fc_rate_limit_exceeded_total",
                "Cumulative rate-limit events.",
                &by_pool,
                m.total_rate_limited as f64,
            );
        }
        let Some(events) = snapshot.pool_events.get(code) else {
            continue;
        };
        f.counter(
            "fc_messages_submitted_total",
            "Cumulative messages routed to the pool.",
            &by_pool,
            events.submitted as f64,
        );
        for (reason, n) in &events.rejected {
            f.counter(
                "fc_messages_rejected_total",
                "Cumulative messages the pool handed back or settled without delivering them, by reason.",
                &[("pool", code), ("reason", reason)],
                *n as f64,
            );
        }
        for (result, n) in &events.processed {
            let success = if *result == "SUCCESS" {
                "true"
            } else {
                "false"
            };
            f.counter(
                "fc_messages_processed_total",
                "Cumulative messages processed, by success and mediation result.",
                &[("pool", code), ("success", success), ("result", result)],
                *n as f64,
            );
        }
        let h = &events.duration;
        f.histogram(
            "fc_mediation_duration_seconds",
            "Mediation latency in seconds.",
            &by_pool,
            &h.counts,
            h.count,
            h.sum_seconds,
        );
    }

    let mut queues: Vec<&QueueMetrics> = snapshot.queues.iter().collect();
    queues.sort_by(|a, b| a.queue_identifier.cmp(&b.queue_identifier));
    for m in queues {
        let queue = normalise_queue_id(&m.queue_identifier);
        f.gauge(
            "fc_queue_pending_messages",
            "Approximate messages waiting on the broker.",
            &[("queue", queue)],
            m.pending_messages as f64,
        );
        f.gauge(
            "fc_queue_in_flight_messages",
            "Approximate messages currently being processed by consumers.",
            &[("queue", queue)],
            m.in_flight_messages as f64,
        );
        f.counter(
            "fc_consumer_messages_received_total",
            "Cumulative messages received from the broker by this consumer.",
            &[("consumer", queue)],
            m.total_polled as f64,
        );
        for (outcome, n) in [
            ("acked", m.total_acked),
            ("nacked", m.total_nacked),
            ("deferred", m.total_deferred),
        ] {
            f.counter(
                "fc_queue_messages_total",
                "Cumulative consumer ack/nack/defer outcomes.",
                &[("queue", queue), ("outcome", outcome)],
                n as f64,
            );
        }
    }

    let mut consumers: Vec<&(String, ConsumerEventSnapshot)> = snapshot.consumers.iter().collect();
    consumers.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, c) in consumers {
        let queue = normalise_queue_id(name);
        f.counter(
            "fc_consumer_polls_total",
            "Cumulative broker polls by the queue's consumer.",
            &[("queue", queue)],
            c.polls as f64,
        );
        if c.poll_errors > 0 {
            f.counter(
                "fc_consumer_errors_total",
                "Cumulative failed broker polls, by type (poll error, recovered panic).",
                &[("queue", queue), ("type", "poll")],
                c.poll_errors as f64,
            );
        }
    }

    let mut breakers: Vec<(&String, &CircuitBreakerStats)> = snapshot.breakers.iter().collect();
    breakers.sort_by(|a, b| a.0.cmp(b.0));
    for (name, b) in breakers {
        f.gauge(
            "fc_circuit_breaker_open",
            "1 when the breaker is OPEN, 0 otherwise.",
            &[("target", name)],
            if b.state == CircuitBreakerState::Open {
                1.0
            } else {
                0.0
            },
        );
        for (outcome, n) in [("success", b.successful_calls), ("failure", b.failed_calls)] {
            f.counter(
                "fc_circuit_breaker_calls_total",
                "Cumulative breaker outcomes.",
                &[("target", name), ("outcome", outcome)],
                n as f64,
            );
        }
    }

    f.gauge(
        "fc_in_pipeline_messages",
        "Total in-flight messages across all pools.",
        &[],
        snapshot.in_pipeline as f64,
    );
    f.counter(
        "fc_router_panics_recovered_total",
        "Cumulative panics in this process, each logged with its backtrace and span context.",
        &[],
        snapshot.panics as f64,
    );

    f.render(out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::EnhancedPoolMetrics;

    use crate::event_counters::{PoolEventCounters, RejectReason};
    use crate::metrics::PoolMetricsCollector;
    use fc_common::MediationResult;

    fn pool(code: &str) -> PoolStats {
        let collector = PoolMetricsCollector::new();
        collector.record_success(3);
        let metrics: EnhancedPoolMetrics = collector.get_metrics();
        PoolStats {
            pool_code: code.to_string(),
            concurrency: 10,
            active_workers: 2,
            queue_size: 7,
            queue_capacity: 200,
            message_group_count: 3,
            rate_limit_per_minute: None,
            is_rate_limited: false,
            metrics: Some(metrics),
        }
    }

    fn snapshot() -> RouterSnapshot {
        let events = PoolEventCounters::default();
        events.submitted();
        events.submitted();
        events.processed(MediationResult::Success);
        events.processed(MediationResult::ErrorConfig);
        events.reject(RejectReason::Capacity, 4);
        events.observe_duration_ms(3);
        events.observe_duration_ms(30_000);

        RouterSnapshot {
            pools: vec![pool("B-POOL"), pool("A-POOL")],
            pool_events: HashMap::from([("A-POOL".to_string(), events.snapshot())]),
            queues: vec![QueueMetrics {
                pending_messages: 11,
                in_flight_messages: 5,
                queue_identifier: "https://sqs.eu-west-1.amazonaws.com/123/my-queue".to_string(),
                total_polled: 100,
                total_acked: 90,
                total_nacked: 8,
                total_deferred: 2,
            }],
            consumers: vec![(
                "my-queue".to_string(),
                ConsumerEventSnapshot {
                    polls: 50,
                    poll_errors: 1,
                },
            )],
            breakers: HashMap::from([(
                "https://svc.example/hook".to_string(),
                CircuitBreakerStats {
                    name: "https://svc.example/hook".to_string(),
                    state: CircuitBreakerState::Open,
                    successful_calls: 9,
                    failed_calls: 6,
                    rejected_calls: 0,
                    failure_rate: 0.4,
                    buffered_calls: 15,
                    buffer_size: 20,
                },
            )]),
            in_pipeline: 12,
            panics: 0,
        }
    }

    fn rendered() -> String {
        let mut out = String::new();
        render(&mut out, &snapshot());
        out
    }

    #[test]
    fn pool_gauges_are_emitted_for_every_pool_in_order() {
        let text = rendered();
        let a = text.find("fc_pool_queue_size{pool=\"A-POOL\"} 7").unwrap();
        let b = text.find("fc_pool_queue_size{pool=\"B-POOL\"} 7").unwrap();
        assert!(a < b, "deterministic order by pool code");
        assert!(text.contains("fc_pool_active_workers{pool=\"A-POOL\"} 2"));
        assert!(text.contains("fc_pool_message_groups{pool=\"A-POOL\"} 3"));
    }

    #[test]
    fn event_counters_are_labelled_like_gos() {
        let text = rendered();
        assert!(text.contains("fc_messages_submitted_total{pool=\"A-POOL\"} 2"));
        assert!(text.contains("fc_messages_rejected_total{pool=\"A-POOL\",reason=\"capacity\"} 4"));
        assert!(text.contains(
            "fc_messages_processed_total{pool=\"A-POOL\",success=\"true\",result=\"SUCCESS\"} 1"
        ));
        assert!(text.contains(
            "fc_messages_processed_total{pool=\"A-POOL\",success=\"false\",result=\"ERROR_CONFIG\"} 1"
        ));
        assert!(text.contains("fc_rate_limit_exceeded_total{pool=\"A-POOL\"} 0"));
        // A pool the event map has not seen yet has no event series.
        assert!(!text.contains("fc_messages_submitted_total{pool=\"B-POOL\"}"));
    }

    #[test]
    fn the_duration_histogram_has_cumulative_buckets_and_an_inf_bucket() {
        let text = rendered();
        assert!(text.contains("# TYPE fc_mediation_duration_seconds histogram"));
        assert!(
            text.contains("fc_mediation_duration_seconds_bucket{pool=\"A-POOL\",le=\"0.005\"} 1")
        );
        assert!(text.contains("fc_mediation_duration_seconds_bucket{pool=\"A-POOL\",le=\"10\"} 1"));
        assert!(
            text.contains("fc_mediation_duration_seconds_bucket{pool=\"A-POOL\",le=\"+Inf\"} 2")
        );
        assert!(text.contains("fc_mediation_duration_seconds_count{pool=\"A-POOL\"} 2"));
        assert!(text.contains("fc_mediation_duration_seconds_sum{pool=\"A-POOL\"} 30.003"));
    }

    #[test]
    fn queue_labels_drop_the_sqs_url_prefix() {
        let text = rendered();
        assert!(text.contains("fc_queue_pending_messages{queue=\"my-queue\"} 11"));
        assert!(text.contains("fc_queue_in_flight_messages{queue=\"my-queue\"} 5"));
        assert!(text.contains("fc_consumer_messages_received_total{consumer=\"my-queue\"} 100"));
        assert!(text.contains("fc_queue_messages_total{queue=\"my-queue\",outcome=\"acked\"} 90"));
        assert!(text.contains("fc_queue_messages_total{queue=\"my-queue\",outcome=\"deferred\"} 2"));
        assert!(text.contains("fc_consumer_polls_total{queue=\"my-queue\"} 50"));
        assert!(text.contains("fc_consumer_errors_total{queue=\"my-queue\",type=\"poll\"} 1"));
    }

    #[test]
    fn breakers_and_globals() {
        let text = rendered();
        assert!(text.contains("fc_circuit_breaker_open{target=\"https://svc.example/hook\"} 1"));
        assert!(text.contains(
            "fc_circuit_breaker_calls_total{target=\"https://svc.example/hook\",outcome=\"failure\"} 6"
        ));
        assert!(text.contains("fc_in_pipeline_messages 12"));
        assert!(text.contains("fc_router_panics_recovered_total 0"));
    }

    #[test]
    fn each_family_has_one_header_and_contiguous_samples() {
        let text = rendered();
        for name in [
            "fc_pool_queue_size",
            "fc_queue_messages_total",
            "fc_circuit_breaker_calls_total",
        ] {
            assert_eq!(
                text.matches(&format!("# TYPE {name} ")).count(),
                1,
                "{name} declared once"
            );
        }
        // Samples of a family sit between its header and the next header.
        let lines: Vec<&str> = text.lines().collect();
        let mut current = None::<&str>;
        for line in lines {
            if let Some(rest) = line.strip_prefix("# TYPE ") {
                current = rest.split(' ').next();
            } else if !line.starts_with('#') {
                let family = current.expect("a sample follows a header");
                assert!(line.starts_with(family), "{line} is not in family {family}");
            }
        }
    }

    #[test]
    fn label_values_are_escaped() {
        let mut snap = snapshot();
        snap.breakers = HashMap::from([(
            "a\"b\\c\nd".to_string(),
            CircuitBreakerStats {
                name: String::new(),
                state: CircuitBreakerState::Closed,
                successful_calls: 0,
                failed_calls: 0,
                rejected_calls: 0,
                failure_rate: 0.0,
                buffered_calls: 0,
                buffer_size: 0,
            },
        )]);
        let mut out = String::new();
        render(&mut out, &snap);
        assert!(out.contains("fc_circuit_breaker_open{target=\"a\\\"b\\\\c\\nd\"} 0"));
    }

    #[test]
    fn a_queue_id_with_no_path_is_kept_whole() {
        assert_eq!(normalise_queue_id("my-queue"), "my-queue");
        assert_eq!(normalise_queue_id("trailing/"), "trailing/");
        assert_eq!(normalise_queue_id("a/b/c"), "c");
    }
}

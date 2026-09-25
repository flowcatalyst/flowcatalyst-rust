//! Normalise each side's run into a summary, check the absolute invariants
//! on each side on its own, and diff the two summaries.
//!
//! Normalisation: stimuli are identified by the harness key (`hk`) the
//! harness put in the payload, never by the platform's job/event ids;
//! times become offsets from the first stimulus, and inter-attempt gaps
//! become coarse buckets.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::scenario::Scenario;
use crate::side::{is_terminal, SideRun};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub stimuli: usize,
    pub ingest_errors: usize,
    pub deliveries: usize,
    pub accepted: usize,
    pub lost: Vec<String>,
    /// hk → number of acceptances, only where > 1.
    pub duplicates: BTreeMap<String, u32>,
    /// attempts per stimulus → how many stimuli (receiver's view).
    pub attempts: BTreeMap<u32, u32>,
    pub per_hk_attempts: BTreeMap<String, u32>,
    /// group → seqs in order of first acceptance.
    pub group_order: BTreeMap<String, Vec<i64>>,
    /// Deliveries of a group member that arrived while an earlier member
    /// of the same group was still unaccepted: "hk (seq s) arrived before
    /// seq t was accepted".
    pub fifo_breaks: Vec<String>,
    pub job_status: BTreeMap<String, u32>,
    pub job_attempts: BTreeMap<i32, u32>,
    pub jobs: usize,
    pub retry_gaps: BTreeMap<String, u32>,
    pub min_retry_gap_ms: Option<u64>,
    pub max_concurrency: u32,
    pub signatures: BTreeMap<String, u32>,
    pub header_names: BTreeSet<String>,
    pub outbox_left: BTreeMap<i16, i64>,
    pub settled: bool,
    pub settle_ms: u64,
}

type HmacSha256 = Hmac<Sha256>;

const TRANSPORT_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "user-agent",
    "accept-encoding",
    "accept",
    "connection",
    "traceparent",
    "tracestate",
    "transfer-encoding",
];

fn gap_bucket(ms: u64) -> &'static str {
    match ms {
        // Wide on purpose: the boundaries sit between the delays either
        // side can mean (immediate; Retry-After / delaySeconds of a few
        // seconds and Go's 5s first backoff; Go's 15s/30s backoff and 30s
        // default deferral; 60s/120s), so poll jitter never flips a bucket.
        0..=999 => "a:<1s",
        1000..=9999 => "b:1-10s",
        10000..=44999 => "c:10-45s",
        45000..=119999 => "d:45-120s",
        _ => "e:>120s",
    }
}

pub fn summarise(run: &SideRun, signing_secret: Option<&str>) -> Summary {
    let mut per_hk: BTreeMap<String, Vec<&crate::receiver::Delivery>> = BTreeMap::new();
    for d in &run.deliveries {
        if let Some(hk) = &d.hk {
            per_hk.entry(hk.clone()).or_default().push(d);
        }
    }
    let mut lost = Vec::new();
    let mut duplicates = BTreeMap::new();
    let mut attempts = BTreeMap::new();
    let mut per_hk_attempts = BTreeMap::new();
    let mut accepted = 0;
    let mut retry_gaps: BTreeMap<String, u32> = BTreeMap::new();
    let mut min_gap: Option<u64> = None;
    for m in &run.sent {
        let ds = per_hk.get(&m.hk).cloned().unwrap_or_default();
        let acc = ds.iter().filter(|d| d.accepted).count() as u32;
        if acc == 0 {
            lost.push(m.hk.clone());
        } else {
            accepted += 1;
        }
        if acc > 1 {
            duplicates.insert(m.hk.clone(), acc);
        }
        *attempts.entry(ds.len() as u32).or_insert(0) += 1;
        per_hk_attempts.insert(m.hk.clone(), ds.len() as u32);
        for w in ds.windows(2) {
            // From the previous exchange's end (answer, or arrival when the
            // caller hung up) to the next arrival: the wait the pipeline
            // chose, not the target's own slowness.
            let prev_end = w[0]
                .answered_at_ms
                .or(w[0].hung_up_at_ms)
                .unwrap_or(w[0].at_ms);
            let gap = w[1].at_ms.saturating_sub(prev_end);
            *retry_gaps.entry(gap_bucket(gap).to_string()).or_insert(0) += 1;
            min_gap = Some(min_gap.map_or(gap, |g: u64| g.min(gap)));
        }
    }

    // Group order and FIFO breaks, from arrival order.
    let mut group_order: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut fifo_breaks = Vec::new();
    let mut ordered = run.deliveries.clone();
    ordered.sort_by_key(|d| d.at_ms);
    let mut accepted_seqs: HashMap<String, BTreeSet<i64>> = HashMap::new();
    let sent_by_group: HashMap<String, Vec<i64>> =
        run.sent.iter().fold(HashMap::new(), |mut m, s| {
            if let Some(g) = &s.group {
                m.entry(g.clone()).or_insert_with(Vec::new).push(s.seq);
            }
            m
        });
    for d in &ordered {
        let (Some(g), Some(seq)) = (&d.group, d.seq) else {
            continue;
        };
        let done = accepted_seqs.entry(g.clone()).or_default();
        if let Some(earlier) = sent_by_group
            .get(g)
            .and_then(|all| all.iter().find(|&&t| t < seq && !done.contains(&t)))
        {
            if fifo_breaks.len() < 20 {
                fifo_breaks.push(format!(
                    "{} (group {g} seq {seq}) arrived at {}ms before seq {earlier} was accepted",
                    d.hk.as_deref().unwrap_or("?"),
                    d.at_ms
                ));
            }
        }
        if d.accepted && done.insert(seq) {
            group_order.entry(g.clone()).or_default().push(seq);
        }
    }

    let mut job_status = BTreeMap::new();
    let mut job_attempts = BTreeMap::new();
    for j in &run.jobs {
        *job_status.entry(j.status.clone()).or_insert(0) += 1;
        *job_attempts.entry(j.attempt_count).or_insert(0) += 1;
    }

    let mut signatures = BTreeMap::new();
    let mut header_names = BTreeSet::new();
    for d in &run.deliveries {
        let get = |n: &str| {
            d.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(n))
                .map(|(_, v)| v.clone())
        };
        let verdict = match (
            get("x-flowcatalyst-signature"),
            get("x-flowcatalyst-timestamp"),
        ) {
            (Some(sig), Some(ts)) => match signing_secret {
                Some(secret) => {
                    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
                    mac.update(ts.as_bytes());
                    mac.update(d.raw_body.as_bytes());
                    let want = hex::encode(mac.finalize().into_bytes());
                    if want.eq_ignore_ascii_case(&sig) {
                        "valid"
                    } else {
                        "invalid"
                    }
                }
                None => "unverifiable",
            },
            (Some(_), None) => "signature-without-timestamp",
            _ => "unsigned",
        };
        *signatures.entry(verdict.to_string()).or_insert(0) += 1;
        for (k, _) in &d.headers {
            let k = k.to_ascii_lowercase();
            if !TRANSPORT_HEADERS.contains(&k.as_str()) {
                header_names.insert(k);
            }
        }
    }

    Summary {
        stimuli: run.sent.len(),
        ingest_errors: run.ingest_errors.len(),
        deliveries: run.deliveries.len(),
        accepted,
        lost,
        duplicates,
        attempts,
        per_hk_attempts,
        group_order,
        fifo_breaks,
        job_status,
        job_attempts,
        jobs: run.jobs.len(),
        retry_gaps,
        min_retry_gap_ms: min_gap,
        max_concurrency: run
            .deliveries
            .iter()
            .map(|d| d.concurrent)
            .max()
            .unwrap_or(0),
        signatures,
        header_names,
        outbox_left: run.outbox_left.clone(),
        settled: run.settled,
        settle_ms: run.settle_ms,
    }
}

/// Absolute invariants, checked on one side alone.
pub fn invariants(s: &Scenario, sum: &Summary) -> Vec<String> {
    let mut v = Vec::new();
    let inv = &s.invariants;
    if sum.stimuli == 0 {
        v.push("no stimulus was sent".into());
        return v;
    }
    if inv.no_loss.unwrap_or(true) && !sum.lost.is_empty() {
        v.push(format!(
            "loss: {} of {} stimuli never accepted by the target{}",
            sum.lost.len(),
            sum.stimuli,
            sample(&sum.lost)
        ));
    }
    if inv.no_duplicate_acceptance.unwrap_or(true) && !sum.duplicates.is_empty() {
        v.push(format!(
            "duplicates: {} stimuli accepted more than once ({})",
            sum.duplicates.len(),
            sum.duplicates
                .iter()
                .take(5)
                .map(|(k, n)| format!("{k}×{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let strict_default = s
        .targets
        .iter()
        .any(|t| t.dispatch_mode == "BLOCK_ON_ERROR");
    if inv.group_order.unwrap_or(strict_default) {
        for (g, seqs) in &sum.group_order {
            if seqs.windows(2).any(|w| w[0] > w[1]) {
                v.push(format!("group order: {g} accepted as {seqs:?}"));
            }
        }
        if !sum.fifo_breaks.is_empty() {
            v.push(format!(
                "FIFO: {} deliveries overtook an unaccepted predecessor (first: {})",
                sum.fifo_breaks.len(),
                sum.fifo_breaks[0]
            ));
        }
    }
    if let Some(max) = inv.max_attempts {
        let over: Vec<_> = sum
            .per_hk_attempts
            .iter()
            .filter(|(_, n)| **n > max)
            .collect();
        if !over.is_empty() {
            v.push(format!(
                "retry budget: {} stimuli delivered more than {max} times (max {})",
                over.len(),
                over.iter().map(|(_, n)| **n).max().unwrap_or(0)
            ));
        }
    }
    if let (Some(min), Some(got)) = (inv.min_retry_gap_ms, sum.min_retry_gap_ms) {
        if got < min {
            v.push(format!(
                "backoff: retries {got}ms apart, expected at least {min}ms"
            ));
        }
    }
    if let Some(max) = inv.max_concurrency {
        if sum.max_concurrency > max {
            v.push(format!(
                "pool concurrency: {} requests in flight at the target, pool allows {max}",
                sum.max_concurrency
            ));
        }
    }
    if inv.signed.unwrap_or(true) && sum.deliveries > 0 {
        let bad: u32 = sum
            .signatures
            .iter()
            .filter(|(k, _)| k.as_str() != "valid")
            .map(|(_, n)| *n)
            .sum();
        if bad > 0 {
            v.push(format!(
                "signature: {bad} deliveries not validly signed ({:?})",
                sum.signatures
            ));
        }
    }
    let non_terminal: u32 = sum
        .job_status
        .iter()
        .filter(|(k, _)| !is_terminal(k))
        .map(|(_, n)| *n)
        .sum();
    if sum.settled && non_terminal > 0 && sum.lost.is_empty() {
        v.push(format!(
            "status: every stimulus accepted but {non_terminal} dispatch jobs are not terminal ({:?})",
            sum.job_status
        ));
    }
    v
}

fn sample(v: &[String]) -> String {
    if v.is_empty() {
        return String::new();
    }
    let s: Vec<_> = v.iter().take(3).cloned().collect();
    format!(" (e.g. {})", s.join(", "))
}

#[derive(Debug, Clone, Serialize)]
pub struct Diff {
    pub field: String,
    pub go: String,
    pub rust: String,
    /// The expected-diffs entry that accepts it, if any.
    pub accepted_by: Option<String>,
}

fn show<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

pub fn diff(go: &Summary, rust: &Summary) -> Vec<Diff> {
    let mut out = Vec::new();
    let mut push = |field: &str, a: String, b: String| {
        if a != b {
            out.push(Diff {
                field: field.to_string(),
                go: a,
                rust: b,
                accepted_by: None,
            });
        }
    };
    push(
        "ingestErrors",
        show(&go.ingest_errors),
        show(&rust.ingest_errors),
    );
    push("accepted", show(&go.accepted), show(&rust.accepted));
    push("lost", show(&go.lost.len()), show(&rust.lost.len()));
    push(
        "duplicates",
        show(&go.duplicates.len()),
        show(&rust.duplicates.len()),
    );
    push("attempts", show(&go.attempts), show(&rust.attempts));
    let groups: BTreeSet<&String> = go
        .group_order
        .keys()
        .chain(rust.group_order.keys())
        .collect();
    for g in groups {
        push(
            &format!("groupOrder/{g}"),
            show(&go.group_order.get(g)),
            show(&rust.group_order.get(g)),
        );
    }
    push(
        "fifoBreaks",
        show(&go.fifo_breaks.len()),
        show(&rust.fifo_breaks.len()),
    );
    push("jobStatus", show(&go.job_status), show(&rust.job_status));
    push(
        "jobAttempts",
        show(&go.job_attempts),
        show(&rust.job_attempts),
    );
    push("retryGaps", show(&go.retry_gaps), show(&rust.retry_gaps));
    push(
        "signatures",
        show(&go.signatures.keys().collect::<Vec<_>>()),
        show(&rust.signatures.keys().collect::<Vec<_>>()),
    );
    push("headers", show(&go.header_names), show(&rust.header_names));
    push("outboxLeft", show(&go.outbox_left), show(&rust.outbox_left));
    push("settled", show(&go.settled), show(&rust.settled));
    out
}

/// `expected-diffs.json`: deliberate deviations, each citing a ruling.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectedDiff {
    /// Scenario name, or a prefix ending in `*`, or `*`.
    pub scenario: String,
    /// Diff field, or a prefix ending in `*` (`groupOrder/*`).
    pub field: String,
    pub reason: String,
    /// Owner ruling / decision id (`owner-decisions-2026-09-25 #28`, `X-01` …).
    pub ruling: String,
    /// The difference only shows when a disruption lands in a window of a
    /// few hundred milliseconds (a Go defect that depends on timing): an
    /// entry that matched nothing in a run is then not stale.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub intermittent: bool,
}

/// The diff field an invariant violation is accepted under:
/// `invariant/<side>/<kind>`, the kind being the violation's own prefix
/// (`loss`, `duplicates`, `group-order`, `fifo`, `retry-budget`, `backoff`,
/// `pool-concurrency`, `signature`, `status`). A side's broken invariant is
/// a FAIL unless an `expected-diffs.json` entry names it — which is how a
/// Go defect is cited (decision #31).
pub fn invariant_field(side: crate::stack::SideKind, violation: &str) -> String {
    let kind = violation
        .split(':')
        .next()
        .unwrap_or(violation)
        .trim()
        .to_lowercase()
        .replace(' ', "-");
    format!("invariant/{}/{kind}", side.label())
}

impl ExpectedDiff {
    pub fn matches(&self, scenario: &str, field: &str) -> bool {
        glob(&self.scenario, scenario) && glob(&self.field, field)
    }
    pub fn id(&self) -> String {
        format!("{} / {} ({})", self.scenario, self.field, self.ruling)
    }
}

fn glob(pat: &str, s: &str) -> bool {
    match pat.strip_suffix('*') {
        Some(prefix) => s.starts_with(prefix),
        None => pat == s,
    }
}

pub fn load_expected(path: &std::path::Path) -> anyhow::Result<Vec<ExpectedDiff>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let v: Vec<ExpectedDiff> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    for e in &v {
        if e.ruling.trim().is_empty() || e.reason.trim().is_empty() {
            anyhow::bail!("expected-diffs entry {} has no ruling or reason", e.id());
        }
    }
    Ok(v)
}

#[cfg(test)]
mod invariant_citation_tests {
    use super::*;
    use crate::stack::SideKind;

    #[test]
    fn a_violation_is_cited_by_side_and_kind() {
        assert_eq!(
            invariant_field(SideKind::Go, "loss: 2 of 40 stimuli never accepted"),
            "invariant/go/loss"
        );
        assert_eq!(
            invariant_field(SideKind::Rust, "group order: g1 accepted as [2, 1]"),
            "invariant/rust/group-order"
        );
        assert_eq!(
            invariant_field(SideKind::Go, "FIFO: 3 deliveries overtook"),
            "invariant/go/fifo"
        );
    }

    #[test]
    fn an_entry_may_be_intermittent() {
        let v: Vec<ExpectedDiff> = serde_json::from_str(
            r#"[{"scenario": "s", "field": "invariant/go/loss", "reason": "r", "ruling": "x", "intermittent": true},
                {"scenario": "s", "field": "lost", "reason": "r", "ruling": "x"}]"#,
        )
        .unwrap();
        assert!(v[0].intermittent && !v[1].intermittent);
        assert!(v[0].matches("s", "invariant/go/loss"));
    }
}

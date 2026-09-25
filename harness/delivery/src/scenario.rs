//! Scenario model: JSON files under `harness/delivery/scenarios/`.
//!
//! A scenario is data: the setup (pools, subscriptions / targets), the
//! stimuli (events, dispatch jobs or outbox rows), the receiver script, any
//! disruptions, and the settle condition. See the crate README for the
//! format with examples.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scenario {
    /// Short id; also the receiver path segment. `[a-z0-9-]` only.
    pub name: String,
    /// What the scenario pins, and which review finding it covers.
    pub description: String,
    #[serde(default)]
    pub covers: Vec<String>,
    #[serde(default)]
    pub pools: Vec<PoolSpec>,
    /// Webhook targets: one subscription (event path) or one connection
    /// target (dispatch-job path) each.
    pub targets: Vec<TargetSpec>,
    pub stimuli: Vec<Stimulus>,
    #[serde(default)]
    pub disruptions: Vec<Disruption>,
    pub settle: Settle,
    /// Absolute invariants checked on each side on its own.
    #[serde(default)]
    pub invariants: Invariants,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PoolSpec {
    /// Suffix; the pool code becomes `H-<scenario>-<code>` (upper-cased).
    pub code: String,
    pub concurrency: u32,
    #[serde(default)]
    pub rate_limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetSpec {
    /// Name, used as the receiver path segment and referenced by stimuli.
    pub name: String,
    /// BLOCK_ON_ERROR | NEXT_ON_ERROR | IMMEDIATE
    pub dispatch_mode: String,
    /// Pool (by `PoolSpec::code`); none = the platform default pool.
    #[serde(default)]
    pub pool: Option<String>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    /// `receiver` (default) or `refused` (a port nothing listens on).
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub script: ReceiverScript,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Stimulus {
    /// Events through `POST /api/events/batch`; fan-out via the target's
    /// subscription.
    #[serde(rename_all = "camelCase")]
    Events {
        target: String,
        count: u32,
        /// Message groups; messages are dealt round-robin. Empty = no group.
        #[serde(default)]
        groups: Vec<String>,
        /// Events per batch request.
        #[serde(default = "default_batch")]
        batch: u32,
    },
    /// Dispatch jobs through `POST /api/dispatch-jobs/batch`.
    #[serde(rename_all = "camelCase")]
    DispatchJobs {
        target: String,
        count: u32,
        #[serde(default)]
        groups: Vec<String>,
        #[serde(default = "default_batch")]
        batch: u32,
    },
    /// Events written as rows of the SDK outbox table, forwarded by the
    /// side's outbox processor.
    #[serde(rename_all = "camelCase")]
    OutboxEvents {
        target: String,
        count: u32,
        #[serde(default)]
        groups: Vec<String>,
    },
    /// Wait before the next stimulus.
    #[serde(rename_all = "camelCase")]
    Pause { ms: u64 },
}

fn default_batch() -> u32 {
    50
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Disruption {
    /// When, relative to the first stimulus.
    pub at_ms: u64,
    /// Which process: `router` | `worker` (scheduler) | `platform`.
    pub process: String,
    /// `restart` (SIGTERM, wait, start) | `kill` (SIGKILL, start) |
    /// `down` (SIGTERM, keep down for `downMs`, start).
    pub action: String,
    #[serde(default)]
    pub down_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settle {
    /// Settled when every stimulus has been accepted by the receiver at
    /// least once …
    #[serde(default = "yes")]
    pub all_accepted: bool,
    /// … or when this many deliveries were recorded (for scenarios where
    /// acceptance never comes: permanent 4xx, connection refused).
    #[serde(default)]
    pub min_deliveries: Option<usize>,
    /// … or when the side's database holds at least one dispatch job per
    /// stimulus and every one of them is terminal (COMPLETED / FAILED /
    /// CANCELLED / EXPIRED) — for targets that never answer at all.
    #[serde(default)]
    pub all_terminal: bool,
    /// Then wait this long for stragglers (duplicates show up here).
    #[serde(default = "default_quiet")]
    pub quiet_ms: u64,
    /// Give up after this long; unsettled is reported, not an error.
    pub timeout_ms: u64,
}

fn yes() -> bool {
    true
}
fn default_quiet() -> u64 {
    5_000
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Invariants {
    /// Every stimulus accepted at least once (default true).
    #[serde(default)]
    pub no_loss: Option<bool>,
    /// No stimulus accepted more than once (default true).
    #[serde(default)]
    pub no_duplicate_acceptance: Option<bool>,
    /// Within each group, acceptances arrive in stimulus order (default:
    /// true for BLOCK_ON_ERROR targets, false otherwise).
    #[serde(default)]
    pub group_order: Option<bool>,
    /// Upper bound on attempts per stimulus (the retry budget).
    #[serde(default)]
    pub max_attempts: Option<u32>,
    /// Lower bound on the gap between the first two attempts of a retried
    /// stimulus (backoff honoured), in ms.
    #[serde(default)]
    pub min_retry_gap_ms: Option<u64>,
    /// Upper bound on requests in flight at the target at once (the pool's
    /// concurrency honoured).
    #[serde(default)]
    pub max_concurrency: Option<u32>,
    /// Every delivery carries a valid `X-FlowCatalyst-Signature` for the
    /// harness signer's secret (default true).
    #[serde(default)]
    pub signed: Option<bool>,
}

/// Per-target receiver script.
///
/// The response for a delivery is chosen in this order: the first `bySeq`
/// rule whose seq matches and whose `attempts` window contains this
/// delivery's attempt number; then the first `byAttempt` rule whose window
/// contains the attempt number; then `default`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiverScript {
    #[serde(default)]
    pub default: Option<ScriptedResponse>,
    #[serde(default)]
    pub by_attempt: Vec<AttemptRule>,
    #[serde(default)]
    pub by_seq: Vec<SeqRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptRule {
    /// Inclusive window of 1-based attempt numbers, e.g. `[1, 2]`.
    pub attempts: [u32; 2],
    pub respond: ScriptedResponse,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeqRule {
    pub seq: i64,
    #[serde(default = "all_attempts")]
    pub attempts: [u32; 2],
    pub respond: ScriptedResponse,
}

fn all_attempts() -> [u32; 2] {
    [1, u32::MAX]
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptedResponse {
    pub status: u16,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Answer after this delay (slow target).
    #[serde(default)]
    pub delay_ms: Option<u64>,
    /// Never answer (timeout).
    #[serde(default)]
    pub hang: bool,
}

impl Default for ScriptedResponse {
    fn default() -> Self {
        ScriptedResponse {
            status: 200,
            body: Some(serde_json::json!({"ack": true})),
            headers: BTreeMap::new(),
            delay_ms: None,
            hang: false,
        }
    }
}

impl ReceiverScript {
    pub fn pick(&self, attempt: u32, seq: Option<i64>) -> ScriptedResponse {
        if let Some(seq) = seq {
            if let Some(r) = self
                .by_seq
                .iter()
                .find(|r| r.seq == seq && attempt >= r.attempts[0] && attempt <= r.attempts[1])
            {
                return r.respond.clone();
            }
        }
        if let Some(r) = self
            .by_attempt
            .iter()
            .find(|r| attempt >= r.attempts[0] && attempt <= r.attempts[1])
        {
            return r.respond.clone();
        }
        self.default.clone().unwrap_or_default()
    }
}

impl Stimulus {
    pub fn target(&self) -> Option<&str> {
        match self {
            Stimulus::Events { target, .. }
            | Stimulus::DispatchJobs { target, .. }
            | Stimulus::OutboxEvents { target, .. } => Some(target),
            Stimulus::Pause { .. } => None,
        }
    }
}

pub fn load_dir(dir: &Path) -> anyhow::Result<Vec<Scenario>> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    entries.sort();
    for p in entries {
        let text = std::fs::read_to_string(&p)?;
        let s: Scenario =
            serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
        out.push(s);
    }
    Ok(out)
}

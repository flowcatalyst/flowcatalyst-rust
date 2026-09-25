//! Report: `report.md` (readable) and `report.json` (everything, including
//! every recorded delivery) in the run directory, next to each side's
//! process logs.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::compare::{self, Diff, ExpectedDiff, Summary};
use crate::scenario::Scenario;
use crate::side::{Side, SideRun};
use crate::stack::SideKind;
use crate::Options;

#[derive(Debug, Clone, Serialize)]
pub struct SideResult {
    pub side: SideKind,
    pub summary: Option<Summary>,
    pub invariant_violations: Vec<String>,
    pub diagnosis: Option<String>,
    pub run: Option<SideRun>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioResult {
    pub name: String,
    pub description: String,
    pub covers: Vec<String>,
    /// PASS | ACCEPTED | DIFF | FAIL | ERROR
    pub verdict: String,
    pub sides: Vec<SideResult>,
    pub diffs: Vec<Diff>,
}

fn diagnose(run: &SideRun, sum: &Summary) -> Option<String> {
    if let Some(e) = &run.error {
        return Some(e.clone());
    }
    if sum.stimuli == 0 {
        return Some("nothing was sent".into());
    }
    if sum.lost.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if sum.ingest_errors > 0 {
        parts.push(format!(
            "ingest refused {} request(s): {}",
            sum.ingest_errors,
            run.ingest_errors.first().cloned().unwrap_or_default()
        ));
    }
    let outbox_rows: i64 = run.outbox_left.values().sum();
    if outbox_rows > 0 {
        parts.push(format!(
            "{outbox_rows} outbox row(s) still in the table by status {:?} (0 pending, 9 in progress, 2-6 error)",
            run.outbox_left
        ));
    }
    if sum.jobs == 0 {
        let via_events = run.sent.iter().any(|m| m.kind != "dispatch-job");
        match run.events {
            Some((0, _)) if via_events => parts.push(
                "no event of the scenario was stored: ingest (or the outbox processor) never delivered it to the platform".into(),
            ),
            Some((stored, 0)) if via_events => parts.push(format!(
                "{stored} events stored, none fanned out, no dispatch job: event fan-out is not running or failing (see the platform's ERROR lines under Stacks)"
            )),
            Some((stored, fanned)) if via_events => parts.push(format!(
                "{stored} events stored, {fanned} stamped fanned out, but no dispatch job exists: fan-out matched no subscription (review H7)"
            )),
            _ => parts.push(
                "no dispatch job exists for the scenario: nothing got past ingest".into(),
            ),
        }
    } else {
        let queued = sum.job_status.get("QUEUED").copied().unwrap_or(0) as usize;
        let pending = sum.job_status.get("PENDING").copied().unwrap_or(0) as usize;
        let depth = run.queue_depth.unwrap_or((0, 0));
        if sum.deliveries == 0 && queued == sum.jobs && depth == (0, 0) {
            parts.push(format!(
                "all {queued} jobs are QUEUED, the queue is empty and the target saw nothing: the jobs were marked queued without a message reaching SQS (review C1: the scheduler's publisher is a no-op; or C2: API-created jobs inserted QUEUED, which the poller never reads)"
            ));
        } else if sum.deliveries == 0 && pending == sum.jobs {
            parts.push(format!(
                "all {pending} jobs are still PENDING: the scheduler never dispatched them"
            ));
        } else if depth.0 + depth.1 > 0 {
            parts.push(format!(
                "{} message(s) visible and {} in flight on the queue: the router is not draining it",
                depth.0, depth.1
            ));
        }
        parts.push(format!("job statuses {:?}", sum.job_status));
    }
    if !run.settled {
        parts.push(format!("did not settle within {}ms", run.settle_ms));
    }
    Some(parts.join("; "))
}

impl ScenarioResult {
    pub fn build(
        sc: &Scenario,
        runs: Vec<SideRun>,
        sides: &[Arc<Side>],
        side_errors: &[(SideKind, String)],
        expected: &[ExpectedDiff],
    ) -> ScenarioResult {
        let mut results = Vec::new();
        for run in runs {
            let secret = sides
                .iter()
                .find(|s| s.kind == run.side)
                .and_then(|s| s.signer.as_ref())
                .and_then(|s| s.signing_secret.clone());
            if let Some(e) = &run.error {
                results.push(SideResult {
                    side: run.side,
                    summary: None,
                    invariant_violations: vec![],
                    diagnosis: Some(e.clone()),
                    error: Some(e.clone()),
                    run: Some(run),
                });
                continue;
            }
            let sum = compare::summarise(&run, secret.as_deref());
            let inv = compare::invariants(sc, &sum);
            results.push(SideResult {
                side: run.side,
                diagnosis: diagnose(&run, &sum),
                summary: Some(sum),
                invariant_violations: inv,
                error: None,
                run: Some(run),
            });
        }
        for (k, e) in side_errors {
            results.push(SideResult {
                side: *k,
                summary: None,
                invariant_violations: vec![],
                diagnosis: Some(e.clone()),
                run: None,
                error: Some(e.clone()),
            });
        }
        results.sort_by_key(|r| r.side);

        let mut diffs = Vec::new();
        let go = results
            .iter()
            .find(|r| r.side == SideKind::Go)
            .and_then(|r| r.summary.as_ref());
        let rust = results
            .iter()
            .find(|r| r.side == SideKind::Rust)
            .and_then(|r| r.summary.as_ref());
        if let (Some(g), Some(r)) = (go, rust) {
            diffs = compare::diff(g, r);
            for d in &mut diffs {
                d.accepted_by = expected
                    .iter()
                    .find(|e| e.matches(&sc.name, &d.field))
                    .map(ExpectedDiff::id);
            }
        }
        let verdict = if results.iter().any(|r| r.error.is_some()) {
            "ERROR"
        } else if diffs.iter().any(|d| d.accepted_by.is_none()) {
            "DIFF"
        } else if results.iter().any(|r| !r.invariant_violations.is_empty()) {
            "FAIL"
        } else if !diffs.is_empty() {
            "ACCEPTED"
        } else {
            "PASS"
        };
        ScenarioResult {
            name: sc.name.clone(),
            description: sc.description.clone(),
            covers: sc.covers.clone(),
            verdict: verdict.into(),
            sides: results,
            diffs,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StackInfo {
    pub side: SideKind,
    pub binaries: Vec<String>,
    pub notes: Vec<String>,
    pub boot_error: Option<String>,
    pub logs: Vec<String>,
    /// Per process: the most frequent ERROR lines, normalised.
    pub top_errors: Vec<(String, Vec<(usize, String)>)>,
}

/// Strip ANSI colour, timestamps and ids so repeats of one error collapse
/// into one line; return the `n` most frequent ERROR lines (plus fatal
/// start-up errors printed without a level).
pub fn top_errors(log: &Path, n: usize) -> Vec<(usize, String)> {
    let Ok(text) = std::fs::read_to_string(log) else {
        return vec![];
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for raw in text.lines() {
        let line = strip_ansi(raw);
        let msg = if line.contains("\"level\":\"ERROR\"") {
            // Go slog JSON: keep msg + err.
            let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
            let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("");
            let err = v.get("err").and_then(|m| m.as_str()).unwrap_or("");
            format!("{msg}: {err}")
        } else if let Some(i) = line.find(" ERROR ") {
            line[i + 7..].trim().to_string()
        } else if line.starts_with("Error: ") {
            line.to_string()
        } else {
            continue;
        };
        let norm = normalise_error(&msg);
        *counts.entry(norm).or_insert(0) += 1;
    }
    let mut v: Vec<(usize, String)> = counts.into_iter().map(|(k, c)| (c, k)).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.truncate(n);
    v
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn normalise_error(s: &str) -> String {
    // Collapse long alphanumeric tokens (ids) and digit runs.
    let words: Vec<String> = s
        .split(' ')
        .map(|w| {
            let alnum = w.chars().filter(|c| c.is_ascii_alphanumeric()).count();
            let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
            if alnum >= 12 && digits >= 3 {
                "«id»".to_string()
            } else {
                w.to_string()
            }
        })
        .collect();
    crate::api::truncate(&words.join(" "), 240)
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub run_id: String,
    pub go_commit: Option<String>,
    pub rust_commit: Option<String>,
    pub only: Option<String>,
    pub stacks: Vec<StackInfo>,
    pub scenarios: Vec<ScenarioResult>,
    pub expected_used: Vec<String>,
    pub expected_stale: Vec<String>,
}

impl Report {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        run_id: &str,
        opts: &Options,
        scenarios: Vec<ScenarioResult>,
        sides: &[Arc<Side>],
        side_errors: &[(SideKind, String)],
        expected: &[ExpectedDiff],
        go_commit: Option<String>,
        rust_commit: Option<String>,
    ) -> Report {
        let mut stacks: Vec<StackInfo> = sides
            .iter()
            .map(|s| StackInfo {
                side: s.kind,
                binaries: vec![
                    format!("platform/worker: {}", s.bins.platform.display()),
                    format!("router: {}", s.bins.router.display()),
                    format!("outbox: {}", s.bins.outbox.display()),
                ],
                notes: s.notes.clone(),
                boot_error: s.boot_error.clone(),
                logs: s.logs().iter().map(|p| p.display().to_string()).collect(),
                top_errors: s
                    .logs()
                    .iter()
                    .map(|p| {
                        (
                            p.file_stem()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            top_errors(p, 5),
                        )
                    })
                    .filter(|(_, v)| !v.is_empty())
                    .collect(),
            })
            .collect();
        for (k, e) in side_errors {
            stacks.push(StackInfo {
                side: *k,
                binaries: vec![],
                notes: vec![],
                boot_error: Some(e.clone()),
                logs: vec![],
                top_errors: vec![],
            });
        }
        stacks.sort_by_key(|s| s.side);
        let used: std::collections::BTreeSet<String> = scenarios
            .iter()
            .flat_map(|s| s.diffs.iter().filter_map(|d| d.accepted_by.clone()))
            .collect();
        let stale = if opts.only.is_none() && opts.sides.len() == 2 {
            expected
                .iter()
                .map(ExpectedDiff::id)
                .filter(|id| !used.contains(id))
                .collect()
        } else {
            vec![]
        };
        Report {
            run_id: run_id.into(),
            go_commit,
            rust_commit,
            only: opts.only.clone(),
            stacks,
            scenarios,
            expected_used: used.into_iter().collect(),
            expected_stale: stale,
        }
    }

    /// Non-zero exit: any DIFF / FAIL / ERROR, or a stale expected diff.
    pub fn ok(&self) -> bool {
        self.expected_stale.is_empty()
            && self
                .scenarios
                .iter()
                .all(|s| s.verdict == "PASS" || s.verdict == "ACCEPTED")
    }

    /// True when the harness itself worked: every side booted and every
    /// scenario ran (verdicts aside).
    pub fn harness_ok(&self) -> bool {
        self.stacks.iter().all(|s| s.boot_error.is_none())
    }

    pub fn write(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::write(dir.join("report.json"), serde_json::to_string_pretty(self)?)?;
        std::fs::write(dir.join("report.md"), self.markdown())?;
        Ok(())
    }

    pub fn markdown(&self) -> String {
        let mut m = String::new();
        let _ = writeln!(m, "# Delivery parity run `{}`\n", self.run_id);
        let _ = writeln!(
            m,
            "Go `{}` · Rust `{}`{}\n",
            self.go_commit.as_deref().unwrap_or("?"),
            self.rust_commit.as_deref().unwrap_or("?"),
            self.only
                .as_ref()
                .map(|o| format!(" · only `{o}`"))
                .unwrap_or_default()
        );
        let _ = writeln!(m, "## Verdicts\n");
        let _ = writeln!(m, "| Scenario | Verdict | Go | Rust | Unaccepted diffs |");
        let _ = writeln!(m, "|---|---|---|---|---|");
        for s in &self.scenarios {
            let cell = |k: SideKind| -> String {
                match s.sides.iter().find(|r| r.side == k) {
                    None => "—".into(),
                    Some(r) => match (&r.error, &r.summary) {
                        (Some(_), _) => "ERROR".into(),
                        (None, Some(sum)) => {
                            let inv = if r.invariant_violations.is_empty() {
                                "invariants ok".to_string()
                            } else {
                                format!("**{} invariant(s) broken**", r.invariant_violations.len())
                            };
                            format!(
                                "{}/{} accepted, {} deliveries; {inv}",
                                sum.accepted, sum.stimuli, sum.deliveries
                            )
                        }
                        _ => "?".into(),
                    },
                }
            };
            let n = s.diffs.iter().filter(|d| d.accepted_by.is_none()).count();
            let _ = writeln!(
                m,
                "| [{}](#{}) | **{}** | {} | {} | {} |",
                s.name,
                s.name,
                s.verdict,
                cell(SideKind::Go),
                cell(SideKind::Rust),
                n
            );
        }
        let _ = writeln!(
            m,
            "\nPASS: identical after normalisation, invariants hold on both sides. ACCEPTED: only diffs listed in `expected-diffs.json`. DIFF: an unaccepted difference. FAIL: no diff, but an invariant broken on some side. ERROR: a side could not run the scenario.\n"
        );

        let _ = writeln!(m, "## Stacks\n");
        for st in &self.stacks {
            let _ = writeln!(m, "### {}\n", st.side.label());
            if let Some(e) = &st.boot_error {
                let _ = writeln!(m, "**Boot failed:** {e}\n");
            }
            for b in &st.binaries {
                let _ = writeln!(m, "- {b}");
            }
            for n in &st.notes {
                let _ = writeln!(m, "- {n}");
            }
            let _ = writeln!(m);
            if !st.top_errors.is_empty() {
                let _ = writeln!(
                    m,
                    "Most frequent ERROR lines in the process logs (whole run):\n"
                );
                let _ = writeln!(m, "| process | count | error |");
                let _ = writeln!(m, "|---|---|---|");
                for (proc_name, errs) in &st.top_errors {
                    for (c, e) in errs {
                        let _ = writeln!(
                            m,
                            "| {proc_name} | {c} | `{}` |",
                            e.replace('|', "\\|").replace('`', "'")
                        );
                    }
                }
                let _ = writeln!(m);
            }
        }

        let _ = writeln!(m, "## Scenarios\n");
        for s in &self.scenarios {
            let _ = writeln!(m, "### {}\n", s.name);
            let _ = writeln!(m, "**{}** — {}\n", s.verdict, s.description);
            if !s.covers.is_empty() {
                let _ = writeln!(m, "Covers: {}\n", s.covers.join(", "));
            }
            let sides: Vec<&SideResult> = s.sides.iter().collect();
            let _ = write!(m, "| | ");
            for r in &sides {
                let _ = write!(m, "{} | ", r.side.label());
            }
            let _ = writeln!(m);
            let _ = write!(m, "|---|");
            for _ in &sides {
                let _ = write!(m, "---|");
            }
            let _ = writeln!(m);
            type Row = (&'static str, fn(&Summary, &SideRun) -> String);
            let rows: Vec<Row> = vec![
                ("accepted / sent", |s, _| {
                    format!("{} / {}", s.accepted, s.stimuli)
                }),
                ("deliveries (all attempts)", |s, _| s.deliveries.to_string()),
                ("lost", |s, _| s.lost.len().to_string()),
                ("accepted more than once", |s, _| {
                    s.duplicates.len().to_string()
                }),
                ("attempts per stimulus → count", |s, _| {
                    fmt_map(&s.attempts)
                }),
                ("group acceptance order", |s, _| {
                    if s.group_order.is_empty() {
                        "—".into()
                    } else {
                        s.group_order
                            .iter()
                            .map(|(g, v)| format!("{g}: {}", compact(v)))
                            .collect::<Vec<_>>()
                            .join("<br>")
                    }
                }),
                ("FIFO overtakes", |s, _| s.fifo_breaks.len().to_string()),
                ("job statuses", |s, _| fmt_map(&s.job_status)),
                ("job attempt_count → jobs", |s, _| {
                    fmt_map(&s.job_attempts)
                }),
                ("retry gaps", |s, _| fmt_map(&s.retry_gaps)),
                ("max in flight at target", |s, _| {
                    s.max_concurrency.to_string()
                }),
                ("signatures", |s, _| fmt_map(&s.signatures)),
                ("ingest errors", |s, _| s.ingest_errors.to_string()),
                ("outbox rows left", |s, _| fmt_map(&s.outbox_left)),
                ("events stored / fanned out", |_, r| {
                    r.events
                        .map(|(a, b)| format!("{a} / {b}"))
                        .unwrap_or("?".into())
                }),
                ("queue (visible, in flight)", |_, r| {
                    r.queue_depth
                        .map(|(a, b)| format!("{a}, {b}"))
                        .unwrap_or("?".into())
                }),
                ("settled", |s, _| {
                    if s.settled {
                        format!("yes, {}ms", s.settle_ms)
                    } else {
                        format!("**no** (gave up at {}ms)", s.settle_ms)
                    }
                }),
                ("disruptions", |_, r| {
                    if r.disruptions.is_empty() {
                        "—".into()
                    } else {
                        r.disruptions.join("<br>")
                    }
                }),
            ];
            for (label, f) in rows {
                let _ = write!(m, "| {label} | ");
                for r in &sides {
                    let cell = match (&r.summary, &r.run) {
                        (Some(sum), Some(run)) => f(sum, run),
                        _ => "—".into(),
                    };
                    let _ = write!(m, "{} | ", cell.replace('|', "\\|"));
                }
                let _ = writeln!(m);
            }
            let _ = writeln!(m);
            for r in &sides {
                if !r.invariant_violations.is_empty() {
                    let _ = writeln!(m, "Invariants broken on **{}**:", r.side.label());
                    for v in &r.invariant_violations {
                        let _ = writeln!(m, "- {v}");
                    }
                    let _ = writeln!(m);
                }
                if let Some(d) = &r.diagnosis {
                    let _ = writeln!(m, "Diagnosis ({}): {d}\n", r.side.label());
                }
            }
            if !s.diffs.is_empty() {
                let _ = writeln!(m, "| Diff | go | rust | accepted by |");
                let _ = writeln!(m, "|---|---|---|---|");
                for d in &s.diffs {
                    let _ = writeln!(
                        m,
                        "| {} | `{}` | `{}` | {} |",
                        d.field,
                        clip(&d.go),
                        clip(&d.rust),
                        d.accepted_by.as_deref().unwrap_or("—")
                    );
                }
                let _ = writeln!(m);
            }
        }
        let _ = writeln!(m, "## expected-diffs.json\n");
        if self.expected_used.is_empty() && self.expected_stale.is_empty() {
            let _ = writeln!(m, "No entries used.");
        }
        for e in &self.expected_used {
            let _ = writeln!(m, "- used: {e}");
        }
        for e in &self.expected_stale {
            let _ = writeln!(m, "- **stale** (matched nothing): {e}");
        }
        m
    }
}

fn fmt_map<K: std::fmt::Display, V: std::fmt::Display>(m: &BTreeMap<K, V>) -> String {
    if m.is_empty() {
        return "—".into();
    }
    m.iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `[1,2,3,4,7,5]` → `1-4,7,5`.
fn compact(v: &[i64]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let mut j = i;
        while j + 1 < v.len() && v[j + 1] == v[j] + 1 {
            j += 1;
        }
        if j > i {
            out.push(format!("{}-{}", v[i], v[j]));
        } else {
            out.push(v[i].to_string());
        }
        i = j + 1;
    }
    out.join(",")
}

fn clip(s: &str) -> String {
    crate::api::truncate(s, 160).replace('|', "\\|")
}

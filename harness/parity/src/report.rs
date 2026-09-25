//! The run's result and its two renderings (spec §6): `report.json` and
//! `report.md`. Exit status is non-zero on any `DIFF`, any `ERROR`, any
//! stale allow-list entry, any false `covers` claim, or coverage under
//! [`REQUIRED_COVERAGE`]: a gate, not a dashboard.

use anyhow::{Context, Result};
use serde::Serialize;
use std::fmt::Write as _;
use std::path::Path;

use crate::coverage::CoverageResult;
use crate::diff::DiffEntry;
use crate::expected::ExpectedDiff;
use crate::record::StepRecord;

/// Raised to `1.0` once every surface route has a scenario (Java spec §8 S3).
pub const REQUIRED_COVERAGE: f64 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum StepStatus {
    /// Both sides agree after normalisation.
    Ok,
    /// The sides disagree, but every diff is allow-listed.
    Accepted,
    /// At least one diff is not allow-listed.
    Diff,
    /// The step itself broke on a side (expect mismatch, bad substitution,
    /// missing capture, transport failure): not a parity finding as such.
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub step_id: String,
    pub status: StepStatus,
    pub diffs: Vec<DiffEntry>,
    pub unaccepted: Vec<DiffEntry>,
    /// Raw records, kept on anything that is not `OK`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub go_record: Option<StepRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_record: Option<StepRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioResult {
    pub file: String,
    pub scenario_name: String,
    pub steps: Vec<StepResult>,
    /// `covers` entries never actually requested on a side: a false claim.
    pub unmet_covers_claims: Vec<String>,
}

impl ScenarioResult {
    pub fn count(&self, status: StepStatus) -> usize {
        self.steps.iter().filter(|s| s.status == status).count()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub go_label: String,
    pub rust_label: String,
    pub seed_path: String,
    pub scenarios: Vec<ScenarioResult>,
    pub coverage: CoverageResult,
    pub stale_entries: Vec<ExpectedDiff>,
}

impl Report {
    fn any(&self, status: StepStatus) -> bool {
        self.scenarios.iter().any(|s| s.count(status) > 0)
    }
    pub fn any_diff(&self) -> bool {
        self.any(StepStatus::Diff)
    }
    pub fn any_error(&self) -> bool {
        self.any(StepStatus::Error)
    }
    pub fn any_false_covers_claim(&self) -> bool {
        self.scenarios
            .iter()
            .any(|s| !s.unmet_covers_claims.is_empty())
    }
    pub fn any_stale_entry(&self) -> bool {
        !self.stale_entries.is_empty()
    }
    pub fn coverage_below_threshold(&self) -> bool {
        self.coverage.lockfile_coverage() < REQUIRED_COVERAGE
            || self.coverage.surface_coverage() < REQUIRED_COVERAGE
    }
    pub fn exit_code(&self) -> i32 {
        i32::from(
            self.any_diff()
                || self.any_error()
                || self.any_false_covers_claim()
                || self.any_stale_entry()
                || self.coverage_below_threshold(),
        )
    }

    pub fn summary_line(&self) -> String {
        format!(
            "exit code {} (diff={} error={} falseCoversClaim={} staleAllowListEntry={} coverageBelowThreshold={})",
            self.exit_code(),
            self.any_diff(),
            self.any_error(),
            self.any_false_covers_claim(),
            self.any_stale_entry(),
            self.coverage_below_threshold()
        )
    }

    pub fn write_json(&self, file: &Path) -> Result<()> {
        write(file, &serde_json::to_string_pretty(self)?)
    }

    /// One line per `OK` step, the full diff otherwise, the counts table,
    /// stale entries and coverage at the end.
    pub fn write_markdown(&self, file: &Path) -> Result<()> {
        let mut md = String::new();
        let _ = writeln!(md, "# Parity report: Go vs Rust\n");
        let _ = writeln!(
            md,
            "- go: {}\n- rust: {}\n- seed: {}\n",
            self.go_label, self.rust_label, self.seed_path
        );
        let _ = writeln!(md, "| scenario file | OK | ACCEPTED | DIFF | ERROR |");
        let _ = writeln!(md, "|---|---:|---:|---:|---:|");
        let mut totals = [0usize; 4];
        for s in &self.scenarios {
            let c = [
                s.count(StepStatus::Ok),
                s.count(StepStatus::Accepted),
                s.count(StepStatus::Diff),
                s.count(StepStatus::Error),
            ];
            for (t, v) in totals.iter_mut().zip(c) {
                *t += v;
            }
            let _ = writeln!(
                md,
                "| {} | {} | {} | {} | {} |",
                s.file, c[0], c[1], c[2], c[3]
            );
        }
        let _ = writeln!(
            md,
            "| **total** | **{}** | **{}** | **{}** | **{}** |\n",
            totals[0], totals[1], totals[2], totals[3]
        );
        for s in &self.scenarios {
            let _ = writeln!(md, "## {} (`{}`)\n", s.scenario_name, s.file);
            if !s.unmet_covers_claims.is_empty() {
                let _ = writeln!(
                    md,
                    "**False `covers` claim(s):** {}\n",
                    s.unmet_covers_claims.join(", ")
                );
            }
            for step in &s.steps {
                match step.status {
                    StepStatus::Ok => {
                        let _ = writeln!(md, "- `OK` {}", step.step_id);
                    }
                    StepStatus::Accepted => {
                        let _ = writeln!(
                            md,
                            "- `ACCEPTED` {} ({} allow-listed diff(s))",
                            step.step_id,
                            step.diffs.len()
                        );
                        step.diffs.iter().for_each(|d| diff_line(&mut md, d));
                    }
                    StepStatus::Diff => {
                        let _ = writeln!(md, "- `DIFF` {}", step.step_id);
                        step.diffs.iter().for_each(|d| diff_line(&mut md, d));
                    }
                    StepStatus::Error => {
                        let _ = writeln!(
                            md,
                            "- `ERROR` {}: {}",
                            step.step_id,
                            step.error.as_deref().unwrap_or_default()
                        );
                    }
                }
            }
            md.push('\n');
        }
        if !self.stale_entries.is_empty() {
            let _ = writeln!(md, "## Stale allow-list entries\n");
            for e in &self.stale_entries {
                let _ = writeln!(
                    md,
                    "- {}/{} {} (ruling {})",
                    e.scenario, e.step, e.pointer, e.ruling
                );
            }
            md.push('\n');
        }
        let c = &self.coverage;
        let _ = writeln!(md, "## Coverage\n");
        let _ = writeln!(
            md,
            "- Go lockfile: {} / {} ({:.1}%)",
            c.lockfile_hit,
            c.lockfile_total,
            c.lockfile_coverage() * 100.0
        );
        let _ = writeln!(
            md,
            "- outside-lockfile surface: {} / {} ({:.1}%)\n",
            c.surface_hit,
            c.surface_total,
            c.surface_coverage() * 100.0
        );
        if !c.missing_lockfile.is_empty() {
            let _ = writeln!(
                md,
                "Lockfile routes no scenario hit: {}\n",
                c.missing_lockfile.join(", ")
            );
        }
        if !c.missing_surface.is_empty() {
            let _ = writeln!(
                md,
                "Surface routes no scenario hit: {}\n",
                c.missing_surface.join(", ")
            );
        }
        let _ = writeln!(md, "Exit code: {}", self.exit_code());
        write(file, &md)
    }
}

fn diff_line(md: &mut String, d: &DiffEntry) {
    let _ = writeln!(md, "  - `{}` go=`{}` rust=`{}`", d.pointer, d.go, d.rust);
}

fn write(file: &Path, content: &str) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, content).with_context(|| format!("write {}", file.display()))
}

//! The `expected-diffs.json` allow-list (spec §6): a diff matching an entry
//! is `ACCEPTED`; an entry that matched nothing in a full run is stale and
//! fails the run, so the file cannot rot.
//!
//! Entry shape: `{"scenario", "step", "pointer", "reason", "ruling"}`.
//! `scenario`/`step` may be `"*"`; `scenario` may end in `*` (a name prefix);
//! `pointer` may start with `**/` (a trailing segment at any depth) or end in
//! `/**` (everything below a prefix). `ruling` is mandatory: for this
//! Go-vs-Rust file it names a decision in `docs/owner-decisions-2026-09-25.md`.
//!
//! `"pointer": "!go-expect"` accepts Go missing a named step's own
//! `expect.status` (a known Go defect) when Rust met it; it must name one
//! step of one scenario.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;

/// The allow-list pointer that accepts Go failing a step's own `expect`.
pub const GO_EXPECT: &str = "!go-expect";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExpectedDiff {
    pub scenario: String,
    pub step: String,
    pub pointer: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub ruling: String,
}

#[derive(Debug, Default)]
pub struct ExpectedDiffs {
    entries: Vec<ExpectedDiff>,
    used: RefCell<HashSet<usize>>,
}

impl ExpectedDiffs {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Loads and validates `path` (absent file = empty list).
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::empty());
        }
        let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let entries: Vec<ExpectedDiff> =
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
        Self::from_entries(entries)
    }

    pub fn from_entries(entries: Vec<ExpectedDiff>) -> Result<Self> {
        for e in &entries {
            if e.ruling.trim().is_empty() {
                bail!(
                    "expected-diffs.json entry has no ruling: {}/{} {}",
                    e.scenario,
                    e.step,
                    e.pointer
                );
            }
            if e.pointer == GO_EXPECT && (e.step == "*" || e.scenario == "*") {
                bail!(
                    "expected-diffs.json: a {GO_EXPECT} entry names one step of one scenario \
                     (a name prefix is fine), never \"*\": {}/{}",
                    e.scenario,
                    e.step
                );
            }
        }
        Ok(Self {
            entries,
            used: RefCell::new(HashSet::new()),
        })
    }

    /// Whether the diff at `scenario`/`step`/`pointer` is allow-listed;
    /// every matching entry is marked used.
    pub fn accepts(&self, scenario: &str, step: &str, pointer: &str) -> bool {
        let mut accepted = false;
        for (i, e) in self.entries.iter().enumerate() {
            if matches(e, scenario, step, pointer) {
                self.used.borrow_mut().insert(i);
                accepted = true;
            }
        }
        accepted
    }

    /// Entries that matched nothing in this run.
    pub fn stale(&self) -> Vec<ExpectedDiff> {
        let used = self.used.borrow();
        // Java marks used by record equality, so duplicate entries share fate.
        let used_entries: HashSet<&ExpectedDiff> = used.iter().map(|i| &self.entries[*i]).collect();
        self.entries
            .iter()
            .filter(|e| !used_entries.contains(e))
            .cloned()
            .collect()
    }
}

fn matches(e: &ExpectedDiff, scenario: &str, step: &str, pointer: &str) -> bool {
    let scenario_ok = e.scenario == "*"
        || e.scenario == scenario
        || e.scenario
            .strip_suffix('*')
            .is_some_and(|prefix| scenario.starts_with(prefix));
    let step_ok = e.step == "*" || e.step == step;
    let pointer_ok = if let Some(tail) = e.pointer.strip_prefix("**").filter(|t| t.starts_with('/'))
    {
        pointer.ends_with(tail)
    } else if let Some(prefix) = e.pointer.strip_suffix("/**") {
        pointer == prefix || pointer.starts_with(&format!("{prefix}/"))
    } else {
        e.pointer == pointer
    };
    scenario_ok && step_ok && pointer_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(scenario: &str, step: &str, pointer: &str) -> ExpectedDiff {
        ExpectedDiff {
            scenario: scenario.into(),
            step: step.into(),
            pointer: pointer.into(),
            reason: "r".into(),
            ruling: "#19".into(),
        }
    }

    #[test]
    fn wildcards_and_staleness() {
        let list = ExpectedDiffs::from_entries(vec![
            e("*", "*", "**/$schema"),
            e("dispatch-jobs*", "get", "/status"),
            e("functions*", "create", "/**"),
            e("x", "y", "/never"),
        ])
        .unwrap();
        assert!(list.accepts("a", "b", "/items/3/$schema"));
        assert!(list.accepts("dispatch-jobs: reads", "get", "/status"));
        assert!(!list.accepts("dispatch-jobs: reads", "get", "/error"));
        assert!(list.accepts("functions: all", "create", "/"));
        assert!(list.accepts("functions: all", "create", "/a/b"));
        assert!(!list.accepts("functions: all", "create2", "/a"));
        assert_eq!(list.stale(), vec![e("x", "y", "/never")]);
    }

    #[test]
    fn an_entry_needs_a_ruling_and_go_expect_names_one_step() {
        let mut no_ruling = e("a", "b", "/c");
        no_ruling.ruling = " ".into();
        assert!(ExpectedDiffs::from_entries(vec![no_ruling]).is_err());
        assert!(ExpectedDiffs::from_entries(vec![e("*", "b", GO_EXPECT)]).is_err());
        assert!(ExpectedDiffs::from_entries(vec![e("a*", "b", GO_EXPECT)]).is_ok());
    }
}

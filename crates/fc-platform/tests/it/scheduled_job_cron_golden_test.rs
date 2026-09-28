//! The scheduled-job poller against what Go's own scheduler answers
//! (`tests/data/scheduled_job/cron-go-golden.json`, written by
//! `tests/go/cron_golden` with robfig/cron v3 as flowcatalyst-go's
//! `scheduledjob/cron.go` configures it):
//!
//! - walks: each cron, in each zone, from starts around 2026's daylight-saving
//!   changes (spring gaps, autumn overlaps, changes at midnight, half-hour and
//!   45-minute offsets, southern-hemisphere zones), fires at Go's instants;
//! - grammar: the poller reads what Go's parser reads and skips what it
//!   refuses, with the same message.
//!
//! Java's `CronExpression.next` walks are in `function_promote_golden_test.rs`.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

use fc_platform::scheduled_job::cron::CronSpec;
use fc_platform::scheduled_job::scheduler::poller::{latest_slot_in_window, next_slot_after};

fn golden() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/scheduled_job/cron-go-golden.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_poller_fires_when_gos_scheduler_fires() {
    let golden = golden();
    let walks = golden["walks"].as_array().unwrap();
    assert!(walks.len() > 2000, "{} walks", walks.len());
    let mut failures = Vec::new();
    for w in walks {
        let cron = w["cron"].as_str().unwrap();
        let zone = w["zone"].as_str().unwrap();
        let start: DateTime<Utc> = w["start"].as_str().unwrap().parse().unwrap();
        let want: Vec<i64> = w["fires"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        let crons = vec![cron.to_string()];
        let mut got = Vec::new();
        let mut t = start;
        for _ in 0..want.len() {
            let Some(next) = next_slot_after(&crons, zone, t) else {
                break;
            };
            got.push(next.timestamp());
            t = next;
        }
        if got != want {
            failures.push(format!(
                "{cron} in {zone} from {start}: got {got:?}, Go {want:?}"
            ));
            continue;
        }
        if let Some(last) = want.last() {
            let last = DateTime::from_timestamp(*last, 0).unwrap();
            assert_eq!(
                latest_slot_in_window(&crons, zone, start, last),
                Some(last),
                "{cron} in {zone} from {start}"
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} walks differ:\n{}",
        failures.len(),
        walks.len(),
        failures.join("\n")
    );
}

#[test]
fn the_poller_reads_what_gos_parser_reads() {
    let golden = golden();
    for p in golden["parse"].as_array().unwrap() {
        let cron = p["cron"].as_str().unwrap();
        match (cron.parse::<CronSpec>(), p["error"].as_str()) {
            (Ok(_), None) => {}
            (Err(e), Some(go)) => assert!(
                go.starts_with(&e.to_string()),
                "{cron:?}: Rust '{e}', Go '{go}'"
            ),
            (got, go) => panic!("{cron:?}: Rust {got:?}, Go {go:?}"),
        }
    }
}

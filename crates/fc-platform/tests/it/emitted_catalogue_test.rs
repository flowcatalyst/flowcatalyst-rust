//! Every event type a platform use case emits is in the seeded catalogue, so
//! a subscription can pick it (Go `TestEveryEmittedEventTypeIsCatalogued`,
//! `25c3ed6`). The client events were catalogued under codes nothing emits
//! (`platform:iam:client:*`) while the use cases emit
//! `platform:admin:client:*`; nothing caught it.
//!
//! The emitted codes are read from the source: in every platform file that
//! defines domain events (an `events.rs`, or a file with
//! `impl_domain_event!` or an `EVENT_TYPE` constant), outside its test
//! module, each `"platform:<a>:<b>:<c>"` literal that is an `EVENT_TYPE` or
//! other `&str` constant, a literal on a line of its own (an event macro's
//! argument), or any literal of an `events.rs`. Known gaps are listed; the
//! list may only shrink.

use std::collections::BTreeSet;
use std::fs;

use crate::support::sources;
use fc_platform::seed::platform_event_types;

/// Emitted, not catalogued: no subscription can pick these. Empty since the
/// 33 found 2026-10-11 were catalogued (as Go's F9, `8ddaed4`;
/// docs/go-mirror/2026-10-11-event-catalogue.md); keep it empty: catalogue a
/// new emitted code rather than listing it here. A code leaving this list
/// (now catalogued, or no longer emitted) fails the test until it is removed
/// here.
const KNOWN_UNCATALOGUED: &[&str] = &[];

fn platform_literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("\"platform:") {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        let lit = &after[..end];
        let segments: Vec<&str> = lit.split(':').collect();
        if segments.len() == 4
            && segments.iter().all(|s| {
                !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            })
        {
            out.push(lit.to_string());
        }
        rest = &after[end + 1..];
    }
    out
}

fn emitted_codes() -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    for path in sources::rs_files() {
        let rel = sources::strip_src(&path)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("seed/") || rel.contains("split_tests/") || rel.ends_with("tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source");
        let is_events_file = rel.ends_with("/events.rs");
        if !is_events_file
            && !text.contains("impl_domain_event!")
            && !text.contains("EVENT_TYPE: &")
        {
            continue;
        }
        for line in text.lines() {
            if line.starts_with("#[cfg(test)]") {
                break;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let is_const = trimmed.contains("const ") && trimmed.contains("str = \"");
            let is_lone = trimmed.starts_with("\"platform:");
            if is_events_file || is_const || is_lone {
                codes.extend(platform_literals(trimmed));
            }
        }
    }
    codes
}

#[test]
fn every_emitted_event_type_is_catalogued() {
    let emitted = emitted_codes();
    // The scan works: the client events, the case Go's test was written for.
    for code in [
        "platform:admin:client:created",
        "platform:admin:client:deactivated",
        "platform:iam:client:applications-updated",
    ] {
        assert!(emitted.contains(code), "the scan missed {code}");
    }

    let catalogued: BTreeSet<String> = platform_event_types::definitions()
        .into_iter()
        .map(|d| d.code)
        .collect();
    let known: BTreeSet<String> = KNOWN_UNCATALOGUED.iter().map(|s| s.to_string()).collect();

    let missing: Vec<&String> = emitted
        .iter()
        .filter(|c| !catalogued.contains(*c) && !known.contains(*c))
        .collect();
    assert!(
        missing.is_empty(),
        "emitted event types missing from the seeded catalogue (no subscription can pick them): {missing:#?}"
    );

    let stale: Vec<&String> = known
        .iter()
        .filter(|c| catalogued.contains(*c) || !emitted.contains(*c))
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_UNCATALOGUED lists codes now catalogued or no longer emitted; remove them: {stale:#?}"
    );
}

/// The catalogue no longer lists the client codes nothing emits.
#[test]
fn client_events_are_catalogued_under_the_emitted_codes() {
    let catalogued: BTreeSet<String> = platform_event_types::definitions()
        .into_iter()
        .map(|d| d.code)
        .collect();
    for ev in [
        "created",
        "updated",
        "activated",
        "suspended",
        "deactivated",
        "deleted",
        "note-added",
    ] {
        assert!(catalogued.contains(&format!("platform:admin:client:{ev}")));
        assert!(!catalogued.contains(&format!("platform:iam:client:{ev}")));
    }
    assert!(catalogued.contains("platform:iam:client:applications-updated"));
}

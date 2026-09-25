//! The rules a publish applies beyond the manifest, against what Java's own
//! code answers (`tests/data/function/publish-rules-golden.json`, written
//! by `tests/java/…/PublishRulesGoldenGen.java` from the pinned sources):
//! the cron grammar behind `CRON_INVALID`, `ZoneId.of` behind
//! `TIMEZONE_INVALID`, `PlatformArtifactRef.parse` and
//! `SignaturesMode.parse`.

use std::path::Path;

use serde_json::Value;

use fc_function_signing::SignaturesMode;
use fc_platform::function::artifact::PlatformArtifactRef;
use fc_platform::function::schedule_check::{parse_cron, zone_id_valid};
use fc_platform::scheduled_job::cron::JobZone;

fn golden() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/function/publish-rules-golden.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn table(name: &str) -> Vec<(String, Value)> {
    let golden = golden();
    let cases = golden[name].as_object().unwrap();
    assert!(!cases.is_empty(), "{name} has cases");
    cases.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

#[test]
fn cron_expressions_parse_as_java_parses_them() {
    for (text, want) in table("cron") {
        let got = match parse_cron(&text) {
            Ok(()) => "OK".to_string(),
            Err((code, message)) => format!("{code} {message}"),
        };
        assert_eq!(got, want.as_str().unwrap(), "{text:?}");
    }
}

/// As Java accepts them, except the legacy `SystemV/*` ids: Java accepts
/// them, the scheduler's tz database cannot evaluate them, and the owner
/// ruled them `TIMEZONE_INVALID` (decision 2 of 2026-09-25). Every id that
/// is accepted is one the scheduler evaluates.
#[test]
fn zone_ids_are_accepted_as_java_accepts_them() {
    let mut refused_unlike_java = Vec::new();
    for (id, want) in table("zone") {
        let got = zone_id_valid(&id);
        if want.as_bool().unwrap() && !got {
            refused_unlike_java.push(id.clone());
        } else {
            assert_eq!(got, want.as_bool().unwrap(), "{id:?}");
        }
        if got {
            assert!(JobZone::parse(&id).is_some(), "{id:?} is not evaluable");
        }
    }
    refused_unlike_java.sort();
    assert_eq!(refused_unlike_java.len(), 13, "{refused_unlike_java:?}");
    for id in &refused_unlike_java {
        assert!(id.starts_with("SystemV/"), "{id:?}");
        assert!(JobZone::parse(id).is_none(), "{id:?}");
    }
}

#[test]
fn platform_refs_parse_as_java_parses_them() {
    for (reference, want) in table("platformRef") {
        let got = PlatformArtifactRef::parse(&reference)
            .map(|r| format!("{} {}", r.function_id, r.hex))
            .unwrap_or_else(|| "EMPTY".into());
        assert_eq!(got, want.as_str().unwrap(), "{reference:?}");
    }
}

#[test]
fn signatures_mode_parses_as_java_parses_it() {
    for (raw, want) in table("signaturesMode") {
        let got = match SignaturesMode::parse(&raw) {
            SignaturesMode::Required => "REQUIRED",
            SignaturesMode::Off => "OFF",
        };
        assert_eq!(got, want.as_str().unwrap(), "{raw:?}");
    }
}

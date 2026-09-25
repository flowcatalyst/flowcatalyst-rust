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

/// Java's answers, with owner decision 5's deviations applied: one code,
/// `CRON_INVALID`, for every refusal (Java's `INVALID_CRON` and
/// `CRON_INVALID_SHAPE` are internal, it publishes `CRON_INVALID`), and a
/// five-field cron accepted (seconds 0), so the shape message names both
/// counts.
#[test]
fn cron_expressions_parse_as_java_parses_them() {
    let mut five_fields = 0;
    for (text, want) in table("cron") {
        let got = match parse_cron(&text) {
            Ok(()) => "OK".to_string(),
            Err((code, message)) => format!("{code} {message}"),
        };
        let fields = text.split_whitespace().count();
        let want = want.as_str().unwrap();
        let want = if fields == 5 && !text.trim_start().starts_with('@') {
            five_fields += 1;
            "OK".to_string()
        } else {
            want.replacen("INVALID_CRON ", "CRON_INVALID ", 1)
                .replacen("CRON_INVALID_SHAPE ", "CRON_INVALID ", 1)
                .replace(
                    "must have 6 whitespace-separated fields (sec min hour dom mon dow)",
                    "must have 5 or 6 whitespace-separated fields ([sec] min hour dom mon dow)",
                )
        };
        assert_eq!(got, want, "{text:?}");
    }
    assert_eq!(five_fields, 1, "Java's table has one five-field cron");
}

#[test]
fn zone_ids_are_accepted_as_java_accepts_them() {
    for (id, want) in table("zone") {
        assert_eq!(zone_id_valid(&id), want.as_bool().unwrap(), "{id:?}");
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

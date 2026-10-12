//! An event's persisted `data` is its own fields only, never the envelope
//! (Go's `ToDataJSON`; `fc_platform_core::usecase::DomainEvent`): every
//! event struct's `metadata: EventMetadata` field is `#[serde(skip)]`.
//!
//! Fourteen events had it `#[serde(flatten)]` instead, so their `data`
//! carried `event_id`, `event_type`, `source`, `subject`, `time`, … beside
//! the payload (and a payload `source` collided with the envelope's): the
//! portal events, the developer-credential events, the dispatch-job
//! actions, the connection sync, the email-domain-mapping provider move,
//! the app-docs sync and the service-account token mint.

use std::fs;

use crate::support::sources;

#[test]
fn every_event_metadata_field_is_skipped() {
    let mut fields = 0;
    let mut offenders = Vec::new();
    for path in sources::rs_files() {
        let rel = sources::strip_src(&path)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&path).expect("read source");
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if t != "pub metadata: EventMetadata," && t != "metadata: EventMetadata," {
                continue;
            }
            // `RecordedEvent` serializes its rendered data by hand.
            if rel == "usecase/domain_event.rs"
                && lines[..i]
                    .iter()
                    .rev()
                    .take(3)
                    .any(|l| l.contains("pub struct RecordedEvent"))
            {
                continue;
            }
            fields += 1;
            let attrs: Vec<&str> = lines[..i]
                .iter()
                .rev()
                .map(|l| l.trim())
                .take_while(|l| l.starts_with("#[") || l.starts_with("///") || l.starts_with("//"))
                .collect();
            if !attrs.contains(&"#[serde(skip)]") {
                offenders.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    assert!(
        fields > 50,
        "the scan found only {fields} event metadata fields"
    );
    assert!(
        offenders.is_empty(),
        "event metadata not #[serde(skip)] (the envelope would land in `data`): {offenders:#?}"
    );
}

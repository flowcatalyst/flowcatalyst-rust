//! Agreement with Java. Every file under `tests/data/java-golden/` was
//! written by running Java's own code at `0118cdca`
//! (`tests/java/io/flowcatalyst/function/WebhookGolden.java`, which calls
//! `Webhook` and `Result.fail`). These tests feed the same inputs to this
//! crate and require the same answers back.

use std::path::{Path, PathBuf};

use fc_function_abi::{Response, Webhook};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/java-golden")
        .join(name)
}

/// Rows of a generated table: `#` lines skipped, fields split on tabs.
fn table(name: &str) -> Vec<Vec<String>> {
    std::fs::read_to_string(data(name))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

/// Standard padded base64, for reading the tables.
fn b64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut bits = 0u32;
    let mut n = 0;
    for b in text.bytes().filter(|&b| b != b'=') {
        bits = (bits << 6) | ALPHABET.iter().position(|&a| a == b).unwrap() as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    out
}

/// A table string field: `~` is Java's `null`, anything else base64 UTF-8.
fn opt(field: &str) -> Option<String> {
    (field != "~").then(|| String::from_utf8(b64(field)).unwrap())
}

#[test]
fn webhook_event_agrees_with_java_on_every_row() {
    let rows = table("webhook-event.tsv");
    assert!(rows.len() > 70);
    for row in rows {
        let (name, body) = (&row[0], b64(&row[1]));
        match (row[2].as_str(), Webhook::event(&body)) {
            ("OK", Ok(e)) => {
                let f: Vec<&str> = row[3].split(',').collect();
                assert_eq!(Some(e.id), opt(f[0]), "{name}: id");
                assert_eq!(Some(e.event_type), opt(f[1]), "{name}: type");
                assert_eq!(e.attempt_number.to_string(), f[2], "{name}: attemptNumber");
                assert_eq!(e.source, opt(f[3]), "{name}: source");
                assert_eq!(e.subject, opt(f[4]), "{name}: subject");
                assert_eq!(e.correlation_id, opt(f[5]), "{name}: correlationId");
                assert_eq!(e.message_group, opt(f[6]), "{name}: messageGroup");
                assert_eq!(e.client_id, opt(f[7]), "{name}: clientId");
                assert_eq!(e.client_code, opt(f[8]), "{name}: clientCode");
                assert_eq!(e.data_json, opt(f[9]), "{name}: data");
            }
            ("ERR", Err(e)) => assert_eq!(e.to_string(), row[3], "{name}: message"),
            (java, ours) => panic!("{name}: Java {java}, Rust {ours:?}"),
        }
    }
}

fn instant(t: Option<fc_function_abi::Timestamp>) -> String {
    t.map_or("~".into(), |t| format!("{}.{}", t.epoch_second(), t.nano()))
}

#[test]
fn webhook_schedule_agrees_with_java_on_every_row() {
    let rows = table("webhook-schedule.tsv");
    assert!(rows.len() > 70);
    for row in rows {
        let (name, body) = (&row[0], b64(&row[1]));
        match (row[2].as_str(), Webhook::schedule(&body)) {
            ("OK", Ok(s)) => {
                let f: Vec<&str> = row[3].split(',').collect();
                assert_eq!(Some(s.job_id), opt(f[0]), "{name}: jobId");
                assert_eq!(Some(s.job_code), opt(f[1]), "{name}: jobCode");
                assert_eq!(Some(s.instance_id), opt(f[2]), "{name}: instanceId");
                assert_eq!(instant(s.scheduled_for), f[3], "{name}: scheduledFor");
                assert_eq!(instant(Some(s.fired_at)), f[4], "{name}: firedAt");
                assert_eq!(Some(s.trigger_kind), opt(f[5]), "{name}: triggerKind");
                assert_eq!(s.correlation_id, opt(f[6]), "{name}: correlationId");
                assert_eq!(s.payload_json, opt(f[7]), "{name}: payload");
                assert_eq!(
                    s.tracks_completion.to_string(),
                    f[8],
                    "{name}: tracksCompletion"
                );
                assert_eq!(
                    s.timeout_seconds.map_or("~".into(), |t| t.to_string()),
                    f[9],
                    "{name}: timeoutSeconds"
                );
                assert_eq!(s.concurrent.to_string(), f[10], "{name}: concurrent");
            }
            ("ERR", Err(e)) => assert_eq!(e.to_string(), row[3], "{name}: message"),
            (java, ours) => panic!("{name}: Java {java}, Rust {ours:?}"),
        }
    }
}

#[test]
fn result_fail_body_is_byte_identical_to_java() {
    for row in table("result-fail.tsv") {
        let reason = String::from_utf8(b64(&row[0])).unwrap();
        let r = Response::fail(&reason).unwrap();
        assert_eq!(r.status().to_string(), row[1], "{reason:?}");
        assert_eq!(r.body(), b64(&row[2]), "{reason:?}");
    }
}

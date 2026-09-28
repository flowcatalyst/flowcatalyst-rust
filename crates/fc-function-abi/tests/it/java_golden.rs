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

/// Java's answer or ours for one row: the decoded fields, or an error (its
/// message is not compared: it is advisory).
type Answer = Result<Vec<Option<String>>, ()>;

/// Java's answer from a table row: `OK` and the fields, or `ERR`. String
/// fields are base64 (`~` is `null`); the rest are compared as written.
fn java_answer(row: &[String], text_fields: &[usize]) -> Answer {
    match row[2].as_str() {
        "OK" => Ok(row[3]
            .split(',')
            .enumerate()
            .map(|(i, f)| {
                if text_fields.contains(&i) {
                    opt(f)
                } else {
                    Some(f.to_string())
                }
            })
            .collect()),
        "ERR" => Err(()),
        other => panic!("unknown outcome {other}"),
    }
}

/// Every row where our answer differs from Java's, by name, sorted.
fn disagreements(file: &str, text_fields: &[usize], ours: impl Fn(&[u8]) -> Answer) -> Vec<String> {
    let rows = table(file);
    assert!(rows.len() > 70);
    let mut names: Vec<String> = rows
        .iter()
        .filter(|row| ours(&b64(&row[1])) != java_answer(row, text_fields))
        .map(|row| row[0].clone())
        .collect();
    names.sort();
    names
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    names.sort();
    names
}

/// Malformed bodies Java's hand-written reader answers differently from
/// serde. Neither platform sends any of them.
const EVENT_DEVIATIONS: &[&str] = &[
    // `-0` is not an integer to serde_json (Java: 0).
    "attempt negative zero",
    // A repeated unknown key is ignored (Java refuses any duplicate; a
    // repeated known key is still refused).
    "duplicate unknown key",
    // An escaped lone surrogate is refused (Java: `?`).
    "lone surrogate in id",
    // serde_json nests to 128 (Java: 64).
    "depth 66 in data",
    // Invalid UTF-8 is refused, as RFC 8259 requires (Java decodes it
    // lossily, to U+FFFD).
    "invalid utf-8 in id",
    "truncated utf-8 in id",
    "overlong utf-8 in id",
    "utf-8 c0 af",
    "utf-8 c1 bf",
    "utf-8 c2",
    "utf-8 c2 78",
    "utf-8 e0 80 80",
    "utf-8 e0 9f bf",
    "utf-8 ed a0 80",
    "utf-8 ed bf bf",
    "utf-8 e2 78",
    "utf-8 e2 82 78",
    "utf-8 f0 80 80 80",
    "utf-8 f0 8f bf bf",
    "utf-8 f4 90 80 80",
    "utf-8 f5 80 80 80",
    "utf-8 f7 bf bf bf",
    "utf-8 f8 88 80 80 80",
    "utf-8 f0 9f 78",
    "utf-8 f0 9f 98 78",
    "utf-8 f0 78",
    "utf-8 f4 90 78",
    "utf-8 80 bf",
    "utf-8 fe ff",
];

#[test]
fn webhook_event_agrees_with_java_on_every_well_formed_row() {
    let ours = |body: &[u8]| -> Answer {
        let e = Webhook::event(body).map_err(|_| ())?;
        Ok(vec![
            Some(e.id),
            Some(e.event_type),
            Some(e.attempt_number.to_string()),
            e.source,
            e.subject,
            e.correlation_id,
            e.message_group,
            e.client_id,
            e.client_code,
            e.data_json,
        ])
    };
    let text = [0, 1, 3, 4, 5, 6, 7, 8, 9];
    assert_eq!(
        disagreements("webhook-event.tsv", &text, ours),
        sorted(EVENT_DEVIATIONS)
    );
}

fn instant(t: Option<fc_function_abi::Timestamp>) -> Option<String> {
    Some(t.map_or("~".into(), |t| format!("{}.{}", t.epoch_second(), t.nano())))
}

/// As [`EVENT_DEVIATIONS`], for the schedule envelope: timestamps are RFC
/// 3339 (chrono) rather than `Instant.parse`'s grammar. Both platforms send
/// `yyyy-MM-ddTHH:mm:ss[.fraction]Z`, which both read the same.
const SCHEDULE_DEVIATIONS: &[&str] = &[
    // More than nine fraction digits accepted (and truncated); `.` with no
    // digits refused.
    "ts 2026-09-19T00:00:00.1234567890Z",
    "ts 2026-09-19T00:00:00.Z",
    // Offsets: no seconds; any hour up to 23 (Java: 18).
    "ts 2026-09-19T00:00:00+01:00:30",
    "ts 2026-09-19T00:00:00+19:00",
    "ts 2026-09-19T00:00:00+00:00:00",
    "ts 2026-09-19T00:00:00+17:59:59",
    // A space for the `T` is accepted.
    "ts 2026-09-19 00:00:00Z",
    // No `24:00:00`; a leap second at any minute is accepted (Java: only
    // at 23:59).
    "ts 2026-09-19T24:00:00Z",
    "ts 2026-12-31T24:00:00Z",
    "ts 2026-09-19T12:59:60Z",
    // Four-digit years only.
    "ts +12026-09-19T00:00:00Z",
    "ts -0001-01-01T00:00:00Z",
    "ts -2026-09-19T00:00:00Z",
    "ts +999999999-12-31T23:59:59Z",
    "ts +1000000000-01-01T00:00:00Z",
    "ts -999999999-01-01T00:00:00Z",
];

#[test]
fn webhook_schedule_agrees_with_java_on_every_well_formed_row() {
    let ours = |body: &[u8]| -> Answer {
        let s = Webhook::schedule(body).map_err(|_| ())?;
        Ok(vec![
            Some(s.job_id),
            Some(s.job_code),
            Some(s.instance_id),
            instant(s.scheduled_for),
            instant(Some(s.fired_at)),
            Some(s.trigger_kind),
            s.correlation_id,
            s.payload_json,
            Some(s.tracks_completion.to_string()),
            Some(s.timeout_seconds.map_or("~".into(), |t| t.to_string())),
            Some(s.concurrent.to_string()),
        ])
    };
    let text = [0, 1, 2, 5, 6, 7];
    assert_eq!(
        disagreements("webhook-schedule.tsv", &text, ours),
        sorted(SCHEDULE_DEVIATIONS)
    );
}

#[test]
fn result_fail_body_is_javas_json() {
    for row in table("result-fail.tsv") {
        let reason = String::from_utf8(b64(&row[0])).unwrap();
        let r = Response::fail(&reason).unwrap();
        assert_eq!(r.status().to_string(), row[1], "{reason:?}");
        let ours: serde_json::Value = serde_json::from_slice(r.body()).unwrap();
        let java: serde_json::Value = serde_json::from_slice(&b64(&row[2])).unwrap();
        assert_eq!(ours, java, "{reason:?}");
    }
}

//! Byte-level agreement with Java. Every file under `tests/data/java-golden/`
//! was written by running Java's own code at `0118cdca`
//! (`tests/java/io/flowcatalyst/fnhost/wasm/GoldenGen.java`, which calls
//! `WasmAbi.encode` / `WasmAbi.decode`, `HostFunctions.emit`, `Webhook` and
//! `Result.fail`). These tests feed the same inputs to this crate and require
//! the same bytes back.

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

#[cfg(feature = "extism-abi")]
mod extism {
    use super::*;
    use fc_function_abi::extism_abi::{
        decode_emit_input, decode_reply, encode_emit_answer, Request,
    };
    use fc_function_abi::{
        emit_error, Caller, EventEmitError, FunctionAddress, MultiMap, Principal,
    };
    use indexmap::IndexMap;

    fn multi(entries: &[(&str, &[&str])]) -> MultiMap {
        entries
            .iter()
            .map(|(k, vs)| (k.to_string(), vs.iter().map(|v| v.to_string()).collect()))
            .collect()
    }

    fn strings(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    fn minimal(caller: Caller) -> Request {
        Request {
            address: FunctionAddress::new("a", "s", "n").unwrap(),
            version: 3,
            invocation_id: "inv".into(),
            method: "GET".into(),
            path: "/p".into(),
            original_host: None,
            original_path: None,
            path_params: IndexMap::new(),
            query: MultiMap::new(),
            headers: MultiMap::new(),
            body: vec![],
            remote_address: None,
            caller,
        }
    }

    /// The requests `GoldenGen.encodeFixtures` builds, field for field.
    fn fixtures() -> Vec<(&'static str, Request)> {
        let full = Request {
            address: FunctionAddress::parse("billing.invoices.api").unwrap(),
            version: 12,
            invocation_id: "0HZXEQ5Y8JY5Z".into(),
            method: "POST".into(),
            path: "/orders/42".into(),
            original_host: Some("api.acme.com".into()),
            original_path: Some("/v1/orders/42".into()),
            path_params: [("orderId", "42"), ("z", "last"), ("a", "first")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            query: multi(&[("q", &["a b", "c"]), ("empty", &[]), ("dup", &["1", "1"])]),
            headers: multi(&[
                ("Content-Type", &["application/json"]),
                ("X-Multi", &["1", "2"]),
                ("accept", &["*/*"]),
            ]),
            body: vec![0x00, 0xFF, 0x10, b'h', b'i'],
            remote_address: Some("10.0.0.1".into()),
            caller: Caller::Principal(Principal {
                id: "prn_0HZ".into(),
                principal_type: "service-account".into(),
                tier: None,
                clients: strings(&["clt_1"]),
                roles: strings(&[]),
                applications: strings(&["app_1", "app_2"]),
                all_applications: true,
                permissions: strings(&[
                    "platform:*:event-type:view",
                    "a:b:c:d",
                    "*:*:*:*",
                    "Z:upper",
                    "\u{e9}clair:x",
                ])
                .into_iter()
                .collect(),
            }),
        };
        let escaping = Request {
            address: FunctionAddress::parse("b-1.inv-2.c-3").unwrap(),
            version: i32::MAX,
            invocation_id: String::new(),
            method: String::new(),
            path: "/a\"b\\c/d\u{1}\u{1f}\u{8}\t\n\u{c}\r\u{7f} \u{e9}\u{1F600}\u{2028}\u{2029}<>&'"
                .into(),
            original_host: Some("h\u{e9}llo.example".into()),
            original_path: Some(String::new()),
            path_params: [("p".to_string(), "\u{a0}".to_string())]
                .into_iter()
                .collect(),
            query: multi(&[("k/ey", &["/"])]),
            headers: multi(&[("X-Ctl\u{0}", &["v\u{1b}"])]),
            body: vec![0x41],
            remote_address: Some("::1".into()),
            caller: Caller::Anonymous,
        };
        let sorting = Request {
            address: FunctionAddress::new("a", "s", "n").unwrap(),
            version: 0,
            invocation_id: "inv".into(),
            method: "DELETE".into(),
            path: "/".into(),
            original_host: None,
            original_path: Some("/x".into()),
            path_params: IndexMap::new(),
            query: MultiMap::new(),
            headers: MultiMap::new(),
            body: vec![1, 2],
            remote_address: Some("127.0.0.1".into()),
            caller: Caller::Principal(Principal {
                id: "id\"1".into(),
                principal_type: "user".into(),
                tier: Some("ANCHOR".into()),
                clients: strings(&["*"]),
                roles: strings(&["r\"1", "r/2"]),
                applications: strings(&[]),
                all_applications: false,
                permissions: strings(&[
                    "b",
                    "a",
                    "B",
                    "\u{e9}",
                    "\u{1F600}",
                    "\u{E000}",
                    "\u{FFFD}",
                    "a:b",
                    "a",
                ])
                .into_iter()
                .collect(),
            }),
        };
        vec![
            ("platform-minimal", minimal(Caller::Platform)),
            ("anonymous-minimal", minimal(Caller::Anonymous)),
            (
                "principal-wasmabitest",
                minimal(Caller::Principal(Principal {
                    id: "prn_1".into(),
                    principal_type: "SERVICE".into(),
                    tier: Some("CLIENT".into()),
                    clients: strings(&["clt_1", "clt_2"]),
                    roles: strings(&["role-a"]),
                    applications: strings(&["app_1"]),
                    all_applications: false,
                    permissions: strings(&["b:perm", "a:perm"]).into_iter().collect(),
                })),
            ),
            ("full", full),
            ("escaping", escaping),
            ("principal-sorting", sorting),
        ]
    }

    #[test]
    fn request_encoding_is_byte_identical_to_java() {
        let fixtures = fixtures();
        let files = std::fs::read_dir(data("encode")).unwrap().count();
        assert_eq!(fixtures.len(), files, "one fixture per golden file");
        for (name, request) in fixtures {
            let java = std::fs::read(data(&format!("encode/{name}.json"))).unwrap();
            let ours = request.encode();
            assert!(
                ours == java,
                "{name}:\n rust {}\n java {}",
                String::from_utf8_lossy(&ours),
                String::from_utf8_lossy(&java)
            );
            assert_eq!(
                Request::decode(&java).unwrap(),
                request,
                "{name} decodes back"
            );
        }
    }

    fn headers_field(field: &str) -> Vec<(String, Vec<String>)> {
        if field == "-" {
            return vec![];
        }
        field
            .split(';')
            .map(|entry| {
                // The key is padded base64, so its length is a multiple of 4
                // and padding never sits at such an index: the separator does.
                let sep = (0..entry.len())
                    .step_by(4)
                    .find(|&i| entry.as_bytes()[i] == b'=')
                    .unwrap();
                let (k, vs) = (&entry[..sep], &entry[sep + 1..]);
                let key = String::from_utf8(b64(k)).unwrap();
                let values = if vs.is_empty() {
                    vec![]
                } else {
                    vs.split(',')
                        .map(|v| String::from_utf8(b64(v)).unwrap())
                        .collect()
                };
                (key, values)
            })
            .collect()
    }

    #[test]
    fn reply_decoding_agrees_with_java_on_every_row() {
        let rows = table("decode.tsv");
        assert!(rows.len() > 100);
        for row in rows {
            let (name, input) = (&row[0], b64(&row[1]));
            match (row[2].as_str(), decode_reply(&input)) {
                ("OK", Ok(reply)) => {
                    assert_eq!(reply.status().to_string(), row[3], "{name}: status");
                    let ours: Vec<_> = reply
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    assert_eq!(ours, headers_field(&row[4]), "{name}: headers");
                    assert_eq!(reply.body(), b64(&row[5]), "{name}: body");
                }
                ("ERR", Err(e)) => assert_eq!(e.detail(), row[3], "{name}: detail"),
                (java, ours) => panic!("{name}: Java {java}, Rust {ours:?}"),
            }
        }
    }

    #[test]
    fn emit_input_and_answer_agree_with_java_on_every_row() {
        let rows = table("emit.tsv");
        assert!(rows.len() > 60);
        for row in rows {
            let (name, mode, input, answer) = (&row[0], &row[1], b64(&row[2]), b64(&row[3]));
            let decoded = decode_emit_input(&input);
            // What Java's host does next: emit, then map the outcome.
            let outcome: Result<(), String> = match (&decoded, mode.as_str()) {
                (Err(code), _) => Err(code.to_string()),
                (Ok(_), "capture") => Ok(()),
                (Ok(_), "refuse") => Err(EventEmitError::new("EVENT_TYPE_NOT_OWNED", 403)
                    .code()
                    .into()),
                (Ok(_), "unavailable") => Err(EventEmitError::unavailable().code().into()),
                (Ok(_), "boom") => Err(emit_error::EMIT_FAILED.into()),
                (Ok(_), other) => panic!("{name}: unknown mode {other}"),
            };
            assert_eq!(
                String::from_utf8(encode_emit_answer(match &outcome {
                    Ok(()) => Ok(()),
                    Err(e) => Err(e.as_str()),
                }))
                .unwrap(),
                String::from_utf8(answer).unwrap(),
                "{name}: answer"
            );
            let captured = row[4].as_str();
            if mode != "capture" || captured == "-" {
                continue;
            }
            let event = decoded.unwrap();
            let f: Vec<&str> = captured.split(',').collect();
            assert_eq!(
                Some(event.event_type().to_string()),
                opt(f[0]),
                "{name}: type"
            );
            assert_eq!(
                event.source().map(str::to_string),
                opt(f[1]),
                "{name}: source"
            );
            assert_eq!(
                event.subject().map(str::to_string),
                opt(f[2]),
                "{name}: subject"
            );
            assert_eq!(
                event.data_content_type().map(str::to_string),
                opt(f[3]),
                "{name}: dataContentType"
            );
            assert_eq!(
                String::from_utf8_lossy(event.data()),
                String::from_utf8_lossy(&b64(f[4])),
                "{name}: data"
            );
            assert_eq!(
                event.correlation_id().map(str::to_string),
                opt(f[5]),
                "{name}: correlationId"
            );
            assert_eq!(
                event.causation_id().map(str::to_string),
                opt(f[6]),
                "{name}: causationId"
            );
            assert_eq!(
                event.message_group().map(str::to_string),
                opt(f[7]),
                "{name}: messageGroup"
            );
            assert_eq!(
                Some(event.dedup_id().to_string()),
                opt(f[8]),
                "{name}: dedupId"
            );
        }
    }
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

//! Agreement with Java's own code. `tests/data/function/manifest-golden.json`
//! was written by running Java's `Manifest.check` / `parseStrict` /
//! `readStored` / `toJson` and the value types' parsers at `0118cdca`
//! (`tests/java/io/flowcatalyst/platform/function/ManifestGoldenGen.java`)
//! over every case in `manifest-cases.json`. These tests feed the same
//! inputs to the Rust port and require the same answers: the same problems
//! (code, message, pointer) in the same order, the same first error, and
//! byte-identical normalised JSON.

use std::path::{Path, PathBuf};

use serde_json::Value;

use fc_platform::function::{
    ClientCeilings, Digest, DnsLabel, EndpointAuth, FunctionAddressPattern, FunctionLimits,
    Hostname, HttpMethod, JsonNode, Manifest, PoolUrlTemplate, RoutePattern, Runtime, Segment,
    SettingKey,
};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/function")
        .join(name)
}

fn load(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(data(name)).unwrap()).unwrap()
}

fn cases() -> Value {
    load("manifest-cases.json")
}

fn golden() -> Value {
    load("manifest-golden.json")
}

fn root(case: &Value) -> Option<JsonNode> {
    case["text"]
        .as_str()
        .map(|text| JsonNode::parse(text).expect("case text is JSON"))
}

fn ceilings(case: &Value) -> ClientCeilings {
    match case["ceilings"].as_array() {
        None => ClientCeilings::of(&FunctionLimits::defaults()),
        Some(c) => {
            let n = |i: usize| c[i].as_i64().unwrap() as i32;
            ClientCeilings::new(n(0), n(1), n(2), n(3)).unwrap()
        }
    }
}

fn problems_of(
    rejected: &fc_platform::function::ManifestRejected,
) -> Vec<(String, String, String)> {
    rejected
        .problems()
        .iter()
        .map(|p| (p.code.to_string(), p.message.clone(), p.pointer.clone()))
        .collect()
}

#[test]
fn golden_covers_every_case() {
    let (cases, golden) = (cases(), golden());
    for section in ["manifest", "stored"] {
        let names = |doc: &Value| -> Vec<String> {
            doc[section]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            names(&cases),
            names(&golden),
            "{section}: regenerate the golden file"
        );
    }
}

#[test]
fn manifest_check_matches_java() {
    let (cases, golden) = (cases(), golden());
    let defaults = FunctionLimits::defaults();
    let mut checked = 0;
    for (case, expected) in cases["manifest"]
        .as_array()
        .unwrap()
        .iter()
        .zip(golden["manifest"].as_array().unwrap())
    {
        let name = case["name"].as_str().unwrap();
        let runtime: Runtime = case["runtime"].as_str().unwrap().parse().unwrap();
        let ceilings = ceilings(case);
        let root = root(case);
        let result = Manifest::check(root.as_ref(), runtime, &defaults, &ceilings);
        match expected["result"].as_str().unwrap() {
            "ok" => {
                let manifest = result.unwrap_or_else(|r| panic!("{name}: {:?}", r.problems()));
                assert_eq!(
                    manifest.to_json().to_json_string(),
                    expected["normalised"].as_str().unwrap(),
                    "{name}: normalised JSON"
                );
                assert!(expected["roundTrip"].as_bool().unwrap(), "{name}");
                assert_eq!(
                    Manifest::read_stored(&manifest.to_json()).unwrap(),
                    manifest,
                    "{name}: read_stored(to_json) round-trips"
                );
            }
            "rejected" => {
                let rejected = match result {
                    Ok(m) => panic!("{name}: accepted {m:?}"),
                    Err(r) => r,
                };
                let want: Vec<(String, String, String)> = expected["problems"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        (
                            p["code"].as_str().unwrap().to_string(),
                            p["message"].as_str().unwrap().to_string(),
                            p["pointer"].as_str().unwrap().to_string(),
                        )
                    })
                    .collect();
                assert_eq!(problems_of(&rejected), want, "{name}: problems");
                // Publish rejects with the first problem, and nothing else.
                let err = Manifest::parse_strict(root.as_ref(), runtime, &defaults, &ceilings)
                    .unwrap_err();
                assert_eq!(
                    err.code(),
                    expected["thrown"]["code"].as_str().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    err.message(),
                    expected["thrown"]["message"].as_str().unwrap(),
                    "{name}"
                );
                assert!(err.details().is_empty(), "{name}");
                if name.starts_with("single/") {
                    assert_eq!(rejected.problems().len(), 1, "{name}: no cascades");
                }
                let first = rejected.problems()[0].to_use_case_error();
                assert_eq!(first.details()["pointer"], want[0].2.as_str(), "{name}");
            }
            "exception" => {
                // Java throws Jackson's JsonNodeException (a 500) for an
                // array or object where `runtime` / `entrypoint` should be
                // a string. Rust reports the field as missing instead.
                let rejected = result.expect_err(name);
                let want = match name {
                    "deviation/runtime-object" => ("RUNTIME_INVALID", "/runtime"),
                    "deviation/entrypoint-array" => ("ENTRYPOINT_REQUIRED", "/entrypoint"),
                    other => panic!("unexpected Java exception for {other}"),
                };
                let got: Vec<_> = rejected
                    .problems()
                    .iter()
                    .map(|p| (p.code, p.pointer.as_str()))
                    .collect();
                assert_eq!(got, [want], "{name}");
            }
            other => panic!("{name}: unknown result {other}"),
        }
        checked += 1;
    }
    assert!(checked > 100, "only {checked} cases");
}

#[test]
fn read_stored_matches_java() {
    let (cases, golden) = (cases(), golden());
    for (case, expected) in cases["stored"]
        .as_array()
        .unwrap()
        .iter()
        .zip(golden["stored"].as_array().unwrap())
    {
        let name = case["name"].as_str().unwrap();
        let root = root(case).unwrap();
        let result = Manifest::read_stored(&root);
        match expected["result"].as_str().unwrap() {
            "ok" => assert_eq!(
                result.unwrap().to_json().to_json_string(),
                expected["normalised"].as_str().unwrap(),
                "{name}"
            ),
            "unreadable" => assert_eq!(
                result.unwrap_err().to_string(),
                expected["message"].as_str().unwrap(),
                "{name}"
            ),
            other => panic!("{name}: unexpected {other}"),
        }
    }
}

/// One value table: `parse` answers the rendered value or `(code, message)`.
fn value_table(section: &str, parse: impl Fn(&str) -> Result<String, (String, String)>) {
    let golden = golden();
    let rows = golden["values"][section].as_array().unwrap();
    assert!(!rows.is_empty(), "{section}");
    for row in rows {
        let input = row["input"].as_str().unwrap();
        let expected = match row.get("ok") {
            Some(ok) => Ok(ok.as_str().unwrap().to_string()),
            None => Err((
                row["code"].as_str().unwrap().to_string(),
                row["message"].as_str().unwrap().to_string(),
            )),
        };
        assert_eq!(parse(input), expected, "{section}: {input:?}");
    }
}

fn use_case<T>(
    result: Result<T, fc_platform::UseCaseError>,
    render: impl Fn(T) -> String,
) -> Result<String, (String, String)> {
    result
        .map(render)
        .map_err(|e| (e.code().to_string(), e.message().to_string()))
}

#[test]
fn value_types_match_java() {
    value_table("dnsLabel", |raw| {
        use_case(DnsLabel::parse("field", raw), |l| l.value().to_string())
    });
    value_table("hostname", |raw| {
        use_case(Hostname::parse(raw), |h| {
            format!("{} {}", h.value(), h.zone_candidates().join(","))
        })
    });
    value_table("digest", |raw| {
        use_case(Digest::parse(raw), |d| d.value().to_string())
    });
    value_table("settingKey", |raw| {
        use_case(SettingKey::parse(raw), |k| k.value().to_string())
    });
    value_table("routePattern", |raw| {
        use_case(RoutePattern::parse(raw), |p| {
            let mut out = format!("{} ", p.value());
            for segment in p.segments() {
                match segment {
                    Segment::Literal(l) => out.push_str(&format!("L:{l}|")),
                    Segment::Param(n) => out.push_str(&format!("P:{n}|")),
                    Segment::Rest => out.push_str("R|"),
                }
            }
            out
        })
    });
    value_table("addressPattern", |raw| {
        use_case(FunctionAddressPattern::parse(raw), |p| {
            let kind = match p {
                FunctionAddressPattern::Exact(_) => "Exact",
                FunctionAddressPattern::Service { .. } => "Service",
                FunctionAddressPattern::Application(_) => "Application",
            };
            format!("{kind} {}", p.render())
        })
    });
    value_table("poolUrl", |raw| {
        PoolUrlTemplate::parse(raw)
            .map(|t| {
                let orders = DnsLabel::parse("pool", "orders").unwrap();
                format!("{} {}", t.template(), t.resolve(&orders))
            })
            .map_err(|e| ("IllegalStateException".to_string(), e.to_string()))
    });
    value_table("runtime", |raw| {
        use_case(Runtime::parse_strict(raw), |r| r.as_str().to_string())
    });
    value_table("httpMethod", |raw| {
        use_case(HttpMethod::parse_strict(raw), |m| m.as_str().to_string())
    });
    value_table("endpointAuth", |raw| {
        use_case(EndpointAuth::parse_strict(raw), |a| a.as_str().to_string())
    });
}

#[test]
fn route_matching_matches_java() {
    for row in golden()["matches"].as_array().unwrap() {
        let pattern = RoutePattern::parse(row["pattern"].as_str().unwrap()).unwrap();
        let path = row["path"].as_str().unwrap();
        let got = pattern.matches(path).map(|params| {
            params
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect::<serde_json::Map<_, _>>()
        });
        let want = row["params"].as_object().cloned();
        assert_eq!(got, want, "{} vs {path}", pattern.value());
    }
}

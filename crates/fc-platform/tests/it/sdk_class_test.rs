//! The `x-sdk` classification of the published document (Go commit `21f9734d`,
//! `tools/dump-spec/sdkclass_test.go`): every operation has exactly one class,
//! decided by the lists in `shared/sdk_class.rs`; the class of an operation both
//! platforms document is Go's; and no list names an operation that exists in
//! neither document. No database.

use std::collections::{BTreeMap, BTreeSet};

use fc_platform::shared::sdk_class::{self, APP_OPS, EXTENSION, INTERNAL_OPS};
use serde_json::Value;

use crate::openapi_go_contract_test::{go_document, normalise_path, rust_document, METHODS};

/// `(METHOD, path as {}) -> (operationId, x-sdk class or "")`.
fn classes(doc: &Value) -> BTreeMap<(String, String), (String, String)> {
    let mut out = BTreeMap::new();
    for (path, item) in doc["paths"].as_object().into_iter().flatten() {
        for method in METHODS {
            if let Some(op) = item.get(*method) {
                out.insert(
                    (method.to_uppercase(), normalise_path(path)),
                    (
                        op["operationId"].as_str().unwrap_or_default().to_string(),
                        op[EXTENSION].as_str().unwrap_or_default().to_string(),
                    ),
                );
            }
        }
    }
    out
}

fn operation_ids(doc: &Value) -> BTreeSet<String> {
    classes(doc).into_values().map(|(id, _)| id).collect()
}

/// Classification is always a decision: no operation of the published
/// document is left to a default.
#[tokio::test]
async fn every_operation_is_classified() {
    let mut doc = rust_document();
    let undecided = sdk_class::classify(&mut doc);
    assert!(undecided.is_empty(), "undecided operations: {undecided:#?}");
}

/// The class of every operation both platforms document is Go's: the SDK
/// generators split the document by it.
#[tokio::test]
async fn shared_operations_have_gos_class() {
    let rust = classes(&rust_document());
    let go = classes(&go_document());
    let mut differing = Vec::new();
    let mut compared = 0;
    for (key, (rust_id, rust_class)) in &rust {
        let Some((go_id, go_class)) = go.get(key) else {
            continue;
        };
        compared += 1;
        if rust_class != go_class {
            differing.push(format!(
                "{} {}: rust {rust_id} is {rust_class:?}, go {go_id} is {go_class:?}",
                key.0, key.1
            ));
        }
    }
    assert!(compared > 250, "only {compared} shared operations compared");
    assert!(
        differing.is_empty(),
        "classes differ from Go's:\n{}",
        differing.join("\n")
    );
}

/// A listed operationId must name an operation: in this document, or in Go's
/// (the plain-handler operations Go documents and this platform's document
/// does not yet: the `go-only` lines of the gap baseline). A typo is neither.
#[tokio::test]
async fn listed_operation_ids_exist_somewhere() {
    let rust = operation_ids(&rust_document());
    let go = operation_ids(&go_document());
    let unknown: Vec<&str> = APP_OPS
        .iter()
        .chain(INTERNAL_OPS)
        .copied()
        .filter(|id| !rust.contains(*id) && !go.contains(*id))
        .collect();
    assert!(unknown.is_empty(), "named in no document: {unknown:?}");
}

/// The extension reaches the served document, not only the helper.
#[tokio::test]
async fn the_published_document_carries_the_extension() {
    let classes = classes(&rust_document());
    let count = |c: &str| classes.values().filter(|(_, class)| class == c).count();
    assert!(count("app") > 50, "app operations: {}", count("app"));
    assert!(count("admin") > 150, "admin operations: {}", count("admin"));
    assert_eq!(
        classes[&("POST".to_string(), "/api/events/batch".to_string())].1,
        "app"
    );
    assert_eq!(
        classes[&("GET".to_string(), "/api/roles".to_string())].1,
        "app"
    );
}

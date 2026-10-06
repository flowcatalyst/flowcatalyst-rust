//! Diff two OpenAPI documents into structured `ChangeNotes` plus a
//! human-readable summary.
//!
//! v1 is deliberately shallow: it set-diffs `paths` keys, `components.schemas`
//! keys, and the HTTP verbs of each surviving path. Anything removed is
//! flagged as `has_breaking`. Future versions can drill into request/response
//! schema shapes, but v1 surfaces the change-notes the user explicitly asked
//! for ("notes about what was removed") without pulling in a 3rd-party
//! OpenAPI parser.

use std::collections::BTreeSet;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::entity::ChangeNotes;

/// HTTP methods that appear as object keys under each path in OpenAPI.
const VERBS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// Stable, canonical-JSON hash of a spec. Used for the no-op short-circuit so
/// re-sending the same document doesn't create a new row.
#[expect(
    clippy::let_underscore_must_use,
    reason = "writing to a String cannot fail"
)]
pub fn spec_hash(spec: &Value) -> String {
    let canonical = canonicalize(spec);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    digest.iter().fold(String::with_capacity(64), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{:02x}", b);
        acc
    })
}

/// Recursively sort object keys so equivalent documents hash identically.
#[allow(clippy::wildcard_enum_match_arm)] // serde_json::Value is foreign: scalars copy as they are
fn canonicalize(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

/// Compute the structured diff and a pre-rendered human summary.
pub fn compute_change_notes(prior: &Value, current: &Value) -> (ChangeNotes, String) {
    let prior_paths = collect_object_keys(prior.pointer("/paths"));
    let current_paths = collect_object_keys(current.pointer("/paths"));
    let removed_paths: Vec<String> = prior_paths.difference(&current_paths).cloned().collect();
    let added_paths: Vec<String> = current_paths.difference(&prior_paths).cloned().collect();

    let prior_schemas = collect_object_keys(prior.pointer("/components/schemas"));
    let current_schemas = collect_object_keys(current.pointer("/components/schemas"));
    let removed_schemas: Vec<String> = prior_schemas
        .difference(&current_schemas)
        .cloned()
        .collect();
    let added_schemas: Vec<String> = current_schemas
        .difference(&prior_schemas)
        .cloned()
        .collect();

    // For paths present in both, compare verb sets to surface dropped operations.
    let mut removed_operations: Vec<String> = Vec::new();
    for path in prior_paths.intersection(&current_paths) {
        let prior_verbs = verbs_for_path(prior.pointer(&path_pointer(path)));
        let current_verbs = verbs_for_path(current.pointer(&path_pointer(path)));
        for v in prior_verbs.difference(&current_verbs) {
            removed_operations.push(format!("{} {}", v.to_uppercase(), path));
        }
    }

    let has_breaking =
        !removed_paths.is_empty() || !removed_schemas.is_empty() || !removed_operations.is_empty();

    let mut notes = ChangeNotes {
        added_paths,
        removed_paths,
        added_schemas,
        removed_schemas,
        removed_operations,
        has_breaking,
    };
    // Sort for deterministic output (sets aren't ordered).
    notes.added_paths.sort();
    notes.removed_paths.sort();
    notes.added_schemas.sort();
    notes.removed_schemas.sort();
    notes.removed_operations.sort();

    let summary = render_summary(&notes);
    (notes, summary)
}

fn collect_object_keys(node: Option<&Value>) -> BTreeSet<String> {
    match node {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        _ => BTreeSet::new(),
    }
}

fn verbs_for_path(node: Option<&Value>) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    if let Some(Value::Object(map)) = node {
        for v in VERBS {
            if map.contains_key(*v) {
                set.insert((*v).to_string());
            }
        }
    }
    set
}

/// Build a JSON pointer for `paths.<path>`. OpenAPI path strings contain `/`
/// and `~`, which are reserved in JSON pointers (RFC 6901), so escape them.
fn path_pointer(path: &str) -> String {
    let escaped = path.replace('~', "~0").replace('/', "~1");
    format!("/paths/{}", escaped)
}

fn render_summary(notes: &ChangeNotes) -> String {
    if notes.is_empty() {
        return "No structural changes (descriptions or examples may differ).".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    if !notes.added_paths.is_empty() {
        parts.push(format!("Added {} path(s)", notes.added_paths.len()));
    }
    if !notes.removed_paths.is_empty() {
        parts.push(format!("Removed {} path(s)", notes.removed_paths.len()));
    }
    if !notes.removed_operations.is_empty() {
        parts.push(format!(
            "Removed {} operation(s)",
            notes.removed_operations.len()
        ));
    }
    if !notes.added_schemas.is_empty() {
        parts.push(format!("Added {} schema(s)", notes.added_schemas.len()));
    }
    if !notes.removed_schemas.is_empty() {
        parts.push(format!("Removed {} schema(s)", notes.removed_schemas.len()));
    }

    let mut summary = parts.join("; ");
    if notes.has_breaking {
        summary.push_str(". Contains breaking changes (removals).");
    } else {
        summary.push('.');
    }

    // Append a short tail of the actual identifiers so the listing tells you
    // what changed at a glance.
    let mut details: Vec<String> = Vec::new();
    if !notes.removed_paths.is_empty() {
        details.push(format!(
            "removed paths: {}",
            sample(&notes.removed_paths, 5)
        ));
    }
    if !notes.removed_operations.is_empty() {
        details.push(format!(
            "removed ops: {}",
            sample(&notes.removed_operations, 5)
        ));
    }
    if !notes.removed_schemas.is_empty() {
        details.push(format!(
            "removed schemas: {}",
            sample(&notes.removed_schemas, 5)
        ));
    }
    if !details.is_empty() {
        summary.push_str(" — ");
        summary.push_str(&details.join("; "));
    }
    summary
}

fn sample(items: &[String], n: usize) -> String {
    if items.len() <= n {
        items.join(", ")
    } else {
        format!("{}, … (+{} more)", items[..n].join(", "), items.len() - n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_stable_across_key_order() {
        let a = json!({"a": 1, "b": {"x": 1, "y": 2}});
        let b = json!({"b": {"y": 2, "x": 1}, "a": 1});
        assert_eq!(spec_hash(&a), spec_hash(&b));
    }

    #[test]
    fn diff_finds_added_and_removed_paths() {
        let prior = json!({"paths": {"/a": {"get": {}}, "/b": {"get": {}}}});
        let current = json!({"paths": {"/a": {"get": {}}, "/c": {"post": {}}}});
        let (notes, summary) = compute_change_notes(&prior, &current);
        assert_eq!(notes.removed_paths, vec!["/b"]);
        assert_eq!(notes.added_paths, vec!["/c"]);
        assert!(notes.has_breaking);
        assert!(summary.contains("breaking"));
    }

    #[test]
    fn diff_finds_removed_verbs() {
        let prior = json!({"paths": {"/a": {"get": {}, "post": {}}}});
        let current = json!({"paths": {"/a": {"get": {}}}});
        let (notes, _) = compute_change_notes(&prior, &current);
        assert_eq!(notes.removed_operations, vec!["POST /a"]);
        assert!(notes.has_breaking);
    }

    #[test]
    fn diff_no_changes_is_not_breaking() {
        let spec = json!({"paths": {"/a": {"get": {}}}});
        let (notes, summary) = compute_change_notes(&spec, &spec);
        assert!(!notes.has_breaking);
        assert!(notes.is_empty());
        assert!(summary.starts_with("No structural"));
    }

    #[test]
    fn diff_finds_schema_changes() {
        let prior = json!({"components": {"schemas": {"User": {}, "Order": {}}}});
        let current = json!({"components": {"schemas": {"User": {}, "Invoice": {}}}});
        let (notes, _) = compute_change_notes(&prior, &current);
        assert_eq!(notes.removed_schemas, vec!["Order"]);
        assert_eq!(notes.added_schemas, vec!["Invoice"]);
    }

    #[test]
    fn path_pointer_escapes_slashes() {
        assert_eq!(path_pointer("/users/{id}"), "/paths/~1users~1{id}");
    }
}

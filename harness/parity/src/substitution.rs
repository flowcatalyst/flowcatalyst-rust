//! `${…}` substitution over a step's request (spec §3): path, query values,
//! header values and every string leaf of the JSON body. Produces fresh
//! values; the scenario's own tree (shared by both sides) is never mutated.

use anyhow::Result;
use indexmap::IndexMap;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::vars::Vars;

static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([^}]+)\}").expect("placeholder regex"));

/// Replaces every `${name}` in `template` with `vars.resolve(name)`.
pub fn resolve_str(template: &str, vars: &Vars) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut last = 0;
    for caps in PLACEHOLDER.captures_iter(template) {
        let m = caps.get(0).expect("whole match");
        out.push_str(&template[last..m.start()]);
        out.push_str(&vars.resolve(&caps[1])?);
        last = m.end();
    }
    out.push_str(&template[last..]);
    Ok(out)
}

/// [`resolve_str`] over every value of `map`, keys untouched.
pub fn resolve_map(
    map: &IndexMap<String, String>,
    vars: &Vars,
) -> Result<IndexMap<String, String>> {
    map.iter()
        .map(|(k, v)| Ok((k.clone(), resolve_str(v, vars)?)))
        .collect()
}

/// [`resolve_str`] over every string leaf of a JSON tree, keys untouched.
pub fn resolve_json(node: &Value, vars: &Vars) -> Result<Value> {
    Ok(match node {
        Value::String(s) => Value::String(resolve_str(s, vars)?),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), resolve_json(v, vars)?);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| resolve_json(v, vars))
                .collect::<Result<_>>()?,
        ),
        other => other.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vars::SeedIds;
    use serde_json::json;

    fn vars() -> Vars {
        Vars::new(
            "admin@example.com",
            "pw",
            "abc123",
            SeedIds {
                client_id: "clt_1".into(),
                app_id: "app_1".into(),
                admin_id: "prn_1".into(),
            },
            IndexMap::new(),
        )
    }

    #[test]
    fn substitutes_strings_and_json_leaves() {
        let v = vars();
        assert_eq!(
            resolve_str("/api/x/${client.id}?r=${run}", &v).unwrap(),
            "/api/x/clt_1?r=abc123"
        );
        let body = json!({"code": "p:${run}", "n": 3, "arr": ["${admin.email}", null]});
        assert_eq!(
            resolve_json(&body, &v).unwrap(),
            json!({"code": "p:abc123", "n": 3, "arr": ["admin@example.com", null]})
        );
    }

    #[test]
    fn an_undefined_name_is_an_error_not_an_empty_string() {
        assert!(resolve_str("${missing}", &vars()).is_err());
    }
}

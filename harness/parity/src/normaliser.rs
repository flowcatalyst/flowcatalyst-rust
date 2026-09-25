//! The normalisation rules (spec §5), applied in the spec's order to one
//! side's raw [`StepRecord`] before it is diffed. A pure function: the raw
//! record is untouched (kept for the report).
//!
//! A line-for-line port of Java's `Normaliser`, minus its one Java-specific
//! ruling (the `__Host-fc_session` → `fc_session` rename): Rust names its
//! session cookie `fc_session` as Go does (owner decision #26), so here a
//! renamed cookie would be a finding, not a normalisation.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use indexmap::IndexMap;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

use crate::model::{Ignore, Step};
use crate::record::{Body, StepRecord, CONTENT_TYPE, RETRY_AFTER, SET_COOKIE};
use crate::vars::Vars;

/// Rule 3: RFC 3339, whole-string match only.
static RFC3339: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$")
        .expect("rfc3339 regex")
});

/// The `Expires` cookie attribute (an RFC 1123 date, so rule 3 never reaches it).
static EXPIRES_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(Expires=)[^;]*").expect("expires regex"));

/// Epoch-seconds members/claims that are a time, not a value. `updated_at`
/// is a row timestamp each side stamps on its own clock.
pub const JWT_TIME_CLAIMS: [&str; 5] = ["iat", "exp", "nbf", "auth_time", "updated_at"];

/// Rule 1's substring form applies only to captured values at least this
/// long (in UTF-16 units, as Java's `String.length`); shorter ones would mask
/// by accident.
pub const MIN_SUBSTRING_CAPTURE: usize = 8;

/// One side's record after every rule: what [`crate::diff`] compares.
#[derive(Debug, Clone, PartialEq)]
pub struct Normalised {
    pub status: u16,
    pub headers: IndexMap<String, String>,
    pub body: Value,
}

pub fn java_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Applies rules 1–7 to `record`, using `vars`'s captures (rule 1) and
/// `base_url` (rule 2), then `step`'s `unordered` (rule 6) and `ignore` (rule 7).
pub fn normalise(record: &StepRecord, vars: &Vars, base_url: &str, step: &Step) -> Normalised {
    let labels = vars.labels();
    let mut headers = IndexMap::new();
    for (name, value) in &record.headers {
        headers.insert(
            name.clone(),
            normalise_header(name, value, &labels, base_url),
        );
    }
    let redirect = (300..400).contains(&record.status);
    // A redirect's body and Content-Type are not part of any contract; Location and status are.
    if redirect {
        headers.shift_remove(CONTENT_TYPE);
    }
    let mut body = if redirect {
        Value::String("«redirect»".into())
    } else {
        normalise_node(&body_of(record), &labels, base_url)
    };
    apply_unordered(&mut body, &step.unordered);
    apply_ignore(&mut body, &step.ignore);
    Normalised {
        status: record.status,
        headers,
        body,
    }
}

/// `Set-Cookie` is masked outright (rule 5); `Retry-After` collapses to
/// presence; everything else goes through rules 1–3 as a plain string.
pub fn normalise_header(
    name: &str,
    value: &str,
    labels: &IndexMap<String, String>,
    base_url: &str,
) -> String {
    if name == SET_COOKIE {
        return mask_cookie(value);
    }
    if name == RETRY_AFTER {
        return "«present»".into();
    }
    match normalise_string(value, labels, base_url) {
        Value::String(s) => s,
        // Java's `asString()` on a container node is empty (a header whose
        // whole value is a JWS; not seen in practice).
        _ => String::new(),
    }
}

/// Keeps the cookie's name and attributes, masks the value (rule 5), and
/// masks an `Expires` value to `«time»`. Attributes compare as a set.
pub fn mask_cookie(set_cookie: &str) -> String {
    let (pair, rest) = match set_cookie.find(';') {
        Some(i) => (&set_cookie[..i], &set_cookie[i..]),
        None => (set_cookie, ""),
    };
    let name = match pair.find('=') {
        Some(i) => &pair[..i],
        None => pair,
    };
    let mut attrs: Vec<&str> = rest
        .split(';')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .collect();
    // Java's String.CASE_INSENSITIVE_ORDER.
    attrs.sort_by(|a, b| {
        a.chars()
            .map(|c| c.to_lowercase().collect::<String>())
            .cmp(b.chars().map(|c| c.to_lowercase().collect::<String>()))
    });
    let masked = if attrs.is_empty() {
        format!("{name}=«cookie»")
    } else {
        format!("{name}=«cookie»; {}", attrs.join("; "))
    };
    EXPIRES_ATTR.replace_all(&masked, "${1}«time»").into_owned()
}

fn body_of(record: &StepRecord) -> Value {
    match &record.body {
        Body::Json(v) => v.clone(),
        Body::Text(t) => Value::String(t.clone()),
        Body::Sha256(h) => Value::String(format!("sha256:{h}")),
    }
}

fn normalise_node(node: &Value, labels: &IndexMap<String, String>, base_url: &str) -> Value {
    match node {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                // Rule 3, numeric form: an epoch-seconds member named like a JWT
                // time claim (introspection echoes exp/iat) is a time, not a value.
                if JWT_TIME_CLAIMS.contains(&k.as_str()) && v.is_number() {
                    out.insert(k.clone(), Value::String("«time»".into()));
                } else {
                    out.insert(k.clone(), normalise_node(v, labels, base_url));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| normalise_node(v, labels, base_url))
                .collect(),
        ),
        Value::String(s) => normalise_string(s, labels, base_url),
        other => other.clone(),
    }
}

/// Rule 4 first (a JWS is compared as a structure, never collapsed to its
/// capture name), then rules 1, 2, 3 in order.
fn normalise_string(s: &str, labels: &IndexMap<String, String>, base_url: &str) -> Value {
    if let Some(jws) = try_decode_jws(s, labels, base_url) {
        return jws;
    }
    if let Some(whole) = labels.get(s) {
        return Value::String(format!("«{whole}»"));
    }
    // Rule 1, substring form: an id or token embedded in a message or a derived id.
    let mut out = s.to_string();
    for (value, name) in labels {
        if java_len(value) >= MIN_SUBSTRING_CAPTURE && out.contains(value.as_str()) {
            out = out.replace(value.as_str(), &format!("«{name}»"));
        }
    }
    if !base_url.is_empty() {
        out = out.replace(base_url, "«base»");
    }
    if RFC3339.is_match(&out) {
        out = "«time»".into();
    }
    Value::String(out)
}

/// `{"«jwt»": {"header": …, "claims": …}}`, or `None` when `s` is not a JWS:
/// three non-empty base64url segments, the header a JSON object carrying `alg`.
fn try_decode_jws(s: &str, labels: &IndexMap<String, String>, base_url: &str) -> Option<Value> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let header = decode_segment(parts[0])?;
    header.get("alg")?;
    let claims = decode_segment(parts[1])?;

    let norm_header = normalise_node(&header, labels, base_url);
    let mut norm_claims = normalise_node(&claims, labels, base_url);
    if let Value::Object(c) = &mut norm_claims {
        for field in JWT_TIME_CLAIMS {
            if c.contains_key(field) {
                c.insert(field.to_string(), Value::String("«time»".into()));
            }
        }
        if c.contains_key("jti") {
            c.insert("jti".into(), Value::String("«id»".into()));
        }
    }
    let mut inner = Map::new();
    inner.insert("header".into(), norm_header);
    inner.insert("claims".into(), norm_claims);
    let mut result = Map::new();
    result.insert("«jwt»".into(), Value::Object(inner));
    Some(Value::Object(result))
}

fn decode_segment(segment: &str) -> Option<Value> {
    let bytes = URL_SAFE_NO_PAD.decode(segment.trim_end_matches('=')).ok()?;
    let node: Value = serde_json::from_slice(&bytes).ok()?;
    node.is_object().then_some(node)
}

/// Rule 6: each `unordered` pointer's array, sorted by its elements'
/// normalised compact JSON text.
fn apply_unordered(body: &mut Value, pointers: &[String]) {
    for pointer in pointers {
        if let Some(Value::Array(items)) = body.pointer_mut(pointer) {
            items.sort_by_cached_key(|v| java_sort_key(&v.to_string()));
        }
    }
}

/// Java compares `String`s by UTF-16 code unit; Rust `str` by byte. They
/// agree on the BMP-free ASCII this compares in practice, but the key keeps
/// the port exact.
fn java_sort_key(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Rule 7: each `ignore` pointer removed. A `*` segment means "every array
/// element"; every other segment is a literal key or index.
fn apply_ignore(body: &mut Value, ignores: &[Ignore]) {
    for ignore in ignores {
        let Some(pointer) = ignore.pointer.as_deref() else {
            continue;
        };
        if pointer.is_empty() || pointer == "/" {
            continue;
        }
        let segments: Vec<String> = pointer[1..]
            .split('/')
            .map(|s| s.replace("~1", "/").replace("~0", "~"))
            .collect();
        remove_at(body, &segments, 0);
    }
}

fn remove_at(node: &mut Value, segments: &[String], index: usize) {
    let segment = segments[index].as_str();
    let last = index == segments.len() - 1;
    match node {
        Value::Array(items) if segment == "*" => {
            if last {
                return;
            }
            for child in items {
                remove_at(child, segments, index + 1);
            }
        }
        Value::Object(map) => {
            if last {
                map.shift_remove(segment);
            } else if let Some(child) = map.get_mut(segment) {
                remove_at(child, segments, index + 1);
            }
        }
        Value::Array(items) => {
            if let Ok(i) = segment.parse::<usize>() {
                if !last {
                    if let Some(child) = items.get_mut(i) {
                        remove_at(child, segments, index + 1);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::LOCATION;
    use crate::vars::SeedIds;
    use serde_json::json;

    const BASE: &str = "http://localhost:4000";

    fn vars() -> Vars {
        Vars::new(
            "admin@example.com",
            "pw",
            "run",
            SeedIds {
                client_id: "clt_0SEEDCLIENT01".into(),
                app_id: "app_0SEEDAPP0001".into(),
                admin_id: "prn_0SEEDADMIN01".into(),
            },
            IndexMap::new(),
        )
    }

    fn step(json: Value) -> Step {
        serde_json::from_value(json).unwrap()
    }

    fn plain_step() -> Step {
        step(json!({"id": "s", "request": {"method": "GET", "path": "/"}}))
    }

    fn record(status: u16, body: Value) -> StepRecord {
        StepRecord {
            status,
            headers: IndexMap::new(),
            body: Body::Json(body),
        }
    }

    fn jws(header: Value, claims: Value) -> String {
        format!(
            "{}.{}.c2ln",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    #[test]
    fn rule1_replaces_a_captured_value_whole_and_embedded() {
        let mut v = vars();
        v.capture("etId", "evt_0ABCDEFGHIJKL");
        let out = normalise(
            &record(
                200,
                json!({"id": "evt_0ABCDEFGHIJKL", "other": "untouched",
                       "message": "EventType not found: evt_0ABCDEFGHIJKL",
                       "derived": "evt_0ABCDEFGHIJKL-role-0", "status": "ok-ish"}),
            ),
            &v,
            BASE,
            &plain_step(),
        );
        assert_eq!(out.body["id"], "«etId»");
        assert_eq!(out.body["other"], "untouched");
        assert_eq!(out.body["message"], "EventType not found: «etId»");
        assert_eq!(out.body["derived"], "«etId»-role-0");
        assert_eq!(out.body["status"], "ok-ish");
    }

    #[test]
    fn rule2_and_rule3() {
        let out = normalise(
            &record(
                200,
                json!({"issuer": format!("{BASE}/auth"), "createdAt": "2026-05-24T08:30:00.123456Z",
                       "message": "at 2026-05-24T08:30:00.123456Z", "exp": 1700000000}),
            ),
            &vars(),
            BASE,
            &plain_step(),
        );
        assert_eq!(out.body["issuer"], "«base»/auth");
        assert_eq!(out.body["createdAt"], "«time»");
        assert_eq!(out.body["message"], "at 2026-05-24T08:30:00.123456Z");
        assert_eq!(out.body["exp"], "«time»");
    }

    #[test]
    fn rule4_decodes_a_jws_then_rule1_inside_the_claims() {
        let token = jws(
            json!({"alg": "RS256", "kid": "k"}),
            json!({"sub": "prn_0SEEDADMIN01", "iat": 1, "exp": 2, "updated_at": 3, "jti": "x",
                   "iss": BASE}),
        );
        let mut v = vars();
        v.capture("adminId", "prn_0SEEDADMIN01");
        v.capture("sess", &token);
        let out = normalise(
            &record(200, json!({"t": token, "value": "not.a.jwt"})),
            &v,
            BASE,
            &plain_step(),
        );
        let claims = &out.body["t"]["«jwt»"]["claims"];
        assert_eq!(claims["sub"], "«adminId»");
        assert_eq!(claims["iat"], "«time»");
        assert_eq!(claims["updated_at"], "«time»");
        assert_eq!(claims["jti"], "«id»");
        assert_eq!(claims["iss"], "«base»");
        assert_eq!(out.body["value"], "not.a.jwt");
    }

    #[test]
    fn rule5_masks_the_cookie_value_and_expires_and_sorts_attributes() {
        assert_eq!(
            mask_cookie("fc_session=abc; Path=/; HttpOnly; Expires=Wed, 21 Oct 2026 07:28:00 GMT; SameSite=Lax"),
            "fc_session=«cookie»; Expires=«time»; HttpOnly; Path=/; SameSite=Lax"
        );
        assert_eq!(
            mask_cookie("fc_session=a; Secure; HttpOnly"),
            mask_cookie("fc_session=b; HttpOnly; Secure")
        );
        // No Java ruling carried over: a renamed cookie still compares by name.
        assert_eq!(
            mask_cookie("__Host-fc_session=a; Secure"),
            "__Host-fc_session=«cookie»; Secure"
        );
    }

    #[test]
    fn rule6_sorts_unordered_arrays_and_default_is_ordered() {
        let unordered = step(json!({"id": "s", "request": {"method": "GET", "path": "/"},
                                    "unordered": ["/items"]}));
        let a = normalise(
            &record(200, json!({"items": [{"n": 2}, {"n": 1}]})),
            &vars(),
            BASE,
            &unordered,
        );
        let b = normalise(
            &record(200, json!({"items": [{"n": 1}, {"n": 2}]})),
            &vars(),
            BASE,
            &unordered,
        );
        assert_eq!(a.body, b.body);
        let a = normalise(
            &record(200, json!({"items": [{"n": 2}, {"n": 1}]})),
            &vars(),
            BASE,
            &plain_step(),
        );
        let b = normalise(
            &record(200, json!({"items": [{"n": 1}, {"n": 2}]})),
            &vars(),
            BASE,
            &plain_step(),
        );
        assert_ne!(a.body, b.body);
    }

    #[test]
    fn rule7_removes_ignored_pointers_including_wildcards() {
        let s = step(json!({"id": "s", "request": {"method": "GET", "path": "/"},
                            "ignore": [{"pointer": "/items/*/createdBy", "reason": "r"}]}));
        let out = normalise(
            &record(
                200,
                json!({"items": [{"id": "1", "createdBy": "a"}, {"id": "2", "createdBy": "b"}]}),
            ),
            &vars(),
            BASE,
            &s,
        );
        assert_eq!(out.body, json!({"items": [{"id": "1"}, {"id": "2"}]}));
    }

    #[test]
    fn a_redirect_compares_status_and_location_only() {
        let mut rec = record(302, json!({}));
        rec.headers.insert(CONTENT_TYPE.into(), "text/html".into());
        rec.headers.insert(LOCATION.into(), format!("{BASE}/login"));
        let out = normalise(&rec, &vars(), BASE, &plain_step());
        assert_eq!(out.body, json!("«redirect»"));
        assert!(!out.headers.contains_key(CONTENT_TYPE));
        assert_eq!(out.headers[LOCATION], "«base»/login");
    }
}

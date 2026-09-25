//! Runs one scenario against one side (spec §3, §4): its own HTTP client,
//! its own cookie jar, redirects never followed, 10 s per request. Any
//! failure while building, sending or capturing a step (an undefined
//! `${…}`, a missing capture pointer, an `expect.status` mismatch) becomes a
//! [`StepOutcome::Failed`] for that step alone; the runner keeps going, so a
//! later step is reported on its own merits.
//!
//! The cookie jar is hand-rolled, as in Java: `fc_session` is `Secure` and
//! both sides serve plain loopback HTTP, so a standards-following jar would
//! drop it after login. This one carries whatever `Set-Cookie` sends and
//! forgets a cookie set to an empty value.
//!
//! One deliberate refinement over Java: a scenario header named `Cookie`
//! *replaces* the jar's `Cookie` header, and an empty value sends none. The
//! scenarios use `"headers": {"Cookie": ""}` to mean "bearer only, no
//! session" (see their `why` notes); Java's `HttpRequest.Builder.header`
//! appends instead, which only works if the JDK drops the empty line.

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

use crate::authenticator::SoftAuthenticator;
use crate::coverage::RequestedRoute;
use crate::model::{Request, Scenario, Step};
use crate::normaliser::{java_len, MIN_SUBSTRING_CAPTURE};
use crate::record::StepRecord;
use crate::substitution::{resolve_json, resolve_map, resolve_str};
use crate::vars::Vars;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Members that are per-side by construction (ids, secrets, tokens, cursors,
/// links that carry a token) anywhere in a 2xx body, captured quietly under
/// `auto:<member>` so a later response carrying the same value is masked.
static AUTO_CAPTURE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(id|.*Id|.*Secret|.*SecretRef|.*Cursor|.*Token|.*Url|.*Link|challenge)$")
        .expect("auto-capture regex")
});

/// One step's fate on one side, in scenario order.
#[derive(Debug, Clone)]
pub enum StepOutcome {
    Ran(StepRecord),
    /// `record` is the raw response when one was received but failed
    /// `expect.status`; `None` when the request could not be built or sent.
    Failed {
        message: String,
        record: Option<StepRecord>,
    },
}

/// Every step's outcome and the concrete `(method, path)` of every request sent.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub steps: Vec<StepOutcome>,
    pub requested: Vec<RequestedRoute>,
}

pub struct Runner {
    client: reqwest::Client,
    base_url: String,
    /// cookie name → value, last `Set-Cookie` wins, empty value removes.
    jar: IndexMap<String, String>,
}

struct Sent {
    status: u16,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Sent {
    fn json_body(&self) -> Option<Value> {
        let ct = header_str(&self.headers, "Content-Type").unwrap_or_default();
        if !ct.to_ascii_lowercase().contains("json") || self.body.is_empty() {
            return None;
        }
        serde_json::from_slice(&self.body).ok()
    }
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
}

impl Runner {
    pub fn new(base_url: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .http1_only()
            .no_proxy()
            // Both sides are addressed as `localhost` (WebAuthn needs a host
            // name, not an IP, as the RP id); always dial IPv4 loopback.
            .resolve("localhost", ([127, 0, 0, 1], 0).into())
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.to_string(),
            jar: IndexMap::new(),
        })
    }

    pub async fn run(&mut self, scenario: &Scenario, vars: &mut Vars) -> RunResult {
        let mut outcomes = Vec::with_capacity(scenario.steps.len());
        let mut requested = Vec::new();
        let mut authenticator: Option<SoftAuthenticator> = None;
        let mut last_body: Option<Value> = None;
        for step in &scenario.steps {
            let outcome = self
                .run_step(
                    step,
                    vars,
                    &mut authenticator,
                    &mut last_body,
                    &mut requested,
                )
                .await
                .unwrap_or_else(|e| StepOutcome::Failed {
                    message: format!("{e:#}"),
                    record: None,
                });
            outcomes.push(outcome);
        }
        RunResult {
            steps: outcomes,
            requested,
        }
    }

    async fn run_step(
        &mut self,
        step: &Step,
        vars: &mut Vars,
        authenticator: &mut Option<SoftAuthenticator>,
        last_body: &mut Option<Value>,
        requested: &mut Vec<RequestedRoute>,
    ) -> Result<StepOutcome> {
        let mut body = match &step.request.body {
            Some(b) => Some(resolve_json(b, vars)?),
            None => None,
        };
        if let Some(kind) = step.authenticator.as_deref() {
            // The previous response wraps the ceremony options as
            // {stateId, options: {publicKey}}; the authenticator wants `options`.
            // Its output becomes the body's `credential` member when the step
            // gave a body object, else the whole body.
            let options = last_body
                .as_ref()
                .map(|b| b.get("options").cloned().unwrap_or_else(|| b.clone()))
                .ok_or_else(|| {
                    anyhow!(
                        "authenticator step '{}' has no prior response to act on",
                        step.id
                    )
                })?;
            let auth = authenticator.get_or_insert_with(SoftAuthenticator::new);
            let credential = match kind {
                "register" => auth.register(&options, &self.base_url),
                // The only principal identity every scenario carries (Java's choice).
                "assert" => auth.assertion(&options, &self.base_url, &vars.resolve("admin.id")?),
                other => bail!("unknown authenticator step: {other}"),
            }
            .with_context(|| format!("authenticator step '{}' failed", step.id))?;
            match body.as_mut() {
                Some(Value::Object(map)) => {
                    map.insert("credential".into(), credential);
                }
                _ => body = Some(credential),
            }
        }

        let (sent, path) = self.send(&step.request, body.as_ref(), vars).await?;
        requested.push(RequestedRoute {
            method: step.request.method.clone(),
            path,
        });
        let record = StepRecord::from_response(sent.status, &sent.headers, &sent.body)?;

        if let Some(expected) = step.expect.as_ref().and_then(|e| e.status) {
            if expected != sent.status {
                *last_body = sent.json_body();
                return Ok(StepOutcome::Failed {
                    message: format!("expected status {expected} but got {}", sent.status),
                    record: Some(record),
                });
            }
        }

        capture(step, &sent, vars)?;
        auto_capture_ids(&sent, vars);
        *last_body = sent.json_body();
        Ok(StepOutcome::Ran(record))
    }

    async fn send(
        &mut self,
        request: &Request,
        body: Option<&Value>,
        vars: &Vars,
    ) -> Result<(Sent, String)> {
        let path = resolve_str(&request.path, vars)?;
        let query = encode_pairs(&resolve_map(&request.query, vars)?);
        let url = format!(
            "{}{}{}",
            self.base_url,
            path,
            if query.is_empty() {
                String::new()
            } else {
                format!("?{query}")
            }
        );
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|e| anyhow!("bad method {}: {e}", request.method))?;

        let mut headers = HeaderMap::new();
        let payload: Option<Vec<u8>> = if !request.form.is_empty() {
            headers.insert(
                "Content-Type",
                HeaderValue::from_static("application/x-www-form-urlencoded"),
            );
            Some(encode_pairs(&resolve_map(&request.form, vars)?).into_bytes())
        } else if let Some(b) = body {
            headers.insert("Content-Type", HeaderValue::from_static("application/json"));
            Some(serde_json::to_vec(b)?)
        } else {
            None
        };
        let custom = resolve_map(&request.headers, vars)?;
        let cookie_overridden = custom.keys().any(|k| k.eq_ignore_ascii_case("cookie"));
        if !self.jar.is_empty() && !cookie_overridden {
            headers.append("Cookie", HeaderValue::from_str(&self.cookie_header())?);
        }
        for (name, value) in &custom {
            if name.eq_ignore_ascii_case("cookie") && value.is_empty() {
                continue;
            }
            headers.append(
                HeaderName::from_bytes(name.as_bytes())?,
                HeaderValue::from_str(value)?,
            );
        }
        if let Some(auth) = &request.auth {
            headers.append(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", resolve_str(auth, vars)?))?,
            );
        }

        let mut builder = self.client.request(method, &url).headers(headers);
        if let Some(p) = payload {
            builder = builder.body(p);
        }
        let response = builder
            .send()
            .await
            .map_err(|e| anyhow!("{} {path}: {e}", request.method))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .map_err(|e| anyhow!("{} {path}: reading body: {e}", request.method))?
            .to_vec();
        self.update_jar(&headers);
        Ok((
            Sent {
                status,
                headers,
                body,
            },
            path,
        ))
    }

    fn cookie_header(&self) -> String {
        self.jar
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Every `Set-Cookie` updates the jar: an empty value removes the cookie,
    /// anything else (over)writes it. Attributes are not tracked.
    fn update_jar(&mut self, headers: &HeaderMap) {
        for sc in headers.get_all("Set-Cookie") {
            let sc = String::from_utf8_lossy(sc.as_bytes());
            let pair = sc.split(';').next().unwrap_or_default();
            let Some(eq) = pair.find('=').filter(|&i| i > 0) else {
                continue;
            };
            let name = pair[..eq].trim().to_string();
            let value = pair[eq + 1..].trim().to_string();
            if value.is_empty() {
                self.jar.shift_remove(&name);
            } else {
                self.jar.insert(name, value);
            }
        }
    }
}

/// `application/x-www-form-urlencoded` pairs, as Java's `URLEncoder`
/// (space → `+`, only `A-Za-z0-9.-*_` left bare).
fn encode_pairs(pairs: &IndexMap<String, String>) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                url::form_urlencoded::byte_serialize(k.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// A 2xx JSON body's per-side members, captured quietly (see [`AUTO_CAPTURE_NAME`]).
fn auto_capture_ids(sent: &Sent, vars: &mut Vars) {
    if !(200..300).contains(&sent.status) {
        return;
    }
    if let Some(body) = sent.json_body() {
        auto_capture(&body, vars);
    }
}

fn auto_capture(node: &Value, vars: &mut Vars) {
    match node {
        Value::Object(map) => {
            for (k, v) in map {
                match v {
                    Value::String(s)
                        if AUTO_CAPTURE_NAME.is_match(k)
                            && java_len(s) >= MIN_SUBSTRING_CAPTURE =>
                    {
                        // Labelled by member name only: the same row can surface at
                        // different list positions on the two sides.
                        vars.capture_quietly(&format!("auto:{k}"), s);
                    }
                    other => auto_capture(other, vars),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| auto_capture(v, vars)),
        _ => {}
    }
}

/// Applies `step.capture` against the raw (unnormalised) response.
fn capture(step: &Step, sent: &Sent, vars: &mut Vars) -> Result<()> {
    for (name, spec) in &step.capture {
        let value = if let Some(header) = spec.strip_prefix("header:") {
            header_str(&sent.headers, header)
                .ok_or_else(|| anyhow!("capture '{name}': header {header} not present"))?
        } else if let Some(param) = spec.strip_prefix("location-param:") {
            let location = header_str(&sent.headers, "Location")
                .ok_or_else(|| anyhow!("capture '{name}': Location header not present"))?;
            query_param(&location, param).ok_or_else(|| {
                anyhow!("capture '{name}': Location has no query parameter {param}")
            })?
        } else if let Some(rest) = spec.strip_prefix("param:") {
            // `param:<pointer>?<name>`: a query parameter of a URL held in a body member.
            let q = rest.rfind('?').ok_or_else(|| {
                anyhow!("capture '{name}': param:<pointer>?<name> expected, got {spec}")
            })?;
            let (pointer, param) = (&rest[..q], &rest[q + 1..]);
            let body = sent.json_body();
            let url = body
                .as_ref()
                .and_then(|b| b.pointer(pointer))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "capture '{name}': pointer {pointer} is not a string in the response body"
                    )
                })?;
            query_param(url, param).ok_or_else(|| {
                anyhow!("capture '{name}': {pointer} has no query parameter {param}")
            })?
        } else if let Some(cookie) = spec.strip_prefix("cookie:") {
            cookie_value(&sent.headers, cookie)
                .ok_or_else(|| anyhow!("capture '{name}': cookie {cookie} not present"))?
        } else {
            let body = sent.json_body();
            let at = body.as_ref().and_then(|b| b.pointer(spec)).ok_or_else(|| {
                anyhow!("capture '{name}': pointer {spec} not present in the response body")
            })?;
            match at {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            }
        };
        // An empty capture would mask every empty string on that side.
        if value.is_empty() {
            bail!("capture '{name}': {spec} resolved to an empty value");
        }
        vars.capture(name, &value);
    }
    Ok(())
}

/// One decoded query parameter of a URL.
pub fn query_param(url: &str, param: &str) -> Option<String> {
    let q = url.find('?')?;
    let mut query = &url[q + 1..];
    if let Some(hash) = query.find('#') {
        query = &query[..hash];
    }
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == param)
        .map(|(_, v)| v.into_owned())
}

fn cookie_value(headers: &HeaderMap, cookie: &str) -> Option<String> {
    headers.get_all("Set-Cookie").iter().find_map(|sc| {
        let sc = String::from_utf8_lossy(sc.as_bytes());
        let pair = sc.split(';').next().unwrap_or_default();
        let eq = pair.find('=').filter(|&i| i > 0)?;
        (pair[..eq].trim() == cookie).then(|| pair[eq + 1..].trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_decode_like_java() {
        assert_eq!(
            query_param("http://x/cb?code=a%2Bb&state=s+t#frag", "code").as_deref(),
            Some("a+b")
        );
        assert_eq!(
            query_param("http://x/cb?code=a&state=s+t", "state").as_deref(),
            Some("s t")
        );
        assert_eq!(query_param("http://x/cb", "code"), None);
    }

    #[test]
    fn form_encoding_matches_java_url_encoder() {
        let mut m = IndexMap::new();
        m.insert("redirect_uri".to_string(), "http://a b/*~".to_string());
        assert_eq!(encode_pairs(&m), "redirect_uri=http%3A%2F%2Fa+b%2F*%7E");
    }
}

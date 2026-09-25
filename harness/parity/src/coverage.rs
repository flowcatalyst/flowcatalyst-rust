//! Coverage (spec §7): Go's lockfile operations (`lockfile-operations.json`,
//! extracted from Go's `api/openapi.lock.json`), plus the outside-lockfile
//! surface listed by hand in `surface.json`, each marked hit when some request
//! the run actually sent matches its `METHOD path` template.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One `METHOD /path` entry, from the lockfile or `surface.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub method: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.method, self.path)
    }
}

/// A request a side actually sent: the method and the resolved path (no query).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedRoute {
    pub method: String,
    pub path: String,
}

/// The vendored lockfile extract: provenance plus the operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockfileOperations {
    pub source: String,
    pub commit: String,
    pub operations: Vec<Route>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageResult {
    pub lockfile_total: usize,
    pub lockfile_hit: usize,
    pub surface_total: usize,
    pub surface_hit: usize,
    pub missing_lockfile: Vec<String>,
    pub missing_surface: Vec<String>,
}

impl CoverageResult {
    pub fn lockfile_coverage(&self) -> f64 {
        ratio(self.lockfile_hit, self.lockfile_total)
    }
    pub fn surface_coverage(&self) -> f64 {
        ratio(self.surface_hit, self.surface_total)
    }
}

fn ratio(hit: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        hit as f64 / total as f64
    }
}

/// `/api/event-types/{id}` matches `/api/event-types/abc` but not
/// `/api/event-types` (a different segment count is a different operation).
pub fn matches_template(template: &str, actual: &str) -> bool {
    let t: Vec<&str> = template.split('/').collect();
    let a: Vec<&str> = actual.split('/').collect();
    t.len() == a.len()
        && t.iter()
            .zip(&a)
            .all(|(seg, act)| (seg.starts_with('{') && seg.ends_with('}')) || seg == act)
}

fn is_hit(route: &Route, requested: &[RequestedRoute]) -> bool {
    requested.iter().any(|r| {
        r.method.eq_ignore_ascii_case(&route.method) && matches_template(&route.path, &r.path)
    })
}

pub fn load_lockfile(path: &Path) -> Result<LockfileOperations> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

/// `surface.json`: a JSON array of `"METHOD /path"` strings (absent = empty).
pub fn load_surface(path: &Path) -> Result<Vec<Route>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let lines: Vec<String> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    lines
        .iter()
        .map(|line| match line.split_once(' ') {
            Some((m, p)) => Ok(Route {
                method: m.to_ascii_uppercase(),
                path: p.to_string(),
                operation_id: None,
            }),
            None => bail!("surface.json entry is not 'METHOD /path': {line}"),
        })
        .collect()
}

pub fn compute(
    lockfile: &[Route],
    surface: &[Route],
    requested: &[RequestedRoute],
) -> CoverageResult {
    let missing = |routes: &[Route]| -> Vec<String> {
        routes
            .iter()
            .filter(|r| !is_hit(r, requested))
            .map(ToString::to_string)
            .collect()
    };
    let missing_lockfile = missing(lockfile);
    let missing_surface = missing(surface);
    CoverageResult {
        lockfile_total: lockfile.len(),
        lockfile_hit: lockfile.len() - missing_lockfile.len(),
        surface_total: surface.len(),
        surface_hit: surface.len() - missing_surface.len(),
        missing_lockfile,
        missing_surface,
    }
}

/// A scenario's `covers` claims checked against what it actually requested
/// on one side: the claimed ids never hit (or not in the lockfile at all).
pub fn unmet_claims(
    claims: &[String],
    lockfile: &[Route],
    requested: &[RequestedRoute],
) -> Vec<String> {
    claims
        .iter()
        .filter_map(|claim| {
            match lockfile
                .iter()
                .find(|r| r.operation_id.as_deref() == Some(claim.as_str()))
            {
                None => Some(format!("{claim} (not a lockfile operationId)")),
                Some(route) if !is_hit(route, requested) => Some(claim.clone()),
                Some(_) => None,
            }
        })
        .collect()
}

/// Extracts `(method, path, operationId)` from Go's `api/openapi.lock.json`.
pub fn extract_lockfile(
    openapi: &serde_json::Value,
    source: &str,
    commit: &str,
) -> Result<LockfileOperations> {
    let paths = openapi
        .get("paths")
        .and_then(|p| p.as_object())
        .context("lockfile has no paths object")?;
    let mut operations = Vec::new();
    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            continue;
        };
        for (method, op) in item {
            if !matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete") {
                continue;
            }
            operations.push(Route {
                method: method.to_ascii_uppercase(),
                path: path.clone(),
                operation_id: op
                    .get("operationId")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
    }
    Ok(LockfileOperations {
        source: source.to_string(),
        commit: commit.to_string(),
        operations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(m: &str, p: &str) -> RequestedRoute {
        RequestedRoute {
            method: m.into(),
            path: p.into(),
        }
    }

    #[test]
    fn template_matching_is_per_segment() {
        assert!(matches_template(
            "/api/event-types/{id}",
            "/api/event-types/abc"
        ));
        assert!(!matches_template(
            "/api/event-types/{id}",
            "/api/event-types"
        ));
        assert!(!matches_template(
            "/api/event-types/{id}",
            "/api/clients/abc"
        ));
    }

    #[test]
    fn covers_claims_and_coverage() {
        let lock = vec![
            Route {
                method: "GET".into(),
                path: "/api/x/{id}".into(),
                operation_id: Some("getX".into()),
            },
            Route {
                method: "POST".into(),
                path: "/api/x".into(),
                operation_id: Some("createX".into()),
            },
        ];
        let requested = vec![req("get", "/api/x/1")];
        assert_eq!(
            unmet_claims(
                &["getX".into(), "createX".into(), "nope".into()],
                &lock,
                &requested
            ),
            vec![
                "createX".to_string(),
                "nope (not a lockfile operationId)".to_string()
            ]
        );
        let c = compute(&lock, &[], &requested);
        assert_eq!((c.lockfile_hit, c.lockfile_total), (1, 2));
        assert_eq!(c.surface_coverage(), 1.0);
    }
}

//! The scenario file format (Java `parity/scenarios/**`, spec §3), read
//! unchanged so the files can be re-synced from the Java repo by copying.
//! Unknown members (`why`, at file and step level) are ignored.

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

/// One scenario file: an ordered list of steps run to completion on Go
/// first and then on Rust, each side against its own clone.
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub name: String,
    /// Lockfile `operationId`s this scenario claims to exercise; a claim no
    /// request actually hit fails the run.
    #[serde(default)]
    pub covers: Vec<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// One request/response pair within a scenario.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub id: String,
    pub request: Request,
    /// A sanity check, not the oracle: the step is `ERROR` on the side that
    /// disagrees.
    #[serde(default)]
    pub expect: Option<Expect>,
    /// name → JSON Pointer, or `header:` / `cookie:` / `location-param:` /
    /// `param:<pointer>?<name>`; captured per side.
    #[serde(default)]
    pub capture: IndexMap<String, String>,
    /// Pointers to arrays compared as multisets (rule 6).
    #[serde(default)]
    pub unordered: Vec<String>,
    /// Pointers dropped from both sides before comparison (rule 7).
    #[serde(default)]
    pub ignore: Vec<Ignore>,
    /// `"register"` or `"assert"`: run the software authenticator over the
    /// previous step's response.
    #[serde(default)]
    pub authenticator: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Expect {
    #[serde(default)]
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ignore {
    pub pointer: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub reason: Option<String>,
}

/// A step's request. At most one of `body` (JSON) / `form`
/// (`application/x-www-form-urlencoded`) is set.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: IndexMap<String, String>,
    #[serde(default)]
    pub headers: IndexMap<String, String>,
    /// A bearer token: sugar for `Authorization: Bearer …`.
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub form: IndexMap<String, String>,
}

impl Scenario {
    /// Rejects the shapes the Java record constructors reject.
    pub fn validate(&self) -> anyhow::Result<()> {
        for step in &self.steps {
            if step.request.body.is_some() && !step.request.form.is_empty() {
                anyhow::bail!(
                    "scenario '{}' step '{}': a request may carry body or form, never both",
                    self.name,
                    step.id
                );
            }
        }
        Ok(())
    }
}

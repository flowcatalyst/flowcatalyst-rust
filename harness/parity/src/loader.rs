//! Loads `scenarios/<group>/<name>.json` in a stable order (by relative
//! path), optionally filtered by `--only <glob>` against the path relative
//! to the scenarios directory (e.g. `smoke/*`), with Java `PathMatcher`
//! glob semantics: `*` and `?` stay inside one path segment, `**` crosses
//! segments, `{a,b}` is an alternation, `[...]` a character class.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

use crate::model::Scenario;

#[derive(Debug, Clone)]
pub struct Loaded {
    pub relative_path: String,
    pub scenario: Scenario,
}

pub fn load(dir: &Path, only: Option<&str>) -> Result<Vec<Loaded>> {
    if !dir.is_dir() {
        bail!("no such scenarios directory: {}", dir.display());
    }
    let matcher = match only.map(str::trim).filter(|g| !g.is_empty()) {
        Some(glob) => Some(glob_to_regex(glob)?),
        None => None,
    };
    let mut files = Vec::new();
    walk(dir, &mut files)?;
    let mut out = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(m) = &matcher {
            if !m.is_match(&relative) {
                continue;
            }
        }
        let bytes = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
        let scenario: Scenario =
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", file.display()))?;
        scenario.validate()?;
        out.push(Loaded {
            relative_path: relative,
            scenario,
        });
    }
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("walk {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
    Ok(())
}

/// Translates a glob to an anchored regex with Java `PathMatcher` semantics.
pub fn glob_to_regex(glob: &str) -> Result<Regex> {
    let mut re = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    let mut in_group = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                re.push_str(".*");
                i += 1;
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '{' if !in_group => {
                re.push_str("(?:");
                in_group = true;
            }
            '}' if in_group => {
                re.push(')');
                in_group = false;
            }
            ',' if in_group => re.push('|'),
            '[' => {
                let end = chars[i..]
                    .iter()
                    .position(|&ch| ch == ']')
                    .map(|p| p + i)
                    .context("unterminated [ in glob")?;
                let mut class: String = chars[i + 1..end].iter().collect();
                if let Some(rest) = class.strip_prefix('!') {
                    class = format!("^{rest}");
                }
                re.push('[');
                re.push_str(&class.replace('\\', "\\\\"));
                re.push(']');
                i = end;
            }
            other => re.push_str(&regex::escape(&other.to_string())),
        }
        i += 1;
    }
    if in_group {
        bail!("unterminated {{ in glob {glob}");
    }
    re.push('$');
    Ok(Regex::new(&re)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_glob_semantics() {
        let g = glob_to_regex("smoke/*").unwrap();
        assert!(g.is_match("smoke/event-types.json"));
        assert!(!g.is_match("smoke/x/event-types.json"));
        assert!(glob_to_regex("**/crud.json")
            .unwrap()
            .is_match("roles/crud.json"));
        let alt = glob_to_regex("{auth,webauthn}/*.json").unwrap();
        assert!(alt.is_match("auth/mfa.json") && alt.is_match("webauthn/webauthn.json"));
        assert!(!alt.is_match("roles/crud.json"));
    }

    #[test]
    fn the_vendored_scenarios_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
        let all = load(&dir, None).unwrap();
        assert_eq!(all.len(), 45);
        assert_eq!(load(&dir, Some("smoke/*")).unwrap().len(), 1);
    }
}

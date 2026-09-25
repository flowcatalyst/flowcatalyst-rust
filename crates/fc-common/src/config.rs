//! Shared environment variable helper functions.
//!
//! All binaries should use these instead of defining their own.

use std::str::FromStr;

/// Read an env var or return the default.
pub fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Read an env var, trying the primary key first, then an alias.
/// This allows both TS-style (`PORT`) and Rust-style (`FC_API_PORT`) env vars.
pub fn env_or_alias(primary: &str, alias: &str, default: &str) -> String {
    std::env::var(primary)
        .or_else(|_| std::env::var(alias))
        .unwrap_or_else(|_| default.to_string())
}

/// Read an env var and parse it, or return the default.
pub fn env_or_parse<T: FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Read an env var (with alias) and parse it, or return the default.
pub fn env_or_alias_parse<T: FromStr>(primary: &str, alias: &str, default: T) -> T {
    std::env::var(primary)
        .or_else(|_| std::env::var(alias))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Read an env var as a boolean (`"true"` or `"1"` → true), or return the default.
pub fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(default)
}

/// Read an env var as a boolean with an alias fallback.
pub fn env_bool_alias(primary: &str, alias: &str, default: bool) -> bool {
    std::env::var(primary)
        .or_else(|_| std::env::var(alias))
        .ok()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(default)
}

/// Read a required env var, returning an error if missing.
pub fn env_required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("{} environment variable is required", key))
}

// ── env_first: N-way priority alias resolution ─────────────────────────────
//
// Mirrors Go's `envFirst`/`envBoolAlias`/`envIntAlias` pattern
// (flowcatalyst-go `internal/server/envcfg.go`): a single table of names in
// priority order, canonical `FC_*` name first, legacy/deployment-specific
// names as fallbacks. An empty value is treated the same as unset — a var
// present-but-empty in the environment does NOT win over a later, populated
// alias — so callers get the same "first *non-empty* value wins" behaviour
// Go's envFirst has. Every drop-in env-compat shim (standby, router auth,
// notify webhook, ports, …) should route through this family rather than
// hand-rolling its own `std::env::var(...).or_else(...)` chain, so the
// table of aliases lives in one place.

/// Return the first non-empty value among `keys` (priority order), or
/// `default` if none are set. `keys` is typically a `&[&str]` array literal
/// with the canonical `FC_*` name first.
pub fn env_first(keys: &[&str], default: &str) -> String {
    for key in keys {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return v;
            }
        }
    }
    default.to_string()
}

/// Like [`env_first`], but returns `None` instead of a default when no key
/// is set — for callers that need to distinguish "unset" from "empty
/// string default" (e.g. an optional webhook URL).
pub fn env_first_opt(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// [`env_first`] + boolean parsing (`"true"`/`"1"` → true, anything else
/// non-empty → false — same truthy rule as [`env_bool`]).
pub fn env_first_bool(keys: &[&str], default: bool) -> bool {
    for key in keys {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return v == "true" || v == "1";
            }
        }
    }
    default
}

/// Go's `envBool` truth table: `1/true/yes/on` and `0/false/no/off`,
/// case-insensitive and trimmed; anything else is `None` (the caller's
/// default stands).
pub fn parse_go_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Go's `envBoolAlias` generalised to N names: the first **non-empty**
/// name decides, parsed with [`parse_go_bool`]; an unrecognised value there
/// yields `default` — it does not fall through to the next name, exactly as
/// Go's `envBoolAlias(key, alias, def)` returns `envBool(key, def)` whenever
/// `key` is set. Use for the subsystem toggles a Go task definition sets
/// (`PLATFORM_ENABLED`, `MESSAGE_ROUTER_ENABLED`, …).
pub fn env_first_bool_go(keys: &[&str], default: bool) -> bool {
    for key in keys {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return parse_go_bool(&v).unwrap_or(default);
            }
        }
    }
    default
}

/// [`env_first`] + numeric/typed parsing. An unparseable non-empty value
/// is treated as unset and falls through to the next key, matching Go's
/// `envIntAlias` (a malformed value doesn't win over a good one further
/// down the alias chain).
pub fn env_first_parse<T: FromStr>(keys: &[&str], default: T) -> T {
    for key in keys {
        if let Ok(v) = std::env::var(key) {
            if v.is_empty() {
                continue;
            }
            if let Ok(parsed) = v.parse() {
                return parsed;
            }
        }
    }
    default
}

#[cfg(test)]
mod go_bool_tests {
    use super::parse_go_bool;

    #[test]
    fn go_truth_table() {
        for t in ["1", "true", "TRUE", " yes ", "On"] {
            assert_eq!(parse_go_bool(t), Some(true), "{t}");
        }
        for f in ["0", "false", "False", "no", "OFF"] {
            assert_eq!(parse_go_bool(f), Some(false), "{f}");
        }
        for u in ["", "maybe", "2", "enabled"] {
            assert_eq!(parse_go_bool(u), None, "{u}");
        }
    }
}

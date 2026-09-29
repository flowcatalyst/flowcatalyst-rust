//! Go's password-acceptance policy (`auth/passwordpolicy`), applied where the
//! portal plane sets a password: NIST SP 800-63B's shape — length, a
//! common-password blocklist, and "not your own identity" — with no
//! composition rules. `common_passwords.txt` is Go's embedded SecLists 10k
//! list, copied verbatim.

use std::collections::HashSet;
use std::sync::OnceLock;

pub const MIN_LENGTH: usize = 8;
pub const MAX_LENGTH: usize = 128;

const COMMON_PASSWORDS_RAW: &str = include_str!("common_passwords.txt");

fn common_passwords() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        COMMON_PASSWORDS_RAW
            .lines()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .collect()
    })
}

/// Why a password was rejected: a stable code and a user-safe message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub code: &'static str,
    pub message: String,
}

fn violation(code: &'static str, message: impl Into<String>) -> Option<Violation> {
    Some(Violation {
        code,
        message: message.into(),
    })
}

/// Check `pw` against the policy; `email` and `name` identify the account
/// (either may be empty). `None` when the password is acceptable. Lengths
/// are byte lengths, as Go's `len`.
pub fn validate(pw: &str, email: &str, name: &str) -> Option<Violation> {
    if pw.len() < MIN_LENGTH {
        return violation(
            "PASSWORD_TOO_SHORT",
            format!("Password must be at least {MIN_LENGTH} characters"),
        );
    }
    if pw.len() > MAX_LENGTH {
        return violation(
            "PASSWORD_TOO_LONG",
            format!("Password must be at most {MAX_LENGTH} characters"),
        );
    }
    let norm = pw.trim().to_lowercase();
    if all_same_char(&norm) {
        return violation(
            "PASSWORD_TOO_WEAK",
            "Password cannot be a single repeated character",
        );
    }
    if let Some(v) = identity_violation(&norm, email, name) {
        return Some(v);
    }
    if common_passwords().contains(norm.as_str()) {
        return violation(
            "PASSWORD_TOO_COMMON",
            "That password is on the list of most commonly used passwords — choose something less guessable",
        );
    }
    if norm.contains("flowcatalyst") {
        return violation(
            "PASSWORD_TOO_COMMON",
            "Password cannot be based on the product name",
        );
    }
    None
}

/// Whether the (lower-cased) password is derived from the account's email or
/// name, forwards or reversed. Candidates shorter than 4 characters only
/// match on equality.
fn identity_violation(norm_pw: &str, email: &str, name: &str) -> Option<Violation> {
    let reversed: String = norm_pw.chars().rev().collect();
    let mut candidates: Vec<String> = Vec::new();
    let e = email.trim().to_lowercase();
    if !e.is_empty() {
        if let Some(at) = e.find('@').filter(|at| *at > 0) {
            candidates.push(e.clone());
            candidates.push(e[..at].to_string());
        } else {
            candidates.push(e);
        }
    }
    candidates.extend(name.to_lowercase().split_whitespace().map(String::from));
    for c in candidates.iter().filter(|c| !c.is_empty()) {
        let hit = if c.len() >= 4 {
            norm_pw.contains(c.as_str()) || reversed.contains(c.as_str())
        } else {
            norm_pw == c || reversed == *c
        };
        if hit {
            return violation(
                "PASSWORD_CONTAINS_IDENTITY",
                "Password cannot contain your email address or name",
            );
        }
    }
    None
}

fn all_same_char(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => chars.all(|c| c == first),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(pw: &str, email: &str, name: &str) -> Option<&'static str> {
        validate(pw, email, name).map(|v| v.code)
    }

    #[test]
    fn follows_gos_rules() {
        assert_eq!(code("short", "", ""), Some("PASSWORD_TOO_SHORT"));
        assert_eq!(code(&"a".repeat(129), "", ""), Some("PASSWORD_TOO_LONG"));
        assert_eq!(code("aaaaaaaaaa", "", ""), Some("PASSWORD_TOO_WEAK"));
        assert_eq!(code("password", "", ""), Some("PASSWORD_TOO_COMMON"));
        assert_eq!(
            code("my-flowcatalyst-pw", "", ""),
            Some("PASSWORD_TOO_COMMON")
        );
        assert_eq!(
            code("xx-andrew-xx-9", "andrew@example.com", ""),
            Some("PASSWORD_CONTAINS_IDENTITY")
        );
        assert_eq!(
            code("werdna-backwards", "andrew@example.com", ""),
            Some("PASSWORD_CONTAINS_IDENTITY")
        );
        assert_eq!(
            code("Jonas-Battery-7712", "", "Jonas Smith"),
            Some("PASSWORD_CONTAINS_IDENTITY")
        );
        assert_eq!(
            code("Correct-Harness-Battery-7712", "p@example.com", ""),
            None
        );
    }
}

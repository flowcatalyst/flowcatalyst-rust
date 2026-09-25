//! One side's captured values and built-ins (spec §3). Built once per side
//! per scenario; every value a side's responses can be compared "by role"
//! instead of "by value" (rule 1) lives here.
//!
//! The built-ins `${client.id}` / `${app.id}` / `${admin.id}` and `${run}` are
//! the same text on both sides (read from `seed` before the clones were made,
//! or generated once per run); every captured value is per side.

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use indexmap::IndexMap;
use rand::Rng;
use sha2::{Digest, Sha256};

use crate::totp;

/// The ids read from `seed` before either clone was made.
#[derive(Debug, Clone)]
pub struct SeedIds {
    pub client_id: String,
    pub app_id: String,
    pub admin_id: String,
}

#[derive(Debug)]
pub struct Vars {
    /// name → value, this scenario's own captures (last capture of a name wins).
    captures: IndexMap<String, String>,
    admin_email: String,
    admin_password: String,
    run: String,
    ids: SeedIds,
    pkce_verifier: String,
    pkce_challenge: String,
    /// value → name, every value captured on this side across the whole run,
    /// so rule 1 also masks an id a *previous* scenario created. Moved in at
    /// the start of a scenario and taken back out at the end ([`Vars::into_run_labels`]).
    run_labels: IndexMap<String, String>,
}

impl Vars {
    pub fn new(
        admin_email: &str,
        admin_password: &str,
        run: &str,
        ids: SeedIds,
        run_labels: IndexMap<String, String>,
    ) -> Self {
        let pkce_verifier = generate_verifier();
        let pkce_challenge = s256(&pkce_verifier);
        Self {
            captures: IndexMap::new(),
            admin_email: admin_email.to_string(),
            admin_password: admin_password.to_string(),
            run: run.to_string(),
            ids,
            pkce_verifier,
            pkce_challenge,
            run_labels,
        }
    }

    pub fn into_run_labels(self) -> IndexMap<String, String> {
        self.run_labels
    }

    /// Records a captured value under `name`, overwriting an earlier capture
    /// of the same name.
    pub fn capture(&mut self, name: &str, value: &str) {
        self.captures.insert(name.to_string(), value.to_string());
        self.run_labels.insert(value.to_string(), name.to_string());
    }

    /// An automatic capture (`auto:<member>`): recorded for masking only, and
    /// never over an explicit capture of the same value.
    pub fn capture_quietly(&mut self, name: &str, value: &str) {
        if self.captures.values().any(|v| v == value) || self.run_labels.contains_key(value) {
            return;
        }
        self.captures.insert(name.to_string(), value.to_string());
        self.run_labels.insert(value.to_string(), name.to_string());
    }

    /// value → capture name, for rule 1: this scenario's own captures win,
    /// then anything captured earlier in the run on this side. Iteration
    /// order is Java's `LinkedHashMap` order (re-insertion keeps position).
    pub fn labels(&self) -> IndexMap<String, String> {
        let mut out = self.run_labels.clone();
        for (name, value) in &self.captures {
            out.insert(value.clone(), name.clone());
        }
        out
    }

    pub fn captured(&self, name: &str) -> Option<&str> {
        self.captures.get(name).map(String::as_str)
    }

    /// Resolves one `${…}` name: built-ins first, then the dynamic forms
    /// (`totp:`, `b64url:`), then a plain capture. Undefined is an error,
    /// never an empty string.
    pub fn resolve(&self, key: &str) -> Result<String> {
        let v = match key {
            "admin.email" => self.admin_email.clone(),
            "admin.password" => self.admin_password.clone(),
            "run" => self.run.clone(),
            "client.id" => self.ids.client_id.clone(),
            "app.id" => self.ids.app_id.clone(),
            "admin.id" => self.ids.admin_id.clone(),
            "pkce.verifier" => self.pkce_verifier.clone(),
            "pkce.challenge" => self.pkce_challenge.clone(),
            _ => return self.resolve_dynamic(key),
        };
        Ok(v)
    }

    fn resolve_dynamic(&self, key: &str) -> Result<String> {
        let undefined = || anyhow!("undefined substitution: ${{{key}}}");
        if let Some(rest) = key.strip_prefix("totp:") {
            // `totp:<var>` or `totp:<var>:<step offset>`: the offset (-1, 0, +1)
            // picks an adjacent 30-second step inside the ±1 window both sides
            // accept, so two steps can present two different codes without waiting.
            let (secret_var, offset) = match rest.split_once(':') {
                Some((var, off)) => (
                    var,
                    off.parse::<i64>()
                        .map_err(|_| anyhow!("bad totp offset in ${{{key}}}"))?,
                ),
                None => (rest, 0),
            };
            let secret = self.captured(secret_var).ok_or_else(undefined)?;
            return totp::code(secret, totp::current_step() + offset);
        }
        if let Some(var) = key.strip_prefix("b64url:") {
            let value = self.captured(var).ok_or_else(undefined)?;
            return Ok(URL_SAFE_NO_PAD.encode(value.as_bytes()));
        }
        self.captured(key).map(str::to_string).ok_or_else(undefined)
    }
}

/// A fresh RFC 7636 code verifier: 43 characters from the unreserved alphabet.
fn generate_verifier() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::rng();
    (0..43)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// RFC 7636 `S256`: `BASE64URL(SHA256(ASCII(verifier)))`, no padding.
fn s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        Vars::new(
            "a@example.com",
            "pw",
            "run1",
            SeedIds {
                client_id: "clt_1".into(),
                app_id: "app_1".into(),
                admin_id: "prn_1".into(),
            },
            IndexMap::new(),
        )
    }

    #[test]
    fn builtins_and_captures_resolve_and_undefined_is_an_error() {
        let mut v = vars();
        assert_eq!(v.resolve("client.id").unwrap(), "clt_1");
        assert!(v.resolve("nope").is_err());
        v.capture("etId", "evt_0ABCDEFGHIJ");
        assert_eq!(v.resolve("etId").unwrap(), "evt_0ABCDEFGHIJ");
        assert_eq!(v.resolve("b64url:etId").unwrap(), "ZXZ0XzBBQkNERUZHSElK");
        assert_eq!(v.resolve("pkce.challenge").unwrap(), s256(&v.pkce_verifier));
    }

    #[test]
    fn a_quiet_capture_never_overrides_an_explicit_one() {
        let mut v = vars();
        v.capture("etId", "evt_0ABCDEFGHIJ");
        v.capture_quietly("auto:id", "evt_0ABCDEFGHIJ");
        assert_eq!(v.labels().get("evt_0ABCDEFGHIJ").unwrap(), "etId");
        assert!(v.captured("auto:id").is_none());
    }
}

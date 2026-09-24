//! sha256 over a desired-state entry's sorted `config` + `secrets` (Java
//! `fnhost/context/SettingsFingerprint.java`): a different fingerprint in a
//! new document reloads the function in place, same version, new settings.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::java::utf16_len;

/// `config\n` + each `len:key=len:value\n` (lengths in UTF-16 units, as
/// Java's `String.length`), then `secrets\n` + the same, sorted by key.
pub fn settings_fingerprint(
    config: &BTreeMap<String, String>,
    secrets: &BTreeMap<String, String>,
) -> String {
    let mut canonical = String::from("config\n");
    append_sorted(&mut canonical, config);
    canonical.push_str("secrets\n");
    append_sorted(&mut canonical, secrets);
    hex::encode(Sha256::digest(canonical.as_bytes()))
}

fn append_sorted(out: &mut String, map: &BTreeMap<String, String>) {
    for (key, value) in map {
        out.push_str(&format!(
            "{}:{key}={}:{value}\n",
            utf16_len(key),
            utf16_len(value)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_sensitive_to_every_value() {
        let config: BTreeMap<_, _> = [
            ("b".to_owned(), "2".to_owned()),
            ("a".to_owned(), "1".to_owned()),
        ]
        .into();
        let secrets = BTreeMap::new();
        let a = settings_fingerprint(&config, &secrets);
        assert_eq!(a, settings_fingerprint(&config.clone(), &secrets));
        let mut changed = config.clone();
        changed.insert("a".into(), "9".into());
        assert_ne!(a, settings_fingerprint(&changed, &secrets));
        // a key moved from config to secrets is a different fingerprint
        let moved_config: BTreeMap<_, _> = [("b".to_owned(), "2".to_owned())].into();
        let moved_secrets: BTreeMap<_, _> = [("a".to_owned(), "1".to_owned())].into();
        assert_ne!(a, settings_fingerprint(&moved_config, &moved_secrets));
    }

    #[test]
    fn matches_javas_canonical_form() {
        // Java: sha256("config\n1:a=1:1\nsecrets\n")
        let config: BTreeMap<_, _> = [("a".to_owned(), "1".to_owned())].into();
        let expected = hex::encode(Sha256::digest(b"config\n1:a=1:1\nsecrets\n"));
        assert_eq!(settings_fingerprint(&config, &BTreeMap::new()), expected);
    }
}

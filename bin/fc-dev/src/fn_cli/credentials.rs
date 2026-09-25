//! Where `fc-dev fn` gets its platform credentials (Java `FnCredentials`).
//! First complete match wins:
//!
//! 1. `--client-id` / `--client-secret` (with `--platform-url`, or
//!    `FLOWCATALYST_PLATFORM_URL`, or the file's);
//! 2. `FLOWCATALYST_CLIENT_ID` / `FLOWCATALYST_CLIENT_SECRET` (the same
//!    URL rule);
//! 3. `fn-cli.json`, written by a running `fc-dev` (also the only source of
//!    `hostUrl`, `fn invoke`'s default target);
//! 4. an error naming the three places looked in.
//!
//! `fc-dev fn` does not read `.env` / `.env.development`: a project's
//! `.env` from `fc-dev init` carries its application's service account,
//! not a function publisher.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::CliError;

/// `fn-cli.json` (Java `StartCommand.FnCliCredentialsFile`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
}

impl CliFile {
    /// The file, or `None` when it is absent, unreadable or corrupt (then
    /// the caller falls through to its other sources).
    pub fn read(path: &Path) -> Option<CliFile> {
        let bytes = std::fs::read(path).ok()?;
        let mut file: CliFile = serde_json::from_slice(&bytes).ok()?;
        for field in [
            &mut file.platform_url,
            &mut file.client_id,
            &mut file.client_secret,
            &mut file.host_url,
            &mut file.public_url,
        ] {
            if field.as_deref().is_some_and(|v| v.trim().is_empty()) {
                *field = None;
            }
        }
        Some(file)
    }
}

/// A resolved credential set.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub platform_url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("platform_url", &self.platform_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// The flags, as given.
#[derive(Debug, Default, Clone)]
pub struct Flags<'a> {
    pub platform_url: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub client_secret: Option<&'a str>,
}

pub fn resolve(
    flags: &Flags<'_>,
    env: &dyn Fn(&str) -> Option<String>,
    file_path: &Path,
) -> Result<Credentials, CliError> {
    let env = |key: &str| env(key).filter(|v| !v.trim().is_empty());
    let flag = |v: Option<&str>| v.filter(|v| !v.trim().is_empty()).map(str::to_string);
    let url = flag(flags.platform_url).or_else(|| env("FLOWCATALYST_PLATFORM_URL"));
    let file = CliFile::read(file_path);
    let file_url = || file.as_ref().and_then(|f| f.platform_url.clone());

    let id = flag(flags.client_id).or_else(|| env("FLOWCATALYST_CLIENT_ID"));
    let secret = flag(flags.client_secret).or_else(|| env("FLOWCATALYST_CLIENT_SECRET"));
    if let (Some(client_id), Some(client_secret)) = (id, secret) {
        let platform_url = url.or_else(file_url).ok_or_else(|| missing(file_path))?;
        return Ok(Credentials {
            platform_url: trim_slash(platform_url),
            client_id,
            client_secret,
        });
    }
    if let Some(CliFile {
        client_id: Some(client_id),
        client_secret: Some(client_secret),
        ..
    }) = file.clone()
    {
        let platform_url = url.or_else(file_url).ok_or_else(|| missing(file_path))?;
        return Ok(Credentials {
            platform_url: trim_slash(platform_url),
            client_id,
            client_secret,
        });
    }
    Err(missing(file_path))
}

/// `fn init`'s `$schema` base: never fails (flag, env, file, else the
/// local fc-dev's default).
pub fn platform_url_or_default(
    flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    file_path: &Path,
) -> String {
    flag.filter(|v| !v.trim().is_empty())
        .map(str::to_string)
        .or_else(|| env("FLOWCATALYST_PLATFORM_URL").filter(|v| !v.trim().is_empty()))
        .or_else(|| CliFile::read(file_path).and_then(|f| f.platform_url))
        .map(trim_slash)
        .unwrap_or_else(|| "http://localhost:8080".to_string())
}

fn missing(file_path: &Path) -> CliError {
    CliError::Other(format!(
        "no FlowCatalyst credentials found; looked for: (1) --client-id/--client-secret flags, \
         (2) FLOWCATALYST_CLIENT_ID/FLOWCATALYST_CLIENT_SECRET environment variables, (3) {} \
         (written by a running fc-dev)",
        file_path.display()
    ))
}

pub fn trim_slash(url: String) -> String {
    url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn write(dir: &Path, file: &CliFile) -> std::path::PathBuf {
        let path = dir.join("fn-cli.json");
        std::fs::write(&path, serde_json::to_vec(file).unwrap()).unwrap();
        path
    }

    fn file() -> CliFile {
        CliFile {
            platform_url: Some("http://localhost:8080".into()),
            client_id: Some("fcdev-fn-cli".into()),
            client_secret: Some("from-file".into()),
            host_url: Some("http://127.0.0.1:8090".into()),
            public_url: None,
        }
    }

    #[test]
    fn flags_win_over_env_and_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &file());
        let flags = Flags {
            platform_url: None,
            client_id: Some("flag-id"),
            client_secret: Some("flag-secret"),
        };
        let env = env_of(&[
            ("FLOWCATALYST_CLIENT_ID", "env-id"),
            ("FLOWCATALYST_CLIENT_SECRET", "env-secret"),
        ]);
        let creds = resolve(&flags, &env, &path).unwrap();
        assert_eq!(creds.client_id, "flag-id");
        assert_eq!(creds.client_secret, "flag-secret");
        // The URL falls back to the file's.
        assert_eq!(creds.platform_url, "http://localhost:8080");
    }

    #[test]
    fn env_wins_over_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &file());
        let env = env_of(&[
            ("FLOWCATALYST_CLIENT_ID", "env-id"),
            ("FLOWCATALYST_CLIENT_SECRET", "env-secret"),
            ("FLOWCATALYST_PLATFORM_URL", "http://platform:9000/"),
        ]);
        let creds = resolve(&Flags::default(), &env, &path).unwrap();
        assert_eq!(creds.client_id, "env-id");
        assert_eq!(creds.platform_url, "http://platform:9000");
    }

    #[test]
    fn the_file_is_the_last_resort() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &file());
        let creds = resolve(&Flags::default(), &env_of(&[]), &path).unwrap();
        assert_eq!(creds.client_id, "fcdev-fn-cli");
        assert_eq!(creds.client_secret, "from-file");
    }

    #[test]
    fn nothing_anywhere_names_every_place_looked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fn-cli.json");
        let err = resolve(&Flags::default(), &env_of(&[]), &path).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("--client-id"), "{message}");
        assert!(message.contains("FLOWCATALYST_CLIENT_ID"), "{message}");
        assert!(message.contains("fn-cli.json"), "{message}");
    }

    #[test]
    fn a_corrupt_file_is_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fn-cli.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(CliFile::read(&path).is_none());
        assert_eq!(
            platform_url_or_default(None, &env_of(&[]), &path),
            "http://localhost:8080"
        );
    }
}

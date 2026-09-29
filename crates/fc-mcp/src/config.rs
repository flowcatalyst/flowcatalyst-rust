//! MCP server configuration.
//!
//! Resolution order, top to bottom:
//!   1. Explicit env vars (`FLOWCATALYST_URL`, then Go's `FC_MCP_PLATFORM_URL`;
//!      `FLOWCATALYST_CLIENT_ID`, `FLOWCATALYST_CLIENT_SECRET`) — power-user /
//!      CI escape hatch, and how a deployed `fc-server` (`FC_MCP_ENABLED`)
//!      is configured.
//!   2. `~/.cache/flowcatalyst-dev/mcp-credentials.json` — written by
//!      `fc-dev`'s mcp-bootstrap step on startup.
//!   3. Defaults — `base_url` only (`http://localhost:8080`, or the caller's
//!      own listener for [`Config::from_env_or_base`]).
//!
//! Missing `client_id`/`client_secret` after walking those is fatal:
//! we surface a "start fc-dev first" message rather than silently
//! sending empty credentials.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::net::IpAddr;
use std::net::SocketAddr;

const DEFAULT_BASE_URL: &str = "http://localhost:8080";

#[derive(Clone, Debug)]
pub struct Config {
    pub base_url: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Deserialize)]
struct CredentialsFile {
    client_id: String,
    client_secret: String,
    #[serde(default)]
    base_url: Option<String>,
}

impl Config {
    /// Read env first, fall back to the credentials file, and finally
    /// surface a clear error pointing at fc-dev if no creds were found.
    pub fn from_env() -> Result<Self> {
        Self::from_env_or_base(DEFAULT_BASE_URL)
    }

    /// [`Config::from_env`] with `default_base_url` when neither the
    /// environment nor the credentials file names the platform (Go's
    /// in-process MCP dials its own API listener).
    pub fn from_env_or_base(default_base_url: &str) -> Result<Self> {
        let env_base_url = env_opt("FLOWCATALYST_URL").or_else(|| env_opt("FC_MCP_PLATFORM_URL"));
        let env_client_id = env_opt("FLOWCATALYST_CLIENT_ID");
        let env_client_secret = env_opt("FLOWCATALYST_CLIENT_SECRET");

        // Only look at the file if either credential is missing — env
        // values always win.
        let file = if env_client_id.is_none() || env_client_secret.is_none() {
            read_credentials_file()?
        } else {
            None
        };

        let client_id = env_client_id
            .or_else(|| file.as_ref().map(|f| f.client_id.clone()))
            .ok_or_else(missing_creds_error)?;
        let client_secret = env_client_secret
            .or_else(|| file.as_ref().map(|f| f.client_secret.clone()))
            .ok_or_else(missing_creds_error)?;
        let base_url = env_base_url
            .or_else(|| file.as_ref().and_then(|f| f.base_url.clone()))
            .unwrap_or_else(|| default_base_url.to_string())
            .trim_end_matches('/')
            .to_owned();

        Ok(Self {
            base_url,
            client_id,
            client_secret,
        })
    }
}

fn env_opt(name: &str) -> Option<String> {
    env::var(name).ok().filter(|s| !s.is_empty())
}

/// Where `fc-dev`'s `mcp_bootstrap::write_credentials_file` writes them.
/// Returns `None` only on platforms where `dirs::cache_dir()` is
/// unreachable; on those, the user must use env vars.
fn credentials_path() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("flowcatalyst-dev")
            .join("mcp-credentials.json"),
    )
}

fn read_credentials_file() -> Result<Option<CredentialsFile>> {
    let Some(path) = credentials_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let parsed: CredentialsFile = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "parsing {} (regenerate with `fc-dev` restart)",
            path.display()
        )
    })?;
    Ok(Some(parsed))
}

fn missing_creds_error() -> anyhow::Error {
    let hint = credentials_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    anyhow!(
        "no MCP credentials found.\n\n\
         Start the fc-dev server in another terminal first:\n  \
           ./fc-dev\n\n\
         It will provision credentials at {hint} and the MCP server will pick \
         them up automatically on the next launch.\n\n\
         Alternatively, set FLOWCATALYST_CLIENT_ID and FLOWCATALYST_CLIENT_SECRET \
         (and optionally FLOWCATALYST_URL) directly.",
    )
}

/// The MCP HTTP listener's address: `bind` as a full `host:port`, or a bare
/// host (Go's `FC_MCP_BIND`, default `127.0.0.1`) joined with `port` (Go's
/// `FC_MCP_PORT`).
pub fn resolve_bind(bind: &str, port: u16) -> Result<SocketAddr> {
    let bind = bind.trim();
    if let Ok(addr) = bind.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let host = bind.trim_start_matches('[').trim_end_matches(']');
    let ip: IpAddr = host
        .parse()
        .with_context(|| format!("MCP bind address {bind:?} is not an IP or IP:port"))?;
    Ok(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::resolve_bind;

    #[test]
    fn a_bind_is_a_host_and_port_or_a_host_with_the_port() {
        assert_eq!(
            resolve_bind("127.0.0.1:3100", 8090).unwrap().to_string(),
            "127.0.0.1:3100"
        );
        assert_eq!(
            resolve_bind("0.0.0.0", 8090).unwrap().to_string(),
            "0.0.0.0:8090"
        );
        assert_eq!(resolve_bind("::1", 8090).unwrap().to_string(), "[::1]:8090");
        assert!(resolve_bind("localhost", 8090).is_err());
    }
}

//! The MCP role (`FC_MCP_ENABLED`, Go's `StartMCP`): the read-only
//! FlowCatalyst MCP server (`crates/fc-mcp`) as a streamable-HTTP service on
//! its own listener, `/mcp` plus `GET /health`. It reaches the platform over
//! HTTP with OAuth `client_credentials`, never through the database, so an
//! MCP-only node connects to no Postgres.
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `FC_MCP_ENABLED` | `false` | Run the MCP server |
//! | `FC_MCP_BIND` | `127.0.0.1` | Bind host (localhost only unless set; a `host:port` is also accepted) |
//! | `FC_MCP_PORT` | `8090` | Listener port |
//! | `FLOWCATALYST_URL` / `FC_MCP_PLATFORM_URL` | `http://localhost:{FC_API_PORT}` | The platform it calls |
//! | `FLOWCATALYST_CLIENT_ID` / `FLOWCATALYST_CLIENT_SECRET` | - | Its `client_credentials` client (else fc-dev's credentials file) |

use std::net::SocketAddr;

use anyhow::{Context, Result};
use fc_common::config::{env_or, env_or_parse};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Go's defaults (`internal/server/envcfg.go`).
const DEFAULT_BIND: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8090;

/// The MCP role, configured.
pub struct McpRole {
    config: fc_mcp::Config,
    addr: SocketAddr,
}

impl McpRole {
    /// Reads the role's environment. Refuses (an error at boot) when no
    /// credentials are configured: Go would start unauthenticated, but
    /// every tool call against a production platform would then be a 401.
    pub fn from_env(api_port: u16) -> Result<Self> {
        let addr = fc_mcp::resolve_bind(
            &env_or("FC_MCP_BIND", DEFAULT_BIND),
            env_or_parse("FC_MCP_PORT", DEFAULT_PORT),
        )?;
        let config = fc_mcp::Config::from_env_or_base(&format!("http://localhost:{api_port}"))
            .context(
                "FC_MCP_ENABLED=true needs FLOWCATALYST_CLIENT_ID and FLOWCATALYST_CLIENT_SECRET",
            )?;
        Ok(Self { config, addr })
    }

    /// The listener's port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Binds the listener and serves until `stop` is cancelled.
    pub async fn start(self, stop: CancellationToken) -> Result<tokio::task::JoinHandle<()>> {
        let listener = TcpListener::bind(self.addr)
            .await
            .with_context(|| format!("MCP listener on {}", self.addr))?;
        info!(addr = %self.addr, platform_url = %self.config.base_url, "MCP server starting");
        Ok(tokio::spawn(async move {
            if let Err(e) = fc_mcp::serve_http(self.config, listener, stop.cancelled_owned()).await
            {
                warn!(error = %e, "MCP listener stopped with an error");
            }
        }))
    }
}

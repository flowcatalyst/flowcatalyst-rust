//! The failures the MCP server can report, each keeping its cause.
//!
//! Every variant's `Display` is the text the `anyhow` errors it replaces
//! rendered: tool results and `internal_error` data carry `e.to_string()`
//! to the MCP client, and the launching binaries print the error.

use std::io;
use std::net::AddrParseError;
use std::path::PathBuf;
use std::result;

use reqwest::StatusCode;
use rmcp::service::ServerInitializeError;
use tokio::task::JoinError;

pub type Result<T, E = Error> = result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An HTTP call to the platform, or building a client for one.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// Binding or serving the listener.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The MCP handshake with the client failed.
    #[error(transparent)]
    Serve(Box<ServerInitializeError>),
    /// The MCP service task panicked or was cancelled.
    #[error(transparent)]
    ServiceTask(#[from] JoinError),
    /// The token endpoint could not be reached.
    #[error("token request failed: {0}")]
    TokenRequest(#[source] reqwest::Error),
    /// The token endpoint answered with a non-success status.
    #[error("token fetch failed ({status}): {body}")]
    TokenRejected { status: StatusCode, body: String },
    /// The platform answered a tool's request with a non-success status.
    #[error("platform request failed ({status}): {body}")]
    PlatformRejected { status: StatusCode, body: String },
    /// An application document had no `id`.
    #[error("application '{code}' has no id")]
    ApplicationHasNoId { code: String },
    /// Neither the environment nor the credentials file gave credentials.
    #[error(
        "no MCP credentials found.\n\n\
         Start the fc-dev server in another terminal first:\n  \
           ./fc-dev\n\n\
         It will provision credentials at {hint} and the MCP server will pick \
         them up automatically on the next launch.\n\n\
         Alternatively, set FLOWCATALYST_CLIENT_ID and FLOWCATALYST_CLIENT_SECRET \
         (and optionally FLOWCATALYST_URL) directly."
    )]
    MissingCredentials { hint: String },
    /// The credentials file exists but could not be read.
    #[error("reading {}", path.display())]
    CredentialsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The credentials file is not valid.
    #[error("parsing {} (regenerate with `fc-dev` restart)", path.display())]
    CredentialsParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// A bind address that is neither `host:port` nor a bare IP.
    #[error("MCP bind address {bind:?} is not an IP or IP:port")]
    BindAddress {
        bind: String,
        #[source]
        source: AddrParseError,
    },
    /// The readiness probe got a non-success status.
    #[error(
        "FlowCatalyst at {base_url} replied {status} to /q/ready — the platform is reachable but not ready"
    )]
    PlatformNotReady {
        base_url: String,
        status: StatusCode,
    },
    /// The readiness probe could not connect.
    #[error(
        "Cannot reach FlowCatalyst at {base_url}: {source}\n\n\
         Start the dev server in another terminal first:\n  ./fc-dev"
    )]
    PlatformUnreachable {
        base_url: String,
        #[source]
        source: reqwest::Error,
    },
}

// Boxed: the handshake error is several hundred bytes, which would make every
// `Result` in the crate that large.
impl From<ServerInitializeError> for Error {
    fn from(e: ServerInitializeError) -> Self {
        Self::Serve(Box::new(e))
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use reqwest::StatusCode;
    use std::error::Error as _;
    use std::io;
    use std::net::IpAddr;
    use std::path::PathBuf;

    // The expected strings are what the replaced `anyhow!` / `bail!` /
    // `with_context` calls produced.

    #[test]
    fn the_status_messages_are_unchanged() {
        let e = Error::TokenRejected {
            status: StatusCode::UNAUTHORIZED,
            body: "nope".into(),
        };
        assert_eq!(e.to_string(), "token fetch failed (401 Unauthorized): nope");
        let e = Error::PlatformRejected {
            status: StatusCode::NOT_FOUND,
            body: "{}".into(),
        };
        assert_eq!(e.to_string(), "platform request failed (404 Not Found): {}");
        let e = Error::ApplicationHasNoId { code: "x".into() };
        assert_eq!(e.to_string(), "application 'x' has no id");
        let e = Error::PlatformNotReady {
            base_url: "http://h".into(),
            status: StatusCode::SERVICE_UNAVAILABLE,
        };
        assert_eq!(
            e.to_string(),
            "FlowCatalyst at http://h replied 503 Service Unavailable to /q/ready — the platform is reachable but not ready"
        );
    }

    #[test]
    fn the_credentials_hint_is_unchanged() {
        let e = Error::MissingCredentials {
            hint: "/c/m.json".into(),
        };
        assert_eq!(
            e.to_string(),
            "no MCP credentials found.\n\nStart the fc-dev server in another terminal first:\n  ./fc-dev\n\nIt will provision credentials at /c/m.json and the MCP server will pick them up automatically on the next launch.\n\nAlternatively, set FLOWCATALYST_CLIENT_ID and FLOWCATALYST_CLIENT_SECRET (and optionally FLOWCATALYST_URL) directly."
        );
    }

    #[test]
    fn the_file_and_bind_errors_keep_their_context_text_and_cause() {
        let io = io::Error::from(io::ErrorKind::NotFound);
        let e = Error::CredentialsRead {
            path: PathBuf::from("/c/m.json"),
            source: io,
        };
        assert_eq!(e.to_string(), "reading /c/m.json");
        assert!(e.source().is_some());
        let json = serde_json::from_str::<u8>("x").unwrap_err();
        let e = Error::CredentialsParse {
            path: PathBuf::from("/c/m.json"),
            source: json,
        };
        assert_eq!(
            e.to_string(),
            "parsing /c/m.json (regenerate with `fc-dev` restart)"
        );
        assert!(e.source().is_some());
        let bad = "nope".parse::<IpAddr>().unwrap_err();
        let e = Error::BindAddress {
            bind: "localhost".into(),
            source: bad,
        };
        assert_eq!(
            e.to_string(),
            r#"MCP bind address "localhost" is not an IP or IP:port"#
        );
        assert!(e.source().is_some());
    }
}

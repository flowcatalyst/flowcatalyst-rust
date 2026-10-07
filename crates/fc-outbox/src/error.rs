//! What the outbox processor and its repositories report.
//!
//! Every variant's `Display` is the text of the `anyhow` error it replaces.
//! Two of those texts are persisted: the processor writes a failed token
//! mint into the outbox row as `auth: {error}`, and the repositories' errors
//! reach the processor's warn/error log lines.

use std::io;
use std::result;

use reqwest::StatusCode;

#[cfg(feature = "mongo")]
use mongodb::error::Error as MongoError;

use crate::backend::OutboxBackend;
use fc_common::UnknownOutboxItemType;

pub type Result<T, E = Error> = result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A SQL backend failed (connecting, querying, creating the schema).
    #[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql", test))]
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    /// The MongoDB backend failed.
    #[cfg(feature = "mongo")]
    #[error(transparent)]
    Mongo(#[from] MongoError),
    /// An outbox row's `type` is not one of the item types.
    #[error(transparent)]
    UnknownItemType(#[from] UnknownOutboxItemType),
    /// A configured table name that is not a plain SQL identifier.
    #[error("invalid outbox table name {table:?}")]
    InvalidTableName { table: String },
    /// A processing timeout too large to compare against timestamps.
    #[error(transparent)]
    TimeoutOutOfRange(#[from] chrono::OutOfRangeError),
    /// Building the HTTP client for the platform.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// Binding the admin listener.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The backend was not compiled into this binary.
    #[error(
        "the {backend} outbox backend is not built into this binary (fc-outbox-processor has \
         every backend)"
    )]
    BackendNotBuilt { backend: OutboxBackend },
    /// The token endpoint could not be reached.
    #[error("token request: {0}")]
    TokenRequest(#[source] reqwest::Error),
    /// The token endpoint refused the request.
    #[error("token endpoint {status}: {body}")]
    TokenEndpoint { status: StatusCode, body: String },
    /// The token endpoint's answer was not the expected JSON.
    #[error("decode token response: {0}")]
    TokenDecode(#[source] serde_json::Error),
    /// The token endpoint's answer carried no token.
    #[error("token endpoint returned no access_token")]
    TokenMissing,
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::backend::OutboxBackend;
    use reqwest::StatusCode;
    use std::error::Error as _;

    // The expected strings are what the replaced `anyhow!` / `bail!` calls
    // produced; `auth: {e}` is persisted on the outbox row.
    #[test]
    fn the_messages_are_unchanged() {
        assert_eq!(
            Error::InvalidTableName {
                table: "a b".into()
            }
            .to_string(),
            r#"invalid outbox table name "a b""#
        );
        assert_eq!(
            Error::BackendNotBuilt { backend: OutboxBackend::Mongo }.to_string(),
            "the mongo outbox backend is not built into this binary (fc-outbox-processor has every backend)"
        );
        assert_eq!(
            Error::TokenEndpoint {
                status: StatusCode::UNAUTHORIZED,
                body: "no".into()
            }
            .to_string(),
            "token endpoint 401 Unauthorized: no"
        );
        assert_eq!(
            Error::TokenMissing.to_string(),
            "token endpoint returned no access_token"
        );
    }

    #[test]
    fn the_decode_message_keeps_the_cause() {
        let cause = serde_json::from_str::<u8>("x").unwrap_err();
        let text = cause.to_string();
        let e = Error::TokenDecode(cause);
        assert_eq!(e.to_string(), format!("decode token response: {text}"));
        assert!(e.source().is_some());
    }

    #[test]
    fn a_sql_error_renders_as_the_driver_does() {
        let e = Error::from(sqlx::Error::PoolClosed);
        assert_eq!(e.to_string(), sqlx::Error::PoolClosed.to_string());
    }
}

//! Which database the outbox processor reads from.

use std::fmt;
use std::str::FromStr;

/// Outbox storage backend, chosen by the `FC_OUTBOX_DB_TYPE` setting.
///
/// Parsed once at startup. Accepted spellings are exactly the lowercase
/// names below; anything else is a startup error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxBackend {
    Sqlite,
    Postgres,
    Mongo,
}

impl OutboxBackend {
    pub const ALL: [OutboxBackend; 3] = [
        OutboxBackend::Sqlite,
        OutboxBackend::Postgres,
        OutboxBackend::Mongo,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            OutboxBackend::Sqlite => "sqlite",
            OutboxBackend::Postgres => "postgres",
            OutboxBackend::Mongo => "mongo",
        }
    }
}

impl fmt::Display for OutboxBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `FC_OUTBOX_DB_TYPE` named a backend that doesn't exist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown outbox database type: {0:?}. Use sqlite, postgres, or mongo")]
pub struct UnknownOutboxBackend(pub String);

impl FromStr for OutboxBackend {
    type Err = UnknownOutboxBackend;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|b| b.as_str() == s)
            .ok_or_else(|| UnknownOutboxBackend(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_accepted_spellings() {
        assert_eq!("sqlite".parse(), Ok(OutboxBackend::Sqlite));
        assert_eq!("postgres".parse(), Ok(OutboxBackend::Postgres));
        assert_eq!("mongo".parse(), Ok(OutboxBackend::Mongo));
    }

    #[test]
    fn rejects_anything_else() {
        for bad in ["Postgres", "postgresql", "mysql", ""] {
            assert!(bad.parse::<OutboxBackend>().is_err(), "{bad}");
        }
    }
}
